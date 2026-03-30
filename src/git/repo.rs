use anyhow::{Context, Result};
use git2::Repository;
use std::path::Path;

use crate::errors::IvcError;

pub fn open_repo(path: &Path) -> Result<Repository> {
    Repository::discover(path).map_err(|_| IvcError::NotAGitRepo.into())
}

pub fn get_repo_name(repo: &Repository) -> Result<String> {
    // Try to derive from remote "origin" URL
    if let Ok(remote) = repo.find_remote("origin") {
        if let Some(url) = remote.url() {
            // Extract repo name from URL like "git@github.com:user/repo.git" or "https://github.com/user/repo.git"
            let name = url
                .rsplit('/')
                .next()
                .unwrap_or(url)
                .trim_end_matches(".git");
            return Ok(name.to_string());
        }
    }

    // Fallback to directory name
    let workdir = repo
        .workdir()
        .context("Could not determine repository working directory")?;
    let name = workdir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    Ok(name.to_string())
}

pub fn get_current_branch(repo: &Repository) -> Result<String> {
    let head = repo.head().context("Failed to read HEAD reference")?;
    let name = head
        .shorthand()
        .context("HEAD is not a named branch")?
        .to_string();
    Ok(name)
}

pub fn is_ivc_initialised(repo: &Repository) -> bool {
    if let Some(workdir) = repo.workdir() {
        workdir.join(".ivc").exists()
    } else {
        false
    }
}

pub fn require_ivc_initialised(repo: &Repository) -> Result<()> {
    if !is_ivc_initialised(repo) {
        anyhow::bail!(IvcError::NotInitialised);
    }
    Ok(())
}

pub fn get_ivc_dir(repo: &Repository) -> Result<std::path::PathBuf> {
    let workdir = repo
        .workdir()
        .context("Could not determine repository working directory")?;
    Ok(workdir.join(".ivc"))
}
