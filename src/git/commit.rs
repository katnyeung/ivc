use anyhow::{Context, Result};
use git2::Repository;
use std::process::Command;

/// Run any git subcommand with all provided arguments passed through.
pub fn run_git_command(subcommand: &str, args: &[String]) -> Result<()> {
    let status = Command::new("git")
        .arg(subcommand)
        .args(args)
        .status()
        .context(format!("Failed to execute git {}", subcommand))?;

    if !status.success() {
        anyhow::bail!(
            "git {} exited with status {}",
            subcommand,
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

/// Run `git commit` with all provided arguments passed through.
pub fn run_git_commit(args: &[String]) -> Result<()> {
    run_git_command("commit", args)
}

/// Run `git push` with all provided arguments passed through.
pub fn run_git_push(args: &[String]) -> Result<()> {
    run_git_command("push", args)
}

/// Get the SHA of the HEAD commit.
pub fn get_head_commit_sha(repo: &Repository) -> Result<String> {
    let head = repo.head().context("Failed to read HEAD")?;
    let oid = head.target().context("HEAD has no target")?;
    Ok(oid.to_string())
}

/// Get the message of the HEAD commit.
pub fn get_head_commit_message(repo: &Repository) -> Result<String> {
    let head = repo.head().context("Failed to read HEAD")?;
    let oid = head.target().context("HEAD has no target")?;
    let commit = repo.find_commit(oid).context("Failed to find HEAD commit")?;
    let message = commit.message().unwrap_or("").to_string();
    Ok(message)
}
