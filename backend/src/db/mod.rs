use std::str::FromStr;
use std::time::Duration;

use anyhow::Context;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};

/// SQLite in WAL mode allows a single writer; one pooled connection avoids lock contention.
const MAX_CONNECTIONS: u32 = 1;
const MIN_CONNECTIONS: u32 = 1;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
/// Negative value is in KiB: -20000 ≈ 20MB page cache.
const CACHE_SIZE_KIB: &str = "-20000";

pub async fn connect(database_url: &str) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(database_url)
        .context("invalid DATABASE_URL")?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(BUSY_TIMEOUT)
        .pragma("cache_size", CACHE_SIZE_KIB)
        .pragma("temp_store", "MEMORY");

    let db_path = options.clone().get_filename();
    if let Some(parent) = db_path.parent() {
        if !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("failed to create database directory {}", parent.display()))?;
        }
    }

    SqlitePoolOptions::new()
        .max_connections(MAX_CONNECTIONS)
        .min_connections(MIN_CONNECTIONS)
        .connect_with(options)
        .await
        .context("failed to connect to database")
}
