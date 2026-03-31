use anyhow::{Context, Result};

use crate::ai;
use crate::config;
use crate::db;
use crate::git;
use crate::github;
use crate::ivc_json;
use crate::models::commit_capture::{CommitCapture, DiffStats};
use crate::models::event::{Event, EventType};
use crate::models::intention::{Intention, IntentionTree};

pub struct PrArgs {
    pub base: String,
    pub draft: bool,
    pub no_push: bool,
    pub no_pr: bool,
}

pub async fn run(args: PrArgs) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let repo = git::repo::open_repo(&cwd)?;
    git::repo::require_ivc_initialised(&repo)?;

    // Resolve base branch: CLI arg > config > default "main"
    let ivc_dir = git::repo::get_ivc_dir(&repo)?;
    let cfg = config::load_config(&ivc_dir)?;
    let base = if args.base.is_empty() {
        cfg.git.default_base.clone()
    } else {
        args.base.clone()
    };

    let branch = git::repo::get_current_branch(&repo)?;
    let repo_name = git::repo::get_repo_name(&repo)?;

    // Check if we are on the base branch
    if branch == base {
        anyhow::bail!(
            "You are on the base branch '{base}'. Switch to a feature branch first."
        );
    }

    // Get commits since divergence
    let commits = git::branch::get_commits_since_divergence(&repo, &base)
        .with_context(|| format!("Failed to find commits since divergence from '{base}'"))?;

    if commits.is_empty() {
        println!(
            "No commits found on this branch since diverging from '{base}'."
        );
        return Ok(());
    }

    // Connect to DB
    let data_dir = ivc_dir.join("data");
    let db = db::connection::connect_embedded(&data_dir).await?;

    // Get or backfill commit captures
    let mut captures: Vec<CommitCapture> = Vec::new();
    for &oid in &commits {
        let sha = oid.to_string();
        match db::commit_capture::get_by_sha(&db, &sha).await? {
            Some(capture) => captures.push(capture),
            None => {
                // Backfill: commit was made with raw git commit
                let commit = repo.find_commit(oid)?;
                let message = commit.message().unwrap_or("").to_string();
                let (files, stats) = git::diff::get_commit_diff_stats(&repo, oid)?;

                let ticket_ref = {
                    let re = regex::Regex::new(r"[A-Z]+-\d+").ok();
                    re.and_then(|r| r.find(&message).map(|m| m.as_str().to_string()))
                };

                let capture = CommitCapture {
                    id: None,
                    commit_sha: sha,
                    message,
                    branch: branch.clone(),
                    repo: repo_name.clone(),
                    files_changed: files,
                    diff_stats: DiffStats::new(stats.additions, stats.deletions, stats.files_modified),
                    ticket_ref,
                    processed: false,
                    created_at: None,
                };

                db::commit_capture::create(&db, &capture).await?;
                captures.push(capture);
            }
        }
    }

    // Get combined diff
    let diff = git::diff::get_combined_diff(&repo, &commits)?;

    // Find ticket reference from any commit
    let ticket_ref = captures.iter().find_map(|c| c.ticket_ref.as_deref());

    // Build prompt
    let prompt = ai::extraction::build_extraction_prompt(&captures, &diff, ticket_ref);

    println!(
        "Extracting intentions from {} commits...",
        captures.len()
    );

    // Call Claude API
    let client = ai::client::ClaudeClient::new(&cfg.ai.model)?;
    let response = client.extract_intentions(&prompt).await?;

    // Parse response
    let extraction = ai::extraction::parse_extraction_response(&response)?;

    // Delete existing intentions for this branch (idempotent re-extraction)
    db::intention::delete_for_branch(&db, &repo_name, &branch).await?;

    // Store intention tree
    let (root, children) = ai::extraction::to_intentions(&extraction, &branch, &repo_name);
    let root_id = db::intention::create(&db, &root).await?;

    let mut child_ids = Vec::new();
    for (i, (child_intention, depends_on_idx)) in children.iter().enumerate() {
        let child_id = db::intention::create(&db, child_intention).await?;
        db::intention::create_decomposition(&db, &root_id, &child_id, i as i32).await?;

        if let Some(dep_idx) = depends_on_idx {
            if let Some(dep_id) = child_ids.get(*dep_idx) {
                db::intention::create_dependency(&db, &child_id, dep_id, None).await?;
            }
        }

        child_ids.push(child_id);
    }

    // Link intentions to commit captures
    for capture in &captures {
        if let Some(capture_id) = &capture.id {
            db::intention::create_derived_from(&db, &root_id, capture_id).await?;
        }
        db::commit_capture::mark_processed(&db, &capture.commit_sha).await?;
    }

    // Record extraction event
    let event = Event {
        id: None,
        event_type: EventType::IntentionsExtracted,
        source: "CLI".to_string(),
        intention: Some(root_id.clone()),
        payload: serde_json::json!({
            "branch": branch,
            "commit_count": captures.len(),
        }),
        created_at: None,
    };
    db::event::record(&db, &event).await?;

    // Display the tree
    let tree = db::intention::get_tree_for_branch(&db, &repo_name, &branch)
        .await?
        .context("Failed to retrieve intention tree after storing it")?;

    display_intention_tree(&tree, &branch, captures.len(), ticket_ref);

    // ── Phase 2: .ivc.json + GitHub PR ──────────────────────────────

    // Generate and commit .ivc.json
    let workdir = repo
        .workdir()
        .context("Could not determine repository working directory")?;
    let ivc_json_content = ivc_json::generate_ivc_json(&tree, &branch, &repo_name);
    ivc_json::commit_ivc_json(workdir, &ivc_json_content)?;
    println!("\nCommitted .ivc.json to branch.");

    if args.no_pr {
        println!("Stored in .ivc/data. Run `ivc log` to view again.");
        return Ok(());
    }

    // Push branch if not --no-push
    if !args.no_push {
        println!("Pushing branch to remote...");
        git::commit::run_git_command("push", &[
            "-u".to_string(),
            "origin".to_string(),
            branch.clone(),
        ])?;
    }

    // Create GitHub PR if token is available
    if github::client::GithubClient::is_available() {
        let (owner, gh_repo) = resolve_github_owner_repo(&repo, &cfg)?;
        let description = github::pr_description::format_pr_description(
            &tree,
            &branch,
            captures.len(),
            ticket_ref,
        );

        println!("Creating GitHub PR...");
        let gh = github::client::GithubClient::new(&owner, &gh_repo)?;
        let pr_url = gh
            .create_pr(&tree.root.title, &description, &branch, &base, args.draft)
            .await?;

        // Record PrCreated event
        let pr_event = Event {
            id: None,
            event_type: EventType::PrCreated,
            source: "CLI".to_string(),
            intention: Some(root_id),
            payload: serde_json::json!({
                "pr_url": pr_url,
                "branch": branch,
                "draft": args.draft,
            }),
            created_at: None,
        };
        db::event::record(&db, &pr_event).await?;

        println!("PR created: {}", pr_url);
    } else {
        println!(
            "\nGITHUB_TOKEN not set. Skipping PR creation.\n\
             Set GITHUB_TOKEN to create a GitHub PR automatically."
        );
    }

    println!("\nStored in .ivc/data. Run `ivc log` to view again.");

    Ok(())
}

