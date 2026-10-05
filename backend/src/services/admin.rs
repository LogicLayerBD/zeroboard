use std::str::FromStr;

use anyhow::Context;
use serde::Serialize;
use sqlx::sqlite::SqliteConnectOptions;

use crate::errors::AppError;
use crate::AppState;

#[derive(Debug, Serialize)]
pub struct ServerInfo {
    pub version: &'static str,
    pub uptime_seconds: u64,
    /// Size of the main database file (excludes the WAL); 0 for in-memory databases.
    pub db_size_bytes: u64,
    pub user_count: i64,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceStorage {
    /// `None` groups boards whose workspace was deleted (archived boards).
    pub workspace_id: Option<String>,
    pub workspace_name: Option<String>,
    pub attachment_count: i64,
    pub size_bytes: i64,
}

#[derive(Debug, Serialize)]
pub struct StorageUsage {
    pub total_bytes: i64,
    pub attachment_count: i64,
    /// Workspaces that have at least one attachment, largest first.
    pub workspaces: Vec<WorkspaceStorage>,
}

pub async fn server_info(state: &AppState) -> Result<ServerInfo, AppError> {
    let user_count = sqlx::query!(r#"SELECT COUNT(*) AS "count!: i64" FROM users"#)
        .fetch_one(&state.read_db)
        .await?
        .count;

    Ok(ServerInfo {
        version: env!("CARGO_PKG_VERSION"),
        uptime_seconds: state.started_at.elapsed().as_secs(),
        db_size_bytes: db_file_size(&state.config.database_url).await?,
        user_count,
    })
}

async fn db_file_size(database_url: &str) -> Result<u64, AppError> {
    let path = SqliteConnectOptions::from_str(database_url)
        .context("invalid DATABASE_URL")?
        .get_filename()
        .to_path_buf();
    match tokio::fs::metadata(&path).await {
        Ok(metadata) => Ok(metadata.len()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(err) => Err(anyhow::Error::new(err)
            .context("failed to stat database file")
            .into()),
    }
}

pub async fn storage_usage(state: &AppState) -> Result<StorageUsage, AppError> {
    let workspaces = sqlx::query_as!(
        WorkspaceStorage,
        r#"SELECT b.workspace_id AS "workspace_id?",
                  w.name AS "workspace_name?",
                  COUNT(a.id) AS "attachment_count!: i64",
                  COALESCE(SUM(a.size_bytes), 0) AS "size_bytes!: i64"
           FROM attachments a
           JOIN cards c ON c.id = a.card_id
           JOIN boards b ON b.id = c.board_id
           LEFT JOIN workspaces w ON w.id = b.workspace_id
           GROUP BY b.workspace_id, w.name
           ORDER BY COALESCE(SUM(a.size_bytes), 0) DESC, b.workspace_id"#
    )
    .fetch_all(&state.read_db)
    .await?;

    Ok(StorageUsage {
        total_bytes: workspaces.iter().map(|w| w.size_bytes).sum(),
        attachment_count: workspaces.iter().map(|w| w.attachment_count).sum(),
        workspaces,
    })
}
