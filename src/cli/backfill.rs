use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use git2::{Oid, Repository};
use regex::Regex;

use crate::ai;
use crate::config;
use crate::db;
use crate::git;
use crate::models::commit_capture::{CommitCapture, DiffStats};
use crate::models::event::{Event, EventType};
use crate::models::intention::{BackfillMetadata, SourceType};

pub struct BackfillArgs {
    pub pr: Option<u32>,
    pub file: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: usize,
    pub dry_run: bool,
    pub skip_existing: bool,
}

/// Information about a merge commit representing a PR.
#[allow(dead_code)]
struct MergeInfo {
    merge_oid: Oid,
    merge_message: String,
    merge_date: DateTime<Utc>,
    pr_number: Option<u32>,
    is_squash: bool,
    commits: Vec<Oid>,
}

/// Pre-computed stats for a merge, used for dry-run display and processing.
#[allow(dead_code)]
struct MergeStats {
    info: MergeInfo,
    captures: Vec<CommitCapture>,
    total_files: usize,
    total_lines: u32,
    estimated_tokens: u32,
    already_exists: bool,
}

pub async fn run(args: BackfillArgs) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let repo = git::repo::open_repo(&cwd)?;
    git::repo::require_ivc_initialised(&repo)?;

    let repo_name = git::repo::get_repo_name(&repo)?;
    let ivc_dir = git::repo::get_ivc_dir(&repo)?;
    let data_dir = ivc_dir.join("data");
    let db = db::connection::connect_embedded(&data_dir).await?;

    // Determine mode and find merge commits
    let merges = if let Some(pr_number) = args.pr {
        vec![find_merge_commit_for_pr(&repo, pr_number)?]
    } else if let Some(ref file_path) = args.file {
        find_merges_touching_file(&repo, file_path)?
    } else if args.since.is_some() || args.until.is_some() {
        let since = parse_date_opt(&args.since, "since")?;
        let until = parse_date_opt(&args.until, "until")?;
        find_merges_in_date_range(&repo, since, until)?
    } else {
        anyhow::bail!(
            "Specify one of: --pr <number>, --file <path>, or --since <date>\n\
             Examples:\n  ivc backfill --pr 38\n  ivc backfill --since 2025-01-01\n  \
             ivc backfill --file src/main.rs"
        );
    };

    if merges.is_empty() {
        println!("No merge commits found matching the criteria.");
        return Ok(());
    }

    // Apply limit
    let merges: Vec<MergeInfo> = merges.into_iter().take(args.limit).collect();

    // Build stats for each merge (needed for dry-run and processing)
    let mut all_stats: Vec<MergeStats> = Vec::new();
    for info in merges {
        let commit_shas: Vec<String> = info.commits.iter().map(|o| o.to_string()).collect();
        let already_exists = if args.skip_existing {
            db::intention::has_intentions_for_commits(&db, &repo_name, &commit_shas).await?
        } else {
            false
        };

        let (captures, total_files, total_lines) = build_captures_for_merge(&repo, &info, &repo_name)?;
        let estimated_tokens = estimate_tokens(total_lines);

        all_stats.push(MergeStats {
            info,
            captures,
            total_files,
            total_lines,
            estimated_tokens,
            already_exists,
        });
    }

    let to_process: Vec<&MergeStats> = all_stats.iter().filter(|s| !s.already_exists).collect();
    let skipped_count = all_stats.len() - to_process.len();

    // Dry run: display table and exit
    if args.dry_run {
        display_dry_run(&all_stats, &args);
        return Ok(());
    }

    // Single PR mode — simpler output
    if args.pr.is_some() {
        if let Some(stats) = all_stats.into_iter().next() {
            if stats.already_exists {
                println!(
                    "PR #{} already has intentions. Skipping.",
                    stats.info.pr_number.map_or("?".to_string(), |n| n.to_string())
                );
                return Ok(());
            }
            process_single_merge(&db, &repo, stats, &repo_name, &ivc_dir).await?;
        }
        return Ok(());
    }

    // Batch mode — process sequentially with progress
    if to_process.is_empty() {
        println!("All {} merge commits already have intentions. Nothing to do.", all_stats.len());
        return Ok(());
    }

    let total_to_process = to_process.len();
    let mode_desc = if args.file.is_some() {
        format!("touching {}", args.file.as_ref().unwrap())
    } else {
        let since_str = args.since.as_deref().unwrap_or("beginning");
        format!("since {since_str}")
    };

    println!(
        "Backfilling {} PRs {}...\n",
        total_to_process, mode_desc
    );

    let cfg = config::load_config(&ivc_dir)?;
    let client = ai::client::ClaudeClient::new(&cfg.ai.model)?;
    let mut total_tokens_used: u32 = 0;
    let mut processed_count: usize = 0;

    // Consume all_stats, process non-skipped ones
    for stats in all_stats {
        if stats.already_exists {
            continue;
        }
        processed_count += 1;
        let label = merge_label(&stats.info);
        let commit_count = stats.info.commits.len();
        let estimated = stats.estimated_tokens;

        println!(
            "[{}/{}] {} \"{}\" -- {} commits",
            processed_count,
            total_to_process,
            stats.info.merge_date.format("%Y-%m-%d"),
            label,
            commit_count
        );

        match process_merge_with_client(&db, &repo, stats, &repo_name, &client).await {
            Ok(tree) => {
                // Display inline tree
                for (i, node) in tree.children.iter().enumerate() {
                    let is_last = i == tree.children.len() - 1;
                    let prefix = if is_last { "         └── " } else { "         ├── " };
                    println!("{}Intention: {}", prefix, node.intention.title);
                }
                println!("         Stored. (~{} tokens)\n", estimated);
                total_tokens_used += estimated;
            }
            Err(e) => {
                println!("         Error: {e}\n");
            }
        }
    }

    println!(
        "Done. {} PRs backfilled, {} skipped, ~{} tokens used.",
        processed_count, skipped_count, total_tokens_used
    );

    Ok(())
}

