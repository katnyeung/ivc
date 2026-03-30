use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use surrealdb::sql::Thing;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: Option<Thing>,
    pub event_type: EventType,
    pub source: String,
    pub intention: Option<Thing>,
    pub payload: serde_json::Value,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventType {
    CommitCaptured,
    PushSynced,
    IntentionsExtracted,
    IntentionsBackfilled,
    PrCreated,
}
