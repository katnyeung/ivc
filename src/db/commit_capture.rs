use anyhow::{Context, Result};
use surrealdb::engine::local::Db;
use surrealdb::sql::Thing;
use surrealdb::Surreal;

use crate::models::commit_capture::CommitCapture;

/// Insert a new commit capture record.
pub async fn create(db: &Surreal<Db>, capture: &CommitCapture) -> Result<Thing> {
    let result: Option<CommitCapture> = db
        .create("commit_capture")
        .content(capture.clone())
        .await
        .context("Failed to create commit capture")?;
    let record = result.context("No record returned after insert")?;
    record.id.context("Record has no ID")
}

/// Get a commit capture by its SHA.
pub async fn get_by_sha(db: &Surreal<Db>, sha: &str) -> Result<Option<CommitCapture>> {
    let mut result = db
        .query("SELECT * FROM commit_capture WHERE commit_sha = $sha LIMIT 1")
        .bind(("sha", sha.to_string()))
        .await
        .context("Failed to query commit capture by SHA")?;
    let captures: Vec<CommitCapture> = result.take(0)?;
    Ok(captures.into_iter().next())
}

/// Get all unprocessed commit captures for a specific branch.
pub async fn get_unprocessed_for_branch(
    db: &Surreal<Db>,
    repo: &str,
    branch: &str,
) -> Result<Vec<CommitCapture>> {
    let mut result = db
        .query(
            "SELECT * FROM commit_capture WHERE repo = $repo AND branch = $branch AND processed = false ORDER BY created_at ASC",
        )
        .bind(("repo", repo.to_string()))
        .bind(("branch", branch.to_string()))
        .await
        .context("Failed to query unprocessed commits")?;
    let captures: Vec<CommitCapture> = result.take(0)?;
    Ok(captures)
}

/// Get all commit captures for a specific branch.
pub async fn get_for_branch(
    db: &Surreal<Db>,
    repo: &str,
    branch: &str,
) -> Result<Vec<CommitCapture>> {
    let mut result = db
        .query(
            "SELECT * FROM commit_capture WHERE repo = $repo AND branch = $branch ORDER BY created_at ASC",
        )
        .bind(("repo", repo.to_string()))
        .bind(("branch", branch.to_string()))
        .await
        .context("Failed to query commits for branch")?;
    let captures: Vec<CommitCapture> = result.take(0)?;
    Ok(captures)
}

/// Mark a commit capture as processed.
pub async fn mark_processed(db: &Surreal<Db>, sha: &str) -> Result<()> {
    db.query("UPDATE commit_capture SET processed = true WHERE commit_sha = $sha")
        .bind(("sha", sha.to_string()))
        .await
        .context("Failed to mark commit as processed")?;
    Ok(())
}

/// Delete all commit captures for a branch (used for force-push stale replacement).
pub async fn delete_for_branch(db: &Surreal<Db>, repo: &str, branch: &str) -> Result<()> {
    db.query("DELETE FROM commit_capture WHERE repo = $repo AND branch = $branch")
        .bind(("repo", repo.to_string()))
        .bind(("branch", branch.to_string()))
        .await
        .context("Failed to delete stale commit captures")?;
    Ok(())
}

/// Delete commit captures by their SHAs.
pub async fn delete_by_shas(db: &Surreal<Db>, shas: &[String]) -> Result<()> {
    for sha in shas {
        db.query("DELETE FROM commit_capture WHERE commit_sha = $sha")
            .bind(("sha", sha.clone()))
            .await
            .context("Failed to delete stale commit capture")?;
    }
    Ok(())
}