// ── Discovery functions ──────────────────────────────────────────────

/// Find the merge commit for a specific PR number.
fn find_merge_commit_for_pr(repo: &Repository, pr_number: u32) -> Result<MergeInfo> {
    let pr_pattern = Regex::new(&format!(r"#{}(?:\b|[)\s]|$)", pr_number))
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

        return Ok(build_merge_info(repo, oid, &commit, Some(pr_number))?);
    }

    anyhow::bail!(
        "Could not find merge commit for PR #{pr_number}. \
         The PR may use rebase/fast-forward merging which requires \
         GitHub API integration (Phase 2)."
    );
}

/// Find all merge commits in a date range.
fn find_merges_in_date_range(
    repo: &Repository,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
) -> Result<Vec<MergeInfo>> {
    let mut revwalk = repo.revwalk()?;
    revwalk.push_head()?;
    revwalk.set_sorting(git2::Sort::TIME)?;

    let pr_re = Regex::new(r"#(\d+)").ok();
    let mut merges = Vec::new();

    for oid_result in revwalk {
        let oid = oid_result?;
        let commit = repo.find_commit(oid)?;
        let commit_date = git2_time_to_chrono(commit.time());

        // Date filtering
        if let Some(ref s) = since {
            if commit_date < *s {
                // Commits are in reverse chronological order — once we pass
                // the since date, all remaining commits are older. Stop walking.
                break;
            }
        }
        if let Some(ref u) = until {
            if commit_date > *u {
                continue;
            }
        }

        let parent_count = commit.parent_count();
        let message = commit.message().unwrap_or("");

        // Include merge commits (2+ parents) or squash merges with PR number
        let is_merge = parent_count >= 2;
        let has_pr_ref = pr_re.as_ref().map_or(false, |re| re.is_match(message));

        if !is_merge && !has_pr_ref {
            continue;
        }
        // For single-parent commits, only include if they reference a PR (squash merge)
        if parent_count == 1 && !has_pr_ref {
            continue;
        }

        let pr_number = pr_re
            .as_ref()
            .and_then(|re| re.captures(message))
            .and_then(|caps| caps.get(1))
            .and_then(|m| m.as_str().parse::<u32>().ok());

        match build_merge_info(repo, oid, &commit, pr_number) {
            Ok(info) => merges.push(info),
            Err(e) => {
                tracing::warn!("Skipping merge commit {}: {}", &oid.to_string()[..8], e);
            }
        }
    }

    // Reverse to chronological order (oldest first)
    merges.reverse();
    Ok(merges)
}

