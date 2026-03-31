use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::git;
use crate::models::intention::IntentionTree;

/// Generate the .ivc.json content from an IntentionTree, with integrity hash.
pub fn generate_ivc_json(
    tree: &IntentionTree,
    branch: &str,
    repo: &str,
) -> serde_json::Value {
    let sub_intentions: Vec<serde_json::Value> = tree
        .children
        .iter()
        .map(|node| {
            let mut obj = serde_json::json!({
                "title": node.intention.title,
                "type": node.intention.intention_type.to_string(),
                "reasoning": node.intention.reasoning,
                "files_changed": node.intention.files_changed,
                "uncertainties": node.intention.uncertainties,
                "assumptions": node.intention.assumptions,
            });

            if !node.intention.alternatives_considered.is_empty() {
                obj["alternatives_considered"] = serde_json::json!(
                    node.intention.alternatives_considered
                        .iter()
                        .map(|a| serde_json::json!({
                            "approach": a.approach,
                            "rejected_because": a.rejected_because,
                        }))
                        .collect::<Vec<_>>()
                );
            }

            if !node.depends_on.is_empty() {
                obj["depends_on"] = serde_json::json!(node.depends_on);
            }

            obj
        })
        .collect();

    // Build JSON without integrity field first
    let mut json = serde_json::json!({
        "version": "1.0",
        "branch": branch,
        "repo": repo,
        "extracted_at": chrono::Utc::now().to_rfc3339(),
        "root": {
            "title": tree.root.title,
            "type": tree.root.intention_type.to_string(),
            "reasoning": tree.root.reasoning,
            "files_changed": tree.root.files_changed,
            "uncertainties": tree.root.uncertainties,
            "assumptions": tree.root.assumptions,
        },
        "sub_intentions": sub_intentions,
    });

    // Compute integrity hash over the content (without integrity field)
    let integrity = compute_integrity(&json);
    json["integrity"] = serde_json::Value::String(integrity);

    json
}

/// Write intention tree JSON to .ivc/trees/{branch}.json and commit.
pub fn commit_ivc_json(workdir: &Path, content: &serde_json::Value, branch: &str) -> Result<()> {
    let json_path = tree_file_path(workdir, branch);
    write_and_commit(workdir, &json_path, content, "ivc: update intention tree")
}

/// Write backfill intention tree JSON to .ivc/trees/backfill/PR-{N}.json and commit.
pub fn commit_backfill_json(
    workdir: &Path,
    content: &serde_json::Value,
    pr_number: u32,
) -> Result<()> {
    let json_path = backfill_file_path(workdir, pr_number);
    write_and_commit(
        workdir,
        &json_path,
        content,
        &format!("ivc: backfill intention tree for PR #{pr_number}"),
    )
}

fn write_and_commit(
    workdir: &Path,
    json_path: &Path,
    content: &serde_json::Value,
    commit_message: &str,
) -> Result<()> {
    if let Some(parent) = json_path.parent() {
        std::fs::create_dir_all(parent)
            .context("Failed to create trees directory")?;
    }

    let json_str =
        serde_json::to_string_pretty(content).context("Failed to serialize intention tree")?;
    std::fs::write(json_path, &json_str).context("Failed to write intention tree JSON")?;

    let relative = json_path
        .strip_prefix(workdir)
        .unwrap_or(json_path)
        .to_string_lossy()
        .to_string();

    git::commit::run_git_command("add", &[relative])?;
    git::commit::run_git_command("commit", &["-m".to_string(), commit_message.to_string()])?;

    Ok(())
}

fn tree_file_path(workdir: &Path, branch: &str) -> PathBuf {
    workdir
        .join(".ivc")
        .join("trees")
        .join(format!("{}.json", sanitize_branch_name(branch)))
}

fn backfill_file_path(workdir: &Path, pr_number: u32) -> PathBuf {
    workdir
        .join(".ivc")
        .join("trees")
        .join("backfill")
        .join(format!("PR-{pr_number}.json"))
}

fn sanitize_branch_name(branch: &str) -> String {
    branch.replace('/', "-").replace('\\', "-")
}

fn compute_integrity(json_value: &serde_json::Value) -> String {
    let canonical = serde_json::to_string(json_value).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    let hash = hasher.finalize();
    format!("sha256:{:x}", hash)
}
