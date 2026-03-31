use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct IvcConfig {
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub ai: AiConfig,
    #[serde(default)]
    pub git: GitConfig,
    #[serde(default)]
    pub github: GithubConfig,
}

#[derive(Debug, Deserialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_db_mode")]
    pub mode: String,
    #[serde(default = "default_db_path")]
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct AiConfig {
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_model")]
    pub model: String,
}

#[derive(Debug, Deserialize)]
pub struct GitConfig {
    #[serde(default = "default_base")]
    pub default_base: String,
}

fn default_db_mode() -> String {
    "embedded".to_string()
}
fn default_db_path() -> String {
    ".ivc/data".to_string()
}
fn default_provider() -> String {
    "anthropic".to_string()
}
fn default_model() -> String {
    "claude-sonnet-4-20250514".to_string()
}
#[derive(Debug, Deserialize, Default)]
pub struct GithubConfig {
    pub owner: Option<String>,
    pub repo: Option<String>,
}

fn default_base() -> String {
    "main".to_string()
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            mode: default_db_mode(),
            path: default_db_path(),
        }
    }
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: default_model(),
        }
    }
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            default_base: default_base(),
        }
    }
}

impl Default for IvcConfig {
    fn default() -> Self {
        Self {
            database: DatabaseConfig::default(),
            ai: AiConfig::default(),
            git: GitConfig::default(),
            github: GithubConfig::default(),
        }
    }
}

pub fn load_config(ivc_dir: &Path) -> Result<IvcConfig> {
    let config_path = ivc_dir.join("config.toml");
    if !config_path.exists() {
        return Ok(IvcConfig::default());
    }
    let content =
        std::fs::read_to_string(&config_path).context("Failed to read .ivc/config.toml")?;
    let config: IvcConfig = toml::from_str(&content).context("Failed to parse .ivc/config.toml")?;
    Ok(config)
}

pub fn default_config_toml() -> &'static str {
    r#"[database]
mode = "embedded"
path = ".ivc/data"

[ai]
provider = "anthropic"
model = "claude-sonnet-4-20250514"
# api_key is read from ANTHROPIC_API_KEY env var, never stored in config

[git]
default_base = "main"

# [github]
# owner and repo are auto-detected from git remote origin
# GITHUB_TOKEN env var is used for authentication
# owner = "your-github-username"
# repo = "your-repo-name"
"#
}
