use anyhow::{Context, Result};
use chrono::{DateTime, TimeZone, Utc};
use git2::{Oid, Repository};
use regex::Regex;

use crate::ai;
use crate::config;
use crate::db;
use crate::git;
use crate::models::commit_capture::{CommitCapture, DiffStats};
use crate::models::event::{Event, EventType};
use crate::models::intention::{BackfillMetadata, IntentionTree, SourceType};

/// Information about a merge commit representing a PR.
struct MergeInfo {
    merge_oid: Oid,
    merge_message: String,
    merge_date: DateTime<Utc>,
    #[allow(dead_code)]
    pr_number: u32,
    is_squash: bool,
    commits: Vec<Oid>,
}

pub async fn run(pr_number: u32, dry_run: bool) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let repo = git::repo::open_repo(&cwd)?;
    git::repo::require_ivc_initialised(&repo)?;

    let repo_name = git::repo::get_repo_name(&repo)?;
    let ivc_dir = git::repo::get_ivc_dir(&repo)?;
    let data_dir = ivc_dir.join("data");

    // Step 1: Find the merge commit for this PR
    let merge_info = find_merge_commit_for_pr(&repo, pr_number)?;

    // Step 2: Check for existing intentions
    let db = db::connection::connect_embedded(&data_dir).await?;
    let commit_shas: Vec<String> = merge_info.commits.iter().map(|o| o.to_string()).collect();
    if db::intention::has_intentions_for_commits(&db, &repo_name, &commit_shas).await? {
        println!("PR #{pr_number} already has intentions. Skipping.");
        return Ok(());
    }

    // Step 3: Extract diffs and build captures
    let mut captures: Vec<CommitCapture> = Vec::new();
    let mut total_additions: u32 = 0;
    let mut total_deletions: u32 = 0;

    for &oid in &merge_info.commits {
        let commit = repo.find_commit(oid)?;
        let message = commit.message().unwrap_or("").to_string();
        let (files, stats) = git::diff::get_commit_diff_stats(&repo, oid)?;

        total_additions += stats.additions;
        total_deletions += stats.deletions;

        let ticket_ref = {
            let re = Regex::new(r"[A-Z]+-\d+").ok();
            re.and_then(|r| r.find(&message).map(|m| m.as_str().to_string()))
        };

        captures.push(CommitCapture {
            id: None,
            commit_sha: oid.to_string(),
            message,
            branch: format!("PR#{pr_number}"),
            repo: repo_name.clone(),
            files_changed: files,
            diff_stats: DiffStats::new(stats.additions, stats.deletions, stats.files_modified),
            ticket_ref,
            processed: false,
            created_at: None,
        });
    }

    let total_files: usize = captures
        .iter()
        .flat_map(|c| &c.files_changed)
        .collect::<std::collections::HashSet<_>>()
        .len();
    let total_lines = total_additions + total_deletions;
    let estimated_tokens = estimate_tokens(total_lines);

    // Step 4: Dry run — show info and exit
    if dry_run {
        println!("Would backfill PR #{pr_number}:");
        println!();
        let merge_type = if merge_info.is_squash {
            "squash merge"
        } else {
            "merge commit"
        };
        println!(
            "  PR #{pr_number}  ({}) \"{}\"",
            merge_info.merge_date.format("%Y-%m-%d"),
            merge_info.merge_message.lines().next().unwrap_or("").trim()
        );
        println!(
            "    {} commits | {} files | ~{} lines changed ({})",
            merge_info.commits.len(),
            total_files,
            total_lines,
            merge_type
        );
        println!("    Estimated tokens: ~{estimated_tokens}");
        println!();
        println!("Run without --dry-run to proceed.");
        return Ok(());
    }

    // Step 5: Get combined diff
    let diff = git::diff::get_combined_diff(&repo, &merge_info.commits)?;

    // Step 6: Build prompt and call LLM
    let ticket_ref = captures.iter().find_map(|c| c.ticket_ref.as_deref());
    let prompt = ai::extraction::build_extraction_prompt(&captures, &diff, ticket_ref);

    println!(
        "Extracting intentions from PR #{pr_number} ({} commits)...",
        captures.len()
    );

    let cfg = config::load_config(&ivc_dir)?;
    let client = ai::client::ClaudeClient::new(&cfg.ai.model)?;
    let response = client.extract_intentions(&prompt).await?;
    let extraction = ai::extraction::parse_extraction_response(&response)?;

    // Step 7: Store commit captures
    for capture in &captures {
        // Skip if already exists (may have been captured by ivc commit earlier)
        if db::commit_capture::get_by_sha(&db, &capture.commit_sha)
            .await?
            .is_none()
        {
            db::commit_capture::create(&db, capture).await?;
        }
    }

    // Step 8: Build and store intentions with backfill metadata
    let branch_label = format!("PR#{pr_number}");
    let backfill_meta = BackfillMetadata {
        backfilled_at: Utc::now(),
        merge_commit: merge_info.merge_oid.to_string(),
        merge_date: merge_info.merge_date,
        pr_number,
    };

    let (mut root, children) = ai::extraction::to_intentions(&extraction, &branch_label, &repo_name);
    root.source_type = SourceType::Backfilled;
    root.source_confidence = 0.35;
    root.backfill_metadata = Some(backfill_meta.clone());
    root.created_at = Some(merge_info.merge_date);

    let root_id = db::intention::create(&db, &root).await?;

    let mut child_ids = Vec::new();
    for (i, (mut child_intention, depends_on_idx)) in children.into_iter().enumerate() {
        child_intention.source_type = SourceType::Backfilled;
        child_intention.source_confidence = 0.35;
        child_intention.backfill_metadata = Some(backfill_meta.clone());
        child_intention.created_at = Some(merge_info.merge_date);

        let child_id = db::intention::create(&db, &child_intention).await?;
        db::intention::create_decomposition(&db, &root_id, &child_id, i as i32).await?;

        if let Some(dep_idx) = depends_on_idx {
            if let Some(dep_id) = child_ids.get(dep_idx) {
                db::intention::create_dependency(&db, &child_id, dep_id, None).await?;
            }
        }
        child_ids.push(child_id);
    }

    // Link intentions to commit captures
    for capture in &captures {
        if let Some(existing) = db::commit_capture::get_by_sha(&db, &capture.commit_sha).await? {
            if let Some(capture_id) = &existing.id {
                db::intention::create_derived_from(&db, &root_id, capture_id).await?;
            }
        }
        db::commit_capture::mark_processed(&db, &capture.commit_sha).await?;
    }

    // Record event
    let event = Event {
        id: None,
        event_type: EventType::IntentionsBackfilled,
        source: "CLI".to_string(),
        intention: Some(root_id),
        payload: serde_json::json!({
            "pr_number": pr_number,
            "commits_processed": captures.len(),
            "estimated_tokens": estimated_tokens,
        }),
        created_at: None,
    };
    db::event::record(&db, &event).await?;

    // Display the tree
    let tree = db::intention::get_tree_for_branch(&db, &repo_name, &branch_label)
        .await?
        .context("Failed to retrieve intention tree after storing it")?;

    display_backfill_tree(&tree, pr_number, &merge_info.merge_date);

    println!("\nStored in SurrealDB. Run `ivc log` to view.");

    Ok(())
}