/// Find all merge commits that touched a specific file.
fn find_merges_touching_file(repo: &Repository, file_path: &str) -> Result<Vec<MergeInfo>> {
    let mut revwalk = repo.revwalk()?;
    revwalk.push_head()?;
    revwalk.set_sorting(git2::Sort::TIME)?;

    let pr_re = Regex::new(r"#(\d+)").ok();
    let mut seen_merges = std::collections::HashSet::new();
    let mut merges = Vec::new();

    for oid_result in revwalk {
        let oid = oid_result?;
        let commit = repo.find_commit(oid)?;
        let parent_count = commit.parent_count();

        // Only process merge commits
        let is_merge = parent_count >= 2;
        let message = commit.message().unwrap_or("");
        let has_pr_ref = pr_re.as_ref().map_or(false, |re| re.is_match(message));

        if !is_merge && !(parent_count == 1 && has_pr_ref) {
            continue;
        }

        if seen_merges.contains(&oid) {
            continue;
        }

        // Check if this merge touched the file
        let touches_file = if parent_count >= 2 {
            let parent_tree = commit.parent(0)?.tree()?;
            let merge_tree = commit.tree()?;
            let diff = repo.diff_tree_to_tree(Some(&parent_tree), Some(&merge_tree), None)?;
            diff_touches_file(&diff, file_path)
        } else {
            let parent_tree = commit.parent(0)?.tree()?;
            let commit_tree = commit.tree()?;
            let diff = repo.diff_tree_to_tree(Some(&parent_tree), Some(&commit_tree), None)?;
            diff_touches_file(&diff, file_path)
        };

        if !touches_file {
            continue;
        }

        seen_merges.insert(oid);

        let pr_number = pr_re
            .as_ref()
            .and_then(|re| re.captures(message))
            .and_then(|caps| caps.get(1))
            .and_then(|m| m.as_str().parse::<u32>().ok());

        match build_merge_info(repo, oid, &commit, pr_number) {
            Ok(info) => merges.push(info),
            Err(e) => {
                tracing::warn!("Skipping merge commit {}: {}", &oid.to_string()[..8], e);
            }
        }
    }

    // Reverse to chronological order (oldest first)
    merges.reverse();
    Ok(merges)
}

// ── Shared helpers ───────────────────────────────────────────────────

/// Build MergeInfo from a commit, extracting its child commits.
fn build_merge_info(
    repo: &Repository,
    oid: Oid,
    commit: &git2::Commit,
    pr_number: Option<u32>,
) -> Result<MergeInfo> {
    let merge_date = git2_time_to_chrono(commit.time());
    let message = commit.message().unwrap_or("").to_string();
    let parent_count = commit.parent_count();

    if parent_count == 1 {
        // Squash merge
        return Ok(MergeInfo {
            merge_oid: oid,
            merge_message: message,
            merge_date,
            pr_number,
            is_squash: true,
            commits: vec![oid],
        });
    }

    if parent_count >= 2 {
        let base_parent = commit.parent_id(0)?;
        let branch_tip = commit.parent_id(1)?;

        let merge_base = repo
            .merge_base(base_parent, branch_tip)
            .context("Could not find merge base for merge commit")?;

        let mut walk = repo.revwalk()?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;
        walk.push(branch_tip)?;
        walk.hide(merge_base)?;

        let mut commits: Vec<Oid> = Vec::new();
        for c in walk {
            commits.push(c?);
        }
        commits.reverse();

        if commits.is_empty() {
            commits.push(oid);
        }

        return Ok(MergeInfo {
            merge_oid: oid,
            merge_message: message,
            merge_date,
            pr_number,
            is_squash: false,
            commits,
        });
    }

    anyhow::bail!("Commit {} has no parents", oid);
}

/// Check if a diff touches a specific file path.
fn diff_touches_file(diff: &git2::Diff, file_path: &str) -> bool {
    for i in 0..diff.deltas().len() {
        if let Some(delta) = diff.get_delta(i) {
            if let Some(path) = delta.new_file().path() {
                if path.to_string_lossy() == file_path {
                    return true;
                }
            }
            if let Some(path) = delta.old_file().path() {
                if path.to_string_lossy() == file_path {
                    return true;
                }
            }
        }
    }
    false
}

