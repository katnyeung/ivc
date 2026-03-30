use anyhow::{Context, Result};
use git2::{Oid, Repository};

/// Find the merge base (divergence point) between base branch and HEAD.
pub fn find_divergence_point(repo: &Repository, base: &str) -> Result<Oid> {
    let base_ref = repo
        .resolve_reference_from_short_name(base)
        .with_context(|| format!("Could not resolve base branch '{base}'"))?;
    let base_oid = base_ref
        .target()
        .context("Base branch reference has no target")?;

    let head = repo.head().context("Failed to read HEAD")?;
    let head_oid = head.target().context("HEAD has no target")?;

    let merge_base = repo
        .merge_base(base_oid, head_oid)
        .context("Could not find merge base between HEAD and base branch")?;

    Ok(merge_base)
}

/// Get all commits on the current branch since divergence from the base branch.
/// Returns commits in chronological order (oldest first).
pub fn get_commits_since_divergence(repo: &Repository, base: &str) -> Result<Vec<Oid>> {
    let merge_base = find_divergence_point(repo, base)?;

    let head = repo.head()?;
    let head_oid = head.target().context("HEAD has no target")?;

    // If HEAD is the merge base, there are no new commits
    if head_oid == merge_base {
        return Ok(Vec::new());
    }

    let mut revwalk = repo.revwalk()?;
    revwalk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;
    revwalk.push(head_oid)?;
    revwalk.hide(merge_base)?;

    let mut commits: Vec<Oid> = Vec::new();
    for oid in revwalk {
        commits.push(oid?);
    }

    // Reverse to get chronological order (oldest first)
    commits.reverse();
    Ok(commits)
}
