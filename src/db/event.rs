use anyhow::{Context, Result};
use surrealdb::engine::local::Db;
use surrealdb::Surreal;

use crate::models::event::Event;

/// Record an event in the append-only event log.
pub async fn record(db: &Surreal<Db>, event: &Event) -> Result<()> {
    let _: Option<Event> = db
        .create("event")
        .content(event.clone())
        .await
        .context("Failed to record event")?;
    Ok(())
}