/// Find the merge commit for a given PR number by searching git log.
fn find_merge_commit_for_pr(repo: &Repository, pr_number: u32) -> Result<MergeInfo> {
    let pr_pattern = Regex::new(&format!(r"#{}(?:\b|[)\s])", pr_number))
        .context("Failed to compile PR pattern regex")?;

    let mut revwalk = repo.revwalk()?;
    revwalk.push_head()?;
    revwalk.set_sorting(git2::Sort::TIME)?;

    for oid_result in revwalk {
        let oid = oid_result?;
        let commit = repo.find_commit(oid)?;
        let message = commit.message().unwrap_or("");

        if !pr_pattern.is_match(message) {
            continue;
        }

        let merge_date = git2_time_to_chrono(commit.time());
        let parent_count = commit.parent_count();

        if parent_count == 1 {
            // Squash merge: the commit itself is the entire PR
            return Ok(MergeInfo {
                merge_oid: oid,
                merge_message: message.to_string(),
                merge_date,
                pr_number,
                is_squash: true,
                commits: vec![oid],
            });
        }

        if parent_count >= 2 {
            // Regular merge: walk commits between merge base and branch tip
            let base_parent = commit.parent_id(0)?;
            let branch_tip = commit.parent_id(1)?;

            let merge_base = repo
                .merge_base(base_parent, branch_tip)
                .context("Could not find merge base for PR merge commit")?;

            let mut walk = repo.revwalk()?;
            walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;
            walk.push(branch_tip)?;
            walk.hide(merge_base)?;

            let mut commits: Vec<Oid> = Vec::new();
            for c in walk {
                commits.push(c?);
            }
            commits.reverse(); // chronological order

            if commits.is_empty() {
                // Edge case: merge base == branch tip, just use the merge commit
                commits.push(oid);
            }

            return Ok(MergeInfo {
                merge_oid: oid,
                merge_message: message.to_string(),
                merge_date,
                pr_number,
                is_squash: false,
                commits,
            });
        }
    }

    anyhow::bail!(
        "Could not find merge commit for PR #{pr_number}. \
         The PR may use rebase/fast-forward merging which requires \
         GitHub API integration (Phase 2)."
    );
}

/// Convert git2 time to chrono DateTime<Utc>.
fn git2_time_to_chrono(time: git2::Time) -> DateTime<Utc> {
    Utc.timestamp_opt(time.seconds(), 0)
        .single()
        .unwrap_or_else(Utc::now)
}

/// Rough token estimation: ~4 chars per token for code.
fn estimate_tokens(total_lines_changed: u32) -> u32 {
    // Average line ~40 chars, ~10 tokens per line
    // Plus commit messages and prompt overhead (~500 tokens)
    (total_lines_changed * 10).saturating_add(500)
}

fn display_backfill_tree(tree: &IntentionTree, pr_number: u32, merge_date: &DateTime<Utc>) {
    println!();
    println!(
        "Backfilled PR #{} (merged {})",
        pr_number,
        merge_date.format("%Y-%m-%d")
    );
    println!();

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
        println!(
            "{}Type: {} | Source: BACKFILLED (confidence: {:.2})",
            continuation, node.intention.intention_type, node.intention.source_confidence
        );
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

        if !is_last {
            println!("│");
        }
    }
}
