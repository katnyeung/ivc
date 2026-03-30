use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use surrealdb::sql::Thing;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitCapture {
    pub id: Option<Thing>,
    pub commit_sha: String,
    pub message: String,
    pub branch: String,
    pub repo: String,
    pub files_changed: Vec<String>,
    pub diff_stats: DiffStats,
    pub ticket_ref: Option<String>,
    pub processed: bool,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffStats {
    pub additions: u32,
    pub deletions: u32,
    pub files_modified: u32,
}

impl DiffStats {
    pub fn new(additions: u32, deletions: u32, files_modified: u32) -> Self {
        Self {
            additions,
            deletions,
            files_modified,
        }
    }
}
