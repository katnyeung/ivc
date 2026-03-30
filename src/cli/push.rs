use anyhow::{Context, Result};

use crate::db;
use crate::git;
use crate::models::event::{Event, EventType};

pub async fn run(args: Vec<String>) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let repo = git::repo::open_repo(&cwd)?;
    git::repo::require_ivc_initialised(&repo)?;

    // Run the real git push with all args passed through
    git::commit::run_git_push(&args)?;

    // After push succeeds, handle stale capture cleanup
    let branch = git::repo::get_current_branch(&repo)?;
    let repo_name = git::repo::get_repo_name(&repo)?;
    let ivc_dir = git::repo::get_ivc_dir(&repo)?;
    let data_dir = ivc_dir.join("data");
    let db = db::connection::connect_embedded(&data_dir).await?;

    // Check for stale captures (force push scenario)
    let captures = db::commit_capture::get_for_branch(&db, &repo_name, &branch).await?;
    if !captures.is_empty() {
        // Get current commits on the branch to compare
        let current_shas: std::collections::HashSet<String> = {
            // Try to get commits; if branch has no base, treat all as valid
            match git::branch::get_commits_since_divergence(&repo, "main")
                .or_else(|_| git::branch::get_commits_since_divergence(&repo, "master"))
            {
                Ok(oids) => oids.iter().map(|o| o.to_string()).collect(),
                Err(_) => {
                    // Cannot determine divergence; keep all captures
                    captures.iter().map(|c| c.commit_sha.clone()).collect()
                }
            }
        };

        let stale_shas: Vec<String> = captures
            .iter()
            .filter(|c| !current_shas.contains(&c.commit_sha))
            .map(|c| c.commit_sha.clone())
            .collect();

        if !stale_shas.is_empty() {
            db::commit_capture::delete_by_shas(&db, &stale_shas).await?;
            tracing::info!(
                "Removed {} stale commit captures after force push",
                stale_shas.len()
            );
        }
    }

    // Record event
    let event = Event {
        id: None,
        event_type: EventType::PushSynced,
        source: "CLI".to_string(),
        intention: None,
        payload: serde_json::json!({
            "branch": branch,
        }),
        created_at: None,
    };
    db::event::record(&db, &event).await?;

    let capture_count = db::commit_capture::get_for_branch(&db, &repo_name, &branch)
        .await?
        .len();
    println!("Push complete. {capture_count} commit captures synced.");

    Ok(())
}
