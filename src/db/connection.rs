use anyhow::{Context, Result};
use std::path::Path;
use surrealdb::engine::local::{Db, Mem, SurrealKv};
use surrealdb::Surreal;

use super::schema;

/// Connect to embedded SurrealDB at the given path.
pub async fn connect_embedded(path: &Path) -> Result<Surreal<Db>> {
    let db: Surreal<Db> = Surreal::new::<SurrealKv>(path)
        .await
        .context("Failed to connect to embedded SurrealDB")?;
    db.use_ns("ivc")
        .use_db("ivc")
        .await
        .context("Failed to select namespace/database")?;
    schema::initialise(&db).await?;
    Ok(db)
}

/// Connect to in-memory SurrealDB (for tests).
pub async fn connect_memory() -> Result<Surreal<Db>> {
    let db: Surreal<Db> = Surreal::new::<Mem>(())
        .await
        .context("Failed to connect to in-memory SurrealDB")?;
    db.use_ns("ivc")
        .use_db("ivc")
        .await
        .context("Failed to select namespace/database")?;
    schema::initialise(&db).await?;
    Ok(db)
}
