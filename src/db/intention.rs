use anyhow::{Context, Result};
use surrealdb::engine::local::Db;
use surrealdb::sql::Thing;
use surrealdb::Surreal;

use crate::models::intention::{Intention, IntentionNode, IntentionTree};

/// Insert a new intention record.
pub async fn create(db: &Surreal<Db>, intention: &Intention) -> Result<Thing> {
    let result: Option<Intention> = db
        .create("intention")
        .content(intention.clone())
        .await
        .context("Failed to create intention")?;
    let record = result.context("No record returned after insert")?;
    record.id.context("Record has no ID")
}

/// Create a decomposed_into relation (parent -> child).
pub async fn create_decomposition(
    db: &Surreal<Db>,
    parent: &Thing,
    child: &Thing,
    order: i32,
) -> Result<()> {
    db.query("RELATE $parent->decomposed_into->$child SET order = $order")
        .bind(("parent", parent.clone()))
        .bind(("child", child.clone()))
        .bind(("order", order))
        .await
        .context("Failed to create decomposition relation")?;
    Ok(())
}

/// Create a depends_on relation between intentions.
pub async fn create_dependency(
    db: &Surreal<Db>,
    from: &Thing,
    to: &Thing,
    reason: Option<&str>,
) -> Result<()> {
    db.query("RELATE $from->depends_on->$to SET reason = $reason")
        .bind(("from", from.clone()))
        .bind(("to", to.clone()))
        .bind(("reason", reason.map(|s| s.to_string())))
        .await
        .context("Failed to create dependency relation")?;
    Ok(())
}

/// Create a derived_from_commit relation (intention -> commit_capture).
pub async fn create_derived_from(
    db: &Surreal<Db>,
    intention: &Thing,
    commit: &Thing,
) -> Result<()> {
    db.query("RELATE $intention->derived_from_commit->$commit")
        .bind(("intention", intention.clone()))
        .bind(("commit", commit.clone()))
        .await
        .context("Failed to create derived_from_commit relation")?;
    Ok(())
}

/// Get the intention tree for a branch.
pub async fn get_tree_for_branch(
    db: &Surreal<Db>,
    repo: &str,
    branch: &str,
) -> Result<Option<IntentionTree>> {
    // Get root intention (the one that has no incoming decomposed_into)
    let mut result = db
        .query(
            r#"
            SELECT * FROM intention
            WHERE repo = $repo AND branch = $branch
            AND id NOT IN (SELECT VALUE out FROM decomposed_into)
            LIMIT 1
            "#,
        )
        .bind(("repo", repo.to_string()))
        .bind(("branch", branch.to_string()))
        .await
        .context("Failed to query root intention")?;

    let roots: Vec<Intention> = result.take(0)?;
    let root = match roots.into_iter().next() {
        Some(r) => r,
        None => return Ok(None),
    };

    let root_id = root.id.as_ref().context("Root intention has no ID")?;

    // Get children via decomposed_into relation
    let mut result = db
        .query(
            r#"
            SELECT out, order FROM decomposed_into
            WHERE in = $root_id
            ORDER BY order ASC
            "#,
        )
        .bind(("root_id", root_id.clone()))
        .await
        .context("Failed to query child intentions")?;

    let child_ids: Vec<Thing> = result.take("out").unwrap_or_default();

    let mut nodes = Vec::new();
    for child_id in &child_ids {
        // Fetch the child intention
        let mut child_result = db
            .query("SELECT * FROM intention WHERE id = $child_id")
            .bind(("child_id", child_id.clone()))
            .await
            .context("Failed to fetch child intention")?;
        let children: Vec<Intention> = child_result.take(0).unwrap_or_default();

        if let Some(child) = children.into_iter().next() {
            // Get dependency titles
            let mut dep_result = db
                .query(
                    r#"
                    SELECT VALUE out.title FROM depends_on
                    WHERE in = $child_id
                    "#,
                )
                .bind(("child_id", child_id.clone()))
                .await
                .context("Failed to query dependencies")?;
            let dep_titles: Vec<String> = dep_result.take(0).unwrap_or_default();

            nodes.push(IntentionNode {
                intention: child,
                depends_on: dep_titles,
            });
        }
    }

    Ok(Some(IntentionTree {
        root,
        children: nodes,
    }))
}

/// Get all intentions that touched a specific file path.
pub async fn get_by_file(
    db: &Surreal<Db>,
    repo: &str,
    file_path: &str,
) -> Result<Vec<Intention>> {
    let mut result = db
        .query(
            r#"
            SELECT * FROM intention
            WHERE repo = $repo AND $file IN files_changed
            ORDER BY created_at ASC
            "#,
        )
        .bind(("repo", repo.to_string()))
        .bind(("file", file_path.to_string()))
        .await
        .context("Failed to query intentions by file")?;
    let intentions: Vec<Intention> = result.take(0)?;
    Ok(intentions)
}

/// Check if intentions already exist for any of the given commit SHAs.
pub async fn has_intentions_for_commits(
    db: &Surreal<Db>,
    repo: &str,
    commit_shas: &[String],
) -> Result<bool> {
    let mut result = db
        .query(
            r#"
            SELECT count() AS count FROM intention
            WHERE repo = $repo AND commit_shas CONTAINSANY $shas
            GROUP ALL
            "#,
        )
        .bind(("repo", repo.to_string()))
        .bind(("shas", commit_shas.to_vec()))
        .await
        .context("Failed to check for existing intentions")?;

    let counts: Vec<serde_json::Value> = result.take(0).unwrap_or_default();
    if let Some(first) = counts.first() {
        if let Some(count) = first.get("count").and_then(|v| v.as_u64()) {
            return Ok(count > 0);
        }
    }
    Ok(false)
}

/// Delete all intentions and their relations for a branch.
pub async fn delete_for_branch(db: &Surreal<Db>, repo: &str, branch: &str) -> Result<()> {
    db.query(
        r#"
        LET $ids = (SELECT VALUE id FROM intention WHERE repo = $repo AND branch = $branch);
        DELETE FROM decomposed_into WHERE in IN $ids OR out IN $ids;
        DELETE FROM depends_on WHERE in IN $ids OR out IN $ids;
        DELETE FROM derived_from_commit WHERE in IN $ids;
        DELETE FROM intention WHERE repo = $repo AND branch = $branch;
        "#,
    )
    .bind(("repo", repo.to_string()))
    .bind(("branch", branch.to_string()))
    .await
    .context("Failed to delete intentions for branch")?;
    Ok(())
}
