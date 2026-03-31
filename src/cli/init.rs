use anyhow::{Context, Result};
use std::path::Path;

use crate::config;
use crate::db;
use crate::git;

pub async fn run() -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let repo = git::repo::open_repo(&cwd)?;

    let workdir = repo
        .workdir()
        .context("Could not determine repository working directory")?;
    let ivc_dir = workdir.join(".ivc");

    if ivc_dir.exists() {
        println!("IVC is already initialised in this repository.");
        return Ok(());
    }

    // Create .ivc/ directory structure
    let data_dir = ivc_dir.join("data");
    std::fs::create_dir_all(&data_dir).context("Failed to create .ivc/data directory")?;

    // Create trees directories (committed to git)
    let trees_dir = ivc_dir.join("trees");
    std::fs::create_dir_all(trees_dir.join("backfill"))
        .context("Failed to create .ivc/trees/backfill directory")?;

    // Write default config
    let config_path = ivc_dir.join("config.toml");
    std::fs::write(&config_path, config::default_config_toml())
        .context("Failed to write .ivc/config.toml")?;

    // Initialise SurrealDB (creates schema)
    db::connection::connect_embedded(&data_dir).await?;

    // Add .ivc/ to .gitignore if not already there
    add_to_gitignore(workdir)?;

    println!("IVC initialised. Intention data will be stored in .ivc/");
    Ok(())
}

const GITIGNORE_ENTRIES: &str = ".ivc/data/\n.ivc/config.toml\n";

fn add_to_gitignore(workdir: &Path) -> Result<()> {
    let gitignore_path = workdir.join(".gitignore");

    if gitignore_path.exists() {
        let content = std::fs::read_to_string(&gitignore_path)
            .context("Failed to read .gitignore")?;

        // Already migrated to specific ignores
        if content.lines().any(|line| line.trim() == ".ivc/data/") {
            return Ok(());
        }

        // Migrate from old blanket .ivc/ ignore to specific ignores
        if content.lines().any(|line| line.trim() == ".ivc/" || line.trim() == ".ivc") {
            let new_content = content
                .lines()
                .map(|line| {
                    if line.trim() == ".ivc/" || line.trim() == ".ivc" {
                        ".ivc/data/\n.ivc/config.toml"
                    } else {
                        line
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
                + "\n";
            std::fs::write(&gitignore_path, new_content)
                .context("Failed to update .gitignore")?;
            return Ok(());
        }

        // Append new entries
        let mut new_content = content;
        if !new_content.ends_with('\n') {
            new_content.push('\n');
        }
        new_content.push_str(GITIGNORE_ENTRIES);
        std::fs::write(&gitignore_path, new_content).context("Failed to update .gitignore")?;
    } else {
        std::fs::write(&gitignore_path, GITIGNORE_ENTRIES)
            .context("Failed to create .gitignore")?;
    }

    Ok(())
}
