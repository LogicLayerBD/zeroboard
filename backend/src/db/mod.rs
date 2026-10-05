use std::str::FromStr;
use std::time::Duration;

use anyhow::Context;
use sqlx::migrate::Migrator;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};

static MIGRATOR: Migrator = sqlx::migrate!("./src/db/migrations");

/// SQLite in WAL mode allows a single writer; one pooled connection avoids lock contention.
const MAX_CONNECTIONS: u32 = 1;
const MIN_CONNECTIONS: u32 = 1;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
/// Negative value is in KiB: -20000 ≈ 20MB page cache.
const CACHE_SIZE_KIB: &str = "-20000";

/// WAL allows many concurrent readers alongside the single writer.
const READ_MAX_CONNECTIONS: u32 = 4;
const READ_MIN_CONNECTIONS: u32 = 1;
/// Reads can tolerate waiting longer than writes (e.g. during a WAL checkpoint).
const READ_BUSY_TIMEOUT: Duration = Duration::from_secs(30);
const READ_ONLY_MODE_PARAM: &str = "mode=ro";

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

/// Opens a read-only pool for SELECT-only queries. The database file must
/// already exist, so call this after `connect` and `run_migrations`.
pub async fn create_read_pool(database_url: &str) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(&read_only_url(database_url))
        .context("invalid DATABASE_URL")?
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(READ_BUSY_TIMEOUT)
        .pragma("cache_size", CACHE_SIZE_KIB)
        .pragma("temp_store", "MEMORY");

    SqlitePoolOptions::new()
        .max_connections(READ_MAX_CONNECTIONS)
        .min_connections(READ_MIN_CONNECTIONS)
        .connect_with(options)
        .await
        .context("failed to connect read pool to database")
}

fn read_only_url(database_url: &str) -> String {
    let separator = if database_url.contains('?') { '&' } else { '?' };
    format!("{database_url}{separator}{READ_ONLY_MODE_PARAM}")
}

