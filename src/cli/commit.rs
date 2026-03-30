use anyhow::{Context, Result};
use regex::Regex;

use crate::db;
use crate::git;
use crate::models::commit_capture::{CommitCapture, DiffStats};
use crate::models::event::{Event, EventType};

pub async fn run(args: Vec<String>) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let repo = git::repo::open_repo(&cwd)?;
    git::repo::require_ivc_initialised(&repo)?;

    // Run the real git commit with all args passed through
    git::commit::run_git_commit(&args)?;

    // After git commit succeeds, capture metadata
    // Re-open repo to pick up the new HEAD
    let repo = git::repo::open_repo(&cwd)?;
    let sha = git::commit::get_head_commit_sha(&repo)?;
    let message = git::commit::get_head_commit_message(&repo)?;
    let branch = git::repo::get_current_branch(&repo)?;
    let repo_name = git::repo::get_repo_name(&repo)?;

    let head_oid = repo
        .head()?
        .target()
        .context("HEAD has no target")?;
    let (files_changed, diff_stats) =
        git::diff::get_commit_diff_stats(&repo, head_oid)?;

    let ticket_ref = extract_ticket_ref(&message);

    // Connect to DB and store
    let ivc_dir = git::repo::get_ivc_dir(&repo)?;
    let data_dir = ivc_dir.join("data");
    let db = db::connection::connect_embedded(&data_dir).await?;

    let capture = CommitCapture {
        id: None,
        commit_sha: sha.clone(),
        message: message.clone(),
        branch: branch.clone(),
        repo: repo_name.clone(),
        files_changed: files_changed.clone(),
        diff_stats: DiffStats::new(
            diff_stats.additions,
            diff_stats.deletions,
            diff_stats.files_modified,
        ),
        ticket_ref: ticket_ref.clone(),
        processed: false,
        created_at: None,
    };

    db::commit_capture::create(&db, &capture).await?;

    // Record event
    let event = Event {
        id: None,
        event_type: EventType::CommitCaptured,
        source: "CLI".to_string(),
        intention: None,
        payload: serde_json::json!({
            "commit_sha": sha,
            "branch": branch,
        }),
        created_at: None,
    };
    db::event::record(&db, &event).await?;

    let short_sha = &sha[..8.min(sha.len())];
    println!(
        "Commit captured: {short_sha} ({} files, +{} -{})",
        files_changed.len(),
        diff_stats.additions,
        diff_stats.deletions,
    );

    Ok(())
}

fn extract_ticket_ref(message: &str) -> Option<String> {
    let re = Regex::new(r"[A-Z]+-\d+").ok()?;
    re.find(message).map(|m| m.as_str().to_string())
}