/// Build commit captures and stats for a merge.
fn build_captures_for_merge(
    repo: &Repository,
    info: &MergeInfo,
    repo_name: &str,
) -> Result<(Vec<CommitCapture>, usize, u32)> {
    let ticket_re = Regex::new(r"[A-Z]+-\d+").ok();
    let mut captures = Vec::new();
    let mut total_additions: u32 = 0;
    let mut total_deletions: u32 = 0;

    let branch_label = merge_branch_label(info);

    for &oid in &info.commits {
        let commit = repo.find_commit(oid)?;
        let message = commit.message().unwrap_or("").to_string();
        let (files, stats) = git::diff::get_commit_diff_stats(repo, oid)?;

        total_additions += stats.additions;
        total_deletions += stats.deletions;

        let ticket_ref = ticket_re
            .as_ref()
            .and_then(|r| r.find(&message).map(|m| m.as_str().to_string()));

        captures.push(CommitCapture {
            id: None,
            commit_sha: oid.to_string(),
            message,
            branch: branch_label.clone(),
            repo: repo_name.to_string(),
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

    Ok((captures, total_files, total_lines))
}

/// Generate a branch label for a merge.
fn merge_branch_label(info: &MergeInfo) -> String {
    if let Some(pr) = info.pr_number {
        format!("PR#{pr}")
    } else {
        format!("merge-{}", &info.merge_oid.to_string()[..8])
    }
}

/// Generate a human-readable label for a merge.
fn merge_label(info: &MergeInfo) -> String {
    let first_line = info
        .merge_message
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();

    if first_line.len() > 60 {
        format!("{}...", &first_line[..57])
    } else {
        first_line
    }
}

/// Process a single merge — used for `--pr` mode.
async fn process_single_merge(
    db_conn: &surrealdb::Surreal<surrealdb::engine::local::Db>,
    repo: &Repository,
    stats: MergeStats,
    repo_name: &str,
    ivc_dir: &std::path::Path,
) -> Result<()> {
    let cfg = config::load_config(ivc_dir)?;
    let client = ai::client::ClaudeClient::new(&cfg.ai.model)?;

    let pr_label = stats.info.pr_number.map_or_else(
        || merge_label(&stats.info),
        |n| format!("PR #{n}"),
    );
    println!(
        "Extracting intentions from {} ({} commits)...",
        pr_label,
        stats.info.commits.len()
    );

    let tree = process_merge_with_client(db_conn, repo, stats, repo_name, &client).await?;

    // Display tree
    println!();
    println!(
        "Backfilled {} (merged {})",
        pr_label,
        tree.root.created_at.map_or("unknown".to_string(), |d| d.format("%Y-%m-%d").to_string())
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
            "{}Type: {} | Source: BACKFILLED (confidence: 0.35)",
            continuation, node.intention.intention_type
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

        if !is_last {
            println!("│");
        }
    }

    println!("\nStored in SurrealDB. Run `ivc log` to view.");
    Ok(())
}

/// Process a merge: call LLM, store intentions, return the tree.
async fn process_merge_with_client(
    db_conn: &surrealdb::Surreal<surrealdb::engine::local::Db>,
    repo: &Repository,
    stats: MergeStats,
    repo_name: &str,
    client: &ai::client::ClaudeClient,
) -> Result<crate::models::intention::IntentionTree> {
    let MergeStats {
        info,
        captures,
        estimated_tokens,
        ..
    } = stats;

    // Get combined diff
    let diff = git::diff::get_combined_diff(repo, &info.commits)?;

    // Build prompt and call LLM
    let ticket_ref = captures.iter().find_map(|c| c.ticket_ref.as_deref());
    let prompt = ai::extraction::build_extraction_prompt(&captures, &diff, ticket_ref);
    let response = client.extract_intentions(&prompt).await?;
    let extraction = ai::extraction::parse_extraction_response(&response)?;

    // Store commit captures
    for capture in &captures {
        if db::commit_capture::get_by_sha(db_conn, &capture.commit_sha)
            .await?
            .is_none()
        {
            db::commit_capture::create(db_conn, capture).await?;
        }
    }

    // Build and store intentions
    let branch_label = merge_branch_label(&info);
    let backfill_meta = BackfillMetadata {
        backfilled_at: Utc::now(),
        merge_commit: info.merge_oid.to_string(),
        merge_date: info.merge_date,
        pr_number: info.pr_number.unwrap_or(0),
    };

    let (mut root, children) =
        ai::extraction::to_intentions(&extraction, &branch_label, repo_name);
    root.source_type = SourceType::Backfilled;
    root.source_confidence = 0.35;
    root.backfill_metadata = Some(backfill_meta.clone());
    root.created_at = Some(info.merge_date);

    let root_id = db::intention::create(db_conn, &root).await?;

    let mut child_ids = Vec::new();
    for (i, (mut child_intention, depends_on_idx)) in children.into_iter().enumerate() {
        child_intention.source_type = SourceType::Backfilled;
        child_intention.source_confidence = 0.35;
        child_intention.backfill_metadata = Some(backfill_meta.clone());
        child_intention.created_at = Some(info.merge_date);

        let child_id = db::intention::create(db_conn, &child_intention).await?;
        db::intention::create_decomposition(db_conn, &root_id, &child_id, i as i32).await?;

        if let Some(dep_idx) = depends_on_idx {
            if let Some(dep_id) = child_ids.get(dep_idx) {
                db::intention::create_dependency(db_conn, &child_id, dep_id, None).await?;
            }
        }
        child_ids.push(child_id);
    }

    // Link to commit captures
    for capture in &captures {
        if let Some(existing) =
            db::commit_capture::get_by_sha(db_conn, &capture.commit_sha).await?
        {
            if let Some(capture_id) = &existing.id {
                db::intention::create_derived_from(db_conn, &root_id, capture_id).await?;
            }
        }
        db::commit_capture::mark_processed(db_conn, &capture.commit_sha).await?;
    }

    // Record event
    let event = Event {
        id: None,
        event_type: EventType::IntentionsBackfilled,
        source: "CLI".to_string(),
        intention: Some(root_id),
        payload: serde_json::json!({
            "pr_number": info.pr_number,
            "merge_sha": info.merge_oid.to_string(),
            "commits_processed": captures.len(),
            "estimated_tokens": estimated_tokens,
        }),
        created_at: None,
    };
    db::event::record(db_conn, &event).await?;

    // Retrieve stored tree
    db::intention::get_tree_for_branch(db_conn, repo_name, &branch_label)
        .await?
        .context("Failed to retrieve intention tree after storing it")
}

// ── Display ──────────────────────────────────────────────────────────

fn display_dry_run(all_stats: &[MergeStats], args: &BackfillArgs) {
    let mode_desc = if let Some(pr) = args.pr {
        format!("PR #{pr}")
    } else if let Some(ref file) = args.file {
        format!("PRs touching {file}")
    } else {
        let since_str = args.since.as_deref().unwrap_or("beginning");
        let until_str = args.until.as_deref().unwrap_or("now");
        format!("PRs from {since_str} to {until_str}")
    };

    let total_found = all_stats.len();
    println!("Found {} merge commits for {} (showing up to {}, use --limit to change):\n",
        total_found, mode_desc, args.limit);

    println!(
        "  {:<4} {:<14} {:<8} {:<7} {:<14} {}",
        "#", "Merge Date", "Commits", "Files", "Est. Tokens", "Status"
    );

    for (i, stats) in all_stats.iter().enumerate() {
        let status = if stats.already_exists {
            "already exists (skip)"
        } else {
            "new"
        };
        let label = stats.info.pr_number.map_or_else(
            || format!("{}", i + 1),
            |n| format!("{n}"),
        );
        println!(
            "  {:<4} {:<14} {:<8} {:<7} ~{:<13} {}",
            label,
            stats.info.merge_date.format("%Y-%m-%d"),
            stats.info.commits.len(),
            stats.total_files,
            stats.estimated_tokens,
            status,
        );
    }

    let to_process = all_stats.iter().filter(|s| !s.already_exists).count();
    let skipped = all_stats.len() - to_process;
    let total_tokens: u32 = all_stats
        .iter()
        .filter(|s| !s.already_exists)
        .map(|s| s.estimated_tokens)
        .sum();

    println!();
    println!(
        "Would process: {} PRs ({} skipped)",
        to_process, skipped
    );
    println!("Estimated total: ~{} tokens", total_tokens);
    println!("\nRun without --dry-run to proceed.");
}

// ── Utilities ────────────────────────────────────────────────────────

fn parse_date_opt(s: &Option<String>, name: &str) -> Result<Option<DateTime<Utc>>> {
    match s {
        None => Ok(None),
        Some(date_str) => {
            let naive = NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                .with_context(|| format!("Invalid --{name} date '{date_str}'. Use ISO format: YYYY-MM-DD"))?;
            let dt = naive
                .and_hms_opt(0, 0, 0)
                .context("Invalid date")?;
            Ok(Some(Utc.from_utc_datetime(&dt)))
        }
    }
}

fn git2_time_to_chrono(time: git2::Time) -> DateTime<Utc> {
    Utc.timestamp_opt(time.seconds(), 0)
        .single()
        .unwrap_or_else(Utc::now)
}

fn estimate_tokens(total_lines_changed: u32) -> u32 {
    (total_lines_changed * 10).saturating_add(500)
}
