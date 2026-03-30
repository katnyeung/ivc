use anyhow::{Context, Result};
use git2::{Diff, DiffOptions, Oid, Repository};

use crate::models::commit_capture::DiffStats;

/// Get diff stats for a single commit against its parent.
pub fn get_commit_diff_stats(
    repo: &Repository,
    commit_oid: Oid,
) -> Result<(Vec<String>, DiffStats)> {
    let commit = repo
        .find_commit(commit_oid)
        .context("Failed to find commit")?;
    let commit_tree = commit.tree().context("Failed to get commit tree")?;

    let parent_tree = if commit.parent_count() > 0 {
        Some(
            commit
                .parent(0)?
                .tree()
                .context("Failed to get parent tree")?,
        )
    } else {
        None // Initial commit, diff against empty tree
    };

    let diff = repo
        .diff_tree_to_tree(parent_tree.as_ref(), Some(&commit_tree), None)
        .context("Failed to compute diff")?;

    extract_diff_stats(&diff)
}

/// Get the combined text diff across all commits (first commit's parent to last commit).
pub fn get_combined_diff(repo: &Repository, commits: &[Oid]) -> Result<String> {
    if commits.is_empty() {
        return Ok(String::new());
    }

    let first_commit = repo.find_commit(commits[0])?;
    let last_commit = repo.find_commit(*commits.last().unwrap())?;

    let start_tree = if first_commit.parent_count() > 0 {
        Some(first_commit.parent(0)?.tree()?)
    } else {
        None
    };

    let end_tree = last_commit.tree()?;

    let mut opts = DiffOptions::new();
    let diff = repo.diff_tree_to_tree(start_tree.as_ref(), Some(&end_tree), Some(&mut opts))?;

    let mut diff_text = String::new();
    diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        let origin = line.origin();
        if origin == '+' || origin == '-' || origin == ' ' {
            diff_text.push(origin);
        }
        if let Ok(content) = std::str::from_utf8(line.content()) {
            diff_text.push_str(content);
        }
        true
    })?;

    Ok(diff_text)
}

/// Get per-commit diffs as (sha, diff_text) pairs.
pub fn get_per_commit_diffs(repo: &Repository, commits: &[Oid]) -> Result<Vec<(String, String)>> {
    let mut results = Vec::new();

    for &oid in commits {
        let commit = repo.find_commit(oid)?;
        let commit_tree = commit.tree()?;

        let parent_tree = if commit.parent_count() > 0 {
            Some(commit.parent(0)?.tree()?)
        } else {
            None
        };

        let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&commit_tree), None)?;

        let mut diff_text = String::new();
        diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
            let origin = line.origin();
            if origin == '+' || origin == '-' || origin == ' ' {
                diff_text.push(origin);
            }
            if let Ok(content) = std::str::from_utf8(line.content()) {
                diff_text.push_str(content);
            }
            true
        })?;

        results.push((oid.to_string(), diff_text));
    }

    Ok(results)
}

fn extract_diff_stats(diff: &Diff) -> Result<(Vec<String>, DiffStats)> {
    let stats = diff.stats().context("Failed to get diff stats")?;
    let mut files = Vec::new();

    for i in 0..diff.deltas().len() {
        if let Some(delta) = diff.get_delta(i) {
            if let Some(path) = delta.new_file().path() {
                files.push(path.to_string_lossy().to_string());
            } else if let Some(path) = delta.old_file().path() {
                files.push(path.to_string_lossy().to_string());
            }
        }
    }

    let diff_stats = DiffStats::new(
        stats.insertions() as u32,
        stats.deletions() as u32,
        stats.files_changed() as u32,
    );

    Ok((files, diff_stats))
}
