use anyhow::{Context, Result};

use crate::ai;
use crate::config;
use crate::db;
use crate::git;
use crate::models::commit_capture::{CommitCapture, DiffStats};
use crate::models::event::{Event, EventType};
use crate::models::intention::{Intention, IntentionTree};

pub async fn run(base: String) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let repo = git::repo::open_repo(&cwd)?;
    git::repo::require_ivc_initialised(&repo)?;

    // Resolve base branch: CLI arg > config > default "main"
    let ivc_dir = git::repo::get_ivc_dir(&repo)?;
    let base = if base.is_empty() {
        let cfg = config::load_config(&ivc_dir)?;
        cfg.git.default_base
    } else {
        base
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
    let ivc_dir = git::repo::get_ivc_dir(&repo)?;
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

    // Load config for AI model
    let cfg = config::load_config(&ivc_dir)?;

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

    // Record event
    let event = Event {
        id: None,
        event_type: EventType::IntentionsExtracted,
        source: "CLI".to_string(),
        intention: Some(root_id),
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

    println!("\nStored in .ivc/data. Run `ivc log` to view again.");

    Ok(())
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
