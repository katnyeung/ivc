use anyhow::{Context, Result};
use surrealdb::engine::local::Db;
use surrealdb::Surreal;

/// Initialise the SurrealDB schema. Idempotent — safe to call on every connection.
pub async fn initialise(db: &Surreal<Db>) -> Result<()> {
    db.query(SCHEMA)
        .await
        .context("Failed to initialise SurrealDB schema")?;
    Ok(())
}

const SCHEMA: &str = r#"
-- Commit metadata captured at commit time
DEFINE TABLE IF NOT EXISTS commit_capture SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS commit_sha ON commit_capture TYPE string;
DEFINE FIELD IF NOT EXISTS message ON commit_capture TYPE string;
DEFINE FIELD IF NOT EXISTS branch ON commit_capture TYPE string;
DEFINE FIELD IF NOT EXISTS repo ON commit_capture TYPE string;
DEFINE FIELD IF NOT EXISTS files_changed ON commit_capture TYPE array;
DEFINE FIELD IF NOT EXISTS files_changed.* ON commit_capture TYPE string;
DEFINE FIELD IF NOT EXISTS diff_stats ON commit_capture TYPE object;
DEFINE FIELD IF NOT EXISTS diff_stats.additions ON commit_capture TYPE int;
DEFINE FIELD IF NOT EXISTS diff_stats.deletions ON commit_capture TYPE int;
DEFINE FIELD IF NOT EXISTS diff_stats.files_modified ON commit_capture TYPE int;
DEFINE FIELD IF NOT EXISTS ticket_ref ON commit_capture TYPE option<string>;
DEFINE FIELD IF NOT EXISTS processed ON commit_capture TYPE bool DEFAULT false;
DEFINE FIELD IF NOT EXISTS created_at ON commit_capture TYPE datetime DEFAULT time::now();

DEFINE INDEX IF NOT EXISTS commit_sha_idx ON commit_capture FIELDS commit_sha UNIQUE;

-- Structured intentions extracted by LLM
DEFINE TABLE IF NOT EXISTS intention SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS title ON intention TYPE string;
DEFINE FIELD IF NOT EXISTS reasoning ON intention TYPE string;
DEFINE FIELD IF NOT EXISTS type ON intention TYPE string;
DEFINE FIELD IF NOT EXISTS files_changed ON intention TYPE array;
DEFINE FIELD IF NOT EXISTS files_changed.* ON intention TYPE string;
DEFINE FIELD IF NOT EXISTS uncertainties ON intention TYPE array;
DEFINE FIELD IF NOT EXISTS uncertainties.* ON intention TYPE string;
DEFINE FIELD IF NOT EXISTS alternatives_considered ON intention TYPE array;
DEFINE FIELD IF NOT EXISTS assumptions ON intention TYPE array;
DEFINE FIELD IF NOT EXISTS assumptions.* ON intention TYPE string;
DEFINE FIELD IF NOT EXISTS commit_shas ON intention TYPE array;
DEFINE FIELD IF NOT EXISTS commit_shas.* ON intention TYPE string;
DEFINE FIELD IF NOT EXISTS branch ON intention TYPE string;
DEFINE FIELD IF NOT EXISTS repo ON intention TYPE string;
DEFINE FIELD IF NOT EXISTS source_type ON intention TYPE string;
DEFINE FIELD IF NOT EXISTS source_confidence ON intention TYPE float;
DEFINE FIELD IF NOT EXISTS backfill_metadata ON intention TYPE option<object>;
DEFINE FIELD IF NOT EXISTS created_at ON intention TYPE datetime DEFAULT time::now();

DEFINE INDEX IF NOT EXISTS commit_branch_idx ON commit_capture FIELDS repo, branch;

DEFINE INDEX IF NOT EXISTS intention_branch_idx ON intention FIELDS repo, branch;

-- Parent-child: root intention decomposes into sub-intentions
DEFINE TABLE IF NOT EXISTS decomposed_into SCHEMAFULL TYPE RELATION IN intention OUT intention;
DEFINE FIELD IF NOT EXISTS order ON decomposed_into TYPE int;

-- Dependencies between sibling intentions
DEFINE TABLE IF NOT EXISTS depends_on SCHEMAFULL TYPE RELATION IN intention OUT intention;
DEFINE FIELD IF NOT EXISTS reason ON depends_on TYPE option<string>;

-- Link intention to the commits it was derived from
DEFINE TABLE IF NOT EXISTS derived_from_commit SCHEMAFULL TYPE RELATION IN intention OUT commit_capture;

-- Append-only event log
DEFINE TABLE IF NOT EXISTS event SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS event_type ON event TYPE string;
DEFINE FIELD IF NOT EXISTS source ON event TYPE string;
DEFINE FIELD IF NOT EXISTS intention ON event TYPE option<record<intention>>;
DEFINE FIELD IF NOT EXISTS payload ON event TYPE object;
DEFINE FIELD IF NOT EXISTS created_at ON event TYPE datetime DEFAULT time::now();
"#;
