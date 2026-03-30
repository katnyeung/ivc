use anyhow::{Context, Result};

use crate::db;
use crate::git;
use crate::models::intention::IntentionTree;

pub async fn run(file: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let repo = git::repo::open_repo(&cwd)?;
    git::repo::require_ivc_initialised(&repo)?;

    let repo_name = git::repo::get_repo_name(&repo)?;
    let ivc_dir = git::repo::get_ivc_dir(&repo)?;
    let data_dir = ivc_dir.join("data");
    let db = db::connection::connect_embedded(&data_dir).await?;

    if let Some(file_path) = file {
        // File mode: show all intentions across all branches that touched this file
        let intentions = db::intention::get_by_file(&db, &repo_name, &file_path).await?;

        if intentions.is_empty() {
            println!(
                "No intentions found for file '{file_path}'. Run 'ivc pr' or 'ivc backfill' to extract intentions."
            );
            return Ok(());
        }

        println!();
        println!("Intentions for {file_path}");
        println!();

        for (i, intention) in intentions.iter().enumerate() {
            let is_last = i == intentions.len() - 1;
            let prefix = if is_last { "└── " } else { "├── " };
            let continuation = if is_last { "    " } else { "│   " };

            println!("{}{}", prefix, intention.title);
            println!(
                "{}Type: {} | Branch: {} | Source: {:?} (confidence: {:.2})",
                continuation,
                intention.intention_type,
                intention.branch,
                intention.source_type,
                intention.source_confidence,
            );
            println!("{}Reasoning: {}", continuation, intention.reasoning);

            if let Some(date) = &intention.created_at {
                println!("{}Date: {}", continuation, date.format("%Y-%m-%d"));
            }

            if !is_last {
                println!("│");
            }
        }
    } else {
        // Branch mode: show intention tree for current branch
        let branch = git::repo::get_current_branch(&repo)?;

        match db::intention::get_tree_for_branch(&db, &repo_name, &branch).await? {
            Some(tree) => display_tree(&tree, &branch),
            None => {
                println!(
                    "No intentions found for branch '{branch}'. Run 'ivc pr' to extract intentions."
                );
            }
        }
    }

    Ok(())
}

fn display_tree(tree: &IntentionTree, branch: &str) {
    println!();
    println!("Intention tree for {}", branch);
    println!();
    println!("Root: {}", tree.root.title);
    println!("Type: {}", tree.root.intention_type);
    println!("Reasoning: {}", tree.root.reasoning);
    println!();

    for (i, node) in tree.children.iter().enumerate() {
        let is_last = i == tree.children.len() - 1;
        let prefix = if is_last { "└── " } else { "├── " };
        let continuation = if is_last { "    " } else { "│   " };

        println!(
            "{}Intention {}: {}",
            prefix,
            i + 1,
            node.intention.title
        );
        println!("{}Type: {}", continuation, node.intention.intention_type);
        println!(
            "{}Files: {}",
            continuation,
            node.intention.files_changed.join(", ")
        );
        println!("{}Reasoning: {}", continuation, node.intention.reasoning);

        if !node.depends_on.is_empty() {
            println!(
                "{}Depends on: {}",
                continuation,
                node.depends_on.join(", ")
            );
        }

        if !node.intention.uncertainties.is_empty() {
            println!("{}Uncertainties:", continuation);
            for u in &node.intention.uncertainties {
                println!("{}  - {}", continuation, u);
            }
        }

        if !is_last {
            println!("│");
        }
    }
}