/// Applies pending migrations. Each migration runs in its own transaction,
/// and already-applied migrations are verified by checksum.
pub async fn run_migrations(pool: &SqlitePool) -> anyhow::Result<()> {
    MIGRATOR
        .run(pool)
        .await
        .context("failed to run database migrations")?;
    tracing::info!(
        migrations = MIGRATOR.iter().count(),
        "database migrations up to date"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const TEST_USER_ID: &str = "u1";
    const TEST_TIMESTAMP_MS: i64 = 1_700_000_000_000;

    struct TempDb {
        dir: PathBuf,
        url: String,
        pool: SqlitePool,
    }

    impl TempDb {
        async fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("zeroboard-db-{}", uuid::Uuid::new_v4()));
            let url = format!("sqlite://{}", dir.join("nested/test.db").display());
            let pool = connect(&url).await.unwrap();
            run_migrations(&pool).await.unwrap();
            Self { dir, url, pool }
        }

        async fn cleanup(self) {
            self.pool.close().await;
            let _ = tokio::fs::remove_dir_all(&self.dir).await;
        }
    }

    async fn pragma_i64(pool: &SqlitePool, name: &str) -> i64 {
        sqlx::query_scalar(&format!("PRAGMA {name}"))
            .fetch_one(pool)
            .await
            .unwrap()
    }

    /// Seeds user -> workspace -> board -> list -> card, plus a label and
    /// one row in every card-dependent table.
    async fn seed_card_graph(pool: &SqlitePool) {
        let ts = TEST_TIMESTAMP_MS;
        let statements = [
            format!("INSERT INTO users (id, email, name, password, created_at, updated_at) VALUES ('{TEST_USER_ID}', 'a@b.c', 'A', 'hash', {ts}, {ts})"),
            format!("INSERT INTO workspaces (id, name, created_by, created_at, updated_at) VALUES ('w1', 'W', '{TEST_USER_ID}', {ts}, {ts})"),
            format!("INSERT INTO workspace_members (workspace_id, user_id, role, joined_at) VALUES ('w1', '{TEST_USER_ID}', 'admin', {ts})"),
            format!("INSERT INTO boards (id, workspace_id, name, created_by, created_at, updated_at) VALUES ('b1', 'w1', 'B', '{TEST_USER_ID}', {ts}, {ts})"),
            format!("INSERT INTO lists (id, board_id, name, position, created_at, updated_at) VALUES ('l1', 'b1', 'L', 1.0, {ts}, {ts})"),
            format!("INSERT INTO cards (id, list_id, board_id, title, position, created_by, created_at, updated_at) VALUES ('c1', 'l1', 'b1', 'C', 1.0, '{TEST_USER_ID}', {ts}, {ts})"),
            "INSERT INTO labels (id, board_id, name, color) VALUES ('lb1', 'b1', 'Bug', '#ff0000')".to_string(),
            format!("INSERT INTO card_assignees (card_id, user_id) VALUES ('c1', '{TEST_USER_ID}')"),
            "INSERT INTO card_labels (card_id, label_id) VALUES ('c1', 'lb1')".to_string(),
            format!("INSERT INTO attachments (id, card_id, filename, stored_path, size_bytes, uploaded_by, uploaded_at) VALUES ('a1', 'c1', 'f.txt', 'x/f.txt', 1, '{TEST_USER_ID}', {ts})"),
            format!("INSERT INTO time_entries (id, card_id, user_id, minutes, logged_at) VALUES ('t1', 'c1', '{TEST_USER_ID}', 30, {ts})"),
            format!("INSERT INTO comments (id, card_id, user_id, body, created_at, updated_at) VALUES ('cm1', 'c1', '{TEST_USER_ID}', 'hi', {ts}, {ts})"),
            format!("INSERT INTO activity_log (id, card_id, board_id, user_id, action, created_at) VALUES ('ac1', 'c1', 'b1', '{TEST_USER_ID}', 'created_card', {ts})"),
        ];
        for sql in statements {
            sqlx::query(&sql).execute(pool).await.unwrap();
        }
    }

    async fn count(pool: &SqlitePool, table: &str) -> i64 {
        sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn connect_applies_pragmas_and_creates_parent_dir() {
        let db = TempDb::new().await;

        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(journal_mode, "wal");
        assert_eq!(pragma_i64(&db.pool, "foreign_keys").await, 1);
        // synchronous: 1 = NORMAL
        assert_eq!(pragma_i64(&db.pool, "synchronous").await, 1);
        assert_eq!(pragma_i64(&db.pool, "busy_timeout").await, 5000);
        assert_eq!(pragma_i64(&db.pool, "cache_size").await, -20000);
        // temp_store: 2 = MEMORY
        assert_eq!(pragma_i64(&db.pool, "temp_store").await, 2);
        assert!(db.dir.join("nested").is_dir());

        db.cleanup().await;
    }

    /// SQLITE_READONLY primary result code.
    const SQLITE_READONLY_CODE: &str = "8";

    #[test]
    fn read_only_url_appends_mode_param() {
        assert_eq!(read_only_url("sqlite://data/z.db"), "sqlite://data/z.db?mode=ro");
        assert_eq!(
            read_only_url("sqlite://data/z.db?cache=shared"),
            "sqlite://data/z.db?cache=shared&mode=ro"
        );
    }

    #[tokio::test]
    async fn read_pool_opens_with_expected_pragmas() {
        let db = TempDb::new().await;
        let read = create_read_pool(&db.url).await.unwrap();

        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&read)
            .await
            .unwrap();
        assert_eq!(journal_mode, "wal");
        assert_eq!(pragma_i64(&read, "foreign_keys").await, 1);
        assert_eq!(pragma_i64(&read, "synchronous").await, 1);
        assert_eq!(pragma_i64(&read, "busy_timeout").await, 30000);
        assert_eq!(pragma_i64(&read, "cache_size").await, -20000);
        assert_eq!(pragma_i64(&read, "temp_store").await, 2);
        assert_eq!(count(&read, "users").await, 0);

        read.close().await;
        db.cleanup().await;
    }

    #[tokio::test]
    async fn read_pool_rejects_writes() {
        let db = TempDb::new().await;
        let read = create_read_pool(&db.url).await.unwrap();

        let err = sqlx::query(
            "INSERT INTO users (id, email, name, password, created_at, updated_at) \
             VALUES ('u1', 'a@b.c', 'A', 'hash', 0, 0)",
        )
        .execute(&read)
        .await
        .unwrap_err();

        let code = err.as_database_error().and_then(|e| e.code()).map(|c| c.into_owned());
        assert_eq!(code.as_deref(), Some(SQLITE_READONLY_CODE), "unexpected error: {err}");
        assert_eq!(count(&db.pool, "users").await, 0);

        read.close().await;
        db.cleanup().await;
    }

    #[tokio::test]
    async fn read_pool_sees_writes_from_write_pool() {
        let db = TempDb::new().await;
        let read = create_read_pool(&db.url).await.unwrap();

        seed_card_graph(&db.pool).await;

        assert_eq!(count(&read, "users").await, 1);
        assert_eq!(count(&read, "cards").await, 1);
        let file_of = |pool: SqlitePool| async move {
            let (_, _, file): (i64, String, String) =
                sqlx::query_as("PRAGMA database_list").fetch_one(&pool).await.unwrap();
            file
        };
        assert_eq!(file_of(db.pool.clone()).await, file_of(read.clone()).await);

        read.close().await;
        db.cleanup().await;
    }

    #[tokio::test]
    async fn migrations_create_all_spec_tables_and_are_idempotent() {
        let db = TempDb::new().await;

        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type = 'table' \
             AND name NOT LIKE 'sqlite_%' AND name != '_sqlx_migrations' ORDER BY name",
        )
        .fetch_all(&db.pool)
        .await
        .unwrap();
        let mut expected = vec![
            "activity_log", "attachments", "boards", "card_assignees", "card_labels", "cards",
            "comments", "labels", "lists", "notifications", "refresh_tokens", "time_entries",
            "users", "workspace_members", "workspaces",
        ];
        expected.sort_unstable();
        assert_eq!(tables, expected);

        run_migrations(&db.pool).await.unwrap();

        db.cleanup().await;
    }

    #[tokio::test]
    async fn foreign_keys_and_checks_are_enforced() {
        let db = TempDb::new().await;
        seed_card_graph(&db.pool).await;

        let orphan = sqlx::query(
            "INSERT INTO lists (id, board_id, name, position, created_at, updated_at) \
             VALUES ('l2', 'missing', 'L', 1.0, 0, 0)",
        )
        .execute(&db.pool)
        .await;
        assert!(orphan.is_err(), "insert with dangling board_id must fail");

        let bad_role = sqlx::query(
            "INSERT INTO workspace_members (workspace_id, user_id, role, joined_at) \
             VALUES ('w1', 'u1', 'owner', 0)",
        )
        .execute(&db.pool)
        .await;
        assert!(bad_role.is_err(), "role outside admin/member/viewer must fail");

        db.cleanup().await;
    }

    #[tokio::test]
    async fn deleting_list_cascades_cards_and_preserves_activity() {
        let db = TempDb::new().await;
        seed_card_graph(&db.pool).await;

        sqlx::query("DELETE FROM lists WHERE id = 'l1'")
            .execute(&db.pool)
            .await
            .unwrap();

        for table in [
            "cards", "card_assignees", "card_labels", "attachments", "time_entries", "comments",
        ] {
            assert_eq!(count(&db.pool, table).await, 0, "{table} should cascade");
        }
        let card_id: Option<String> =
            sqlx::query_scalar("SELECT card_id FROM activity_log WHERE id = 'ac1'")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(card_id, None);
        assert_eq!(count(&db.pool, "labels").await, 1);

        db.cleanup().await;
    }
}
