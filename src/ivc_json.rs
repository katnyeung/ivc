use anyhow::{Context, Result};
use std::path::Path;

use crate::git;
use crate::models::intention::IntentionTree;

/// Generate the .ivc.json content from an IntentionTree.
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

    serde_json::json!({
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
    })
}

/// Write .ivc.json to the repo root and commit it.
pub fn commit_ivc_json(workdir: &Path, content: &serde_json::Value) -> Result<()> {
    let json_path = workdir.join(".ivc.json");
    let json_str =
        serde_json::to_string_pretty(content).context("Failed to serialize .ivc.json")?;

    std::fs::write(&json_path, &json_str).context("Failed to write .ivc.json")?;

    git::commit::run_git_command("add", &[".ivc.json".to_string()])?;
    git::commit::run_git_command(
        "commit",
        &[
            "-m".to_string(),
            "ivc: update intention tree".to_string(),
        ],
    )?;

    Ok(())
}
