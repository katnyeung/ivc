use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use surrealdb::sql::Thing;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intention {
    pub id: Option<Thing>,
    pub title: String,
    pub reasoning: String,
    #[serde(rename = "type")]
    pub intention_type: IntentionType,
    pub files_changed: Vec<String>,
    pub uncertainties: Vec<String>,
    pub alternatives_considered: Vec<Alternative>,
    pub assumptions: Vec<String>,
    pub commit_shas: Vec<String>,
    pub branch: String,
    pub repo: String,
    pub source_type: SourceType,
    pub source_confidence: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backfill_metadata: Option<BackfillMetadata>,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackfillMetadata {
    pub backfilled_at: DateTime<Utc>,
    pub merge_commit: String,
    pub merge_date: DateTime<Utc>,
    pub pr_number: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IntentionType {
    Feature,
    BugFix,
    SecurityPatch,
    TechDebt,
    Refactor,
    Unknown,
}

impl std::fmt::Display for IntentionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IntentionType::Feature => write!(f, "FEATURE"),
            IntentionType::BugFix => write!(f, "BUG_FIX"),
            IntentionType::SecurityPatch => write!(f, "SECURITY_PATCH"),
            IntentionType::TechDebt => write!(f, "TECH_DEBT"),
            IntentionType::Refactor => write!(f, "REFACTOR"),
            IntentionType::Unknown => write!(f, "UNKNOWN"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alternative {
    pub approach: String,
    pub rejected_because: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SourceType {
    ReconstructedFromCommits,
    ReconstructedWithTicket,
    HumanProvided,
    Backfilled,
}

/// Tree structure for displaying intentions
pub struct IntentionTree {
    pub root: Intention,
    pub children: Vec<IntentionNode>,
}

pub struct IntentionNode {
    pub intention: Intention,
    pub depends_on: Vec<String>,
}
