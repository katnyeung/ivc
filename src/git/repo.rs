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

/// Extract (owner, repo) from the git remote origin URL.
/// Handles both SSH and HTTPS formats.
pub fn get_remote_owner_repo(repo: &Repository) -> Result<(String, String)> {
    let remote = repo
        .find_remote("origin")
        .context("No 'origin' remote found")?;
    let url = remote.url().context("Origin remote has no URL")?;

    // SSH format: git@github.com:owner/repo.git
    if let Some(rest) = url.strip_prefix("git@") {
        if let Some((_host, path)) = rest.split_once(':') {
            let path = path.trim_end_matches(".git");
            if let Some((owner, repo_name)) = path.split_once('/') {
                return Ok((owner.to_string(), repo_name.to_string()));
            }
        }
    }

    // HTTPS format: https://github.com/owner/repo.git
    let parts: Vec<&str> = url.trim_end_matches(".git").rsplitn(3, '/').collect();
    if parts.len() >= 2 {
        return Ok((parts[1].to_string(), parts[0].to_string()));
    }

    anyhow::bail!("Could not parse owner/repo from remote URL: {url}")
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