fn resolve_github_owner_repo(
    repo: &git2::Repository,
    cfg: &config::IvcConfig,
) -> Result<(String, String)> {
    let owner = cfg.github.owner.clone();
    let gh_repo = cfg.github.repo.clone();

    match (owner, gh_repo) {
        (Some(o), Some(r)) => Ok((o, r)),
        _ => git::repo::get_remote_owner_repo(repo),
    }
}

fn display_intention_tree(
    tree: &IntentionTree,
    branch: &str,
    commit_count: usize,
    ticket_ref: Option<&str>,
) {
    println!();
    println!(
        "Intention tree for {} ({} commits)",
        branch, commit_count
    );

    if let Some(ticket) = ticket_ref {
        println!(
            "Ticket: {} (not fetched, ticket integration not configured)",
            ticket
        );
    }

    println!();
    display_intention(&tree.root, "", false);

    for (i, node) in tree.children.iter().enumerate() {
        let is_last = i == tree.children.len() - 1;
        let prefix = if is_last { "└── " } else { "├── " };
        let continuation = if is_last { "    " } else { "│   " };

        println!(
            "{}Intention {}: {}",
            prefix,
            i + 1,
            node.intention.title
        );
        println!("{}Type: {}", continuation, node.intention.intention_type);
        println!(
            "{}Files: {}",
            continuation,
            node.intention.files_changed.join(", ")
        );
        println!("{}Reasoning: {}", continuation, node.intention.reasoning);

        if !node.depends_on.is_empty() {
            println!(
                "{}Depends on: {}",
                continuation,
                node.depends_on.join(", ")
            );
        }

        if !node.intention.uncertainties.is_empty() {
            println!("{}Uncertainties:", continuation);
            for u in &node.intention.uncertainties {
                println!("{}  - {}", continuation, u);
            }
        }

        if !node.intention.assumptions.is_empty() {
            println!("{}Assumptions:", continuation);
            for a in &node.intention.assumptions {
                println!("{}  - {}", continuation, a);
            }
        }

        if !node.intention.alternatives_considered.is_empty() {
            println!("{}Alternatives considered:", continuation);
            for alt in &node.intention.alternatives_considered {
                println!(
                    "{}  - {} (rejected: {})",
                    continuation, alt.approach, alt.rejected_because
                );
            }
        }

        if !is_last {
            println!("│");
        }
    }
}

fn display_intention(intention: &Intention, _prefix: &str, _is_root: bool) {
    println!("Root: {}", intention.title);
    println!("Type: {}", intention.intention_type);
    println!("Reasoning: {}", intention.reasoning);
    println!();
}
