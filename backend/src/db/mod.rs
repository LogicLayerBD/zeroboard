use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use anyhow::Context;
use sqlx::migrate::Migrator;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteConnection, SqliteJournalMode, SqlitePool, SqlitePoolOptions,
    SqliteSynchronous,
};
use sqlx::{ConnectOptions, Connection};

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

/// A backup may wait behind the server's writes; give it longer than a request would.
const BACKUP_BUSY_TIMEOUT: Duration = Duration::from_secs(30);
/// SQLite's side files for a WAL database live next to it as `<db>-wal` / `<db>-shm`.
const WAL_SUFFIX: &str = "-wal";
const SHM_SUFFIX: &str = "-shm";
/// The backup is copied next to the live database first so the final swap is a same-filesystem rename.
const RESTORE_STAGING_SUFFIX: &str = ".restoring";
const INTEGRITY_CHECK_OK: &str = "ok";
const MIGRATIONS_TABLE: &str = "_sqlx_migrations";

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

/// For `INSERT/UPDATE/DELETE ... RETURNING` on the write pool, use
/// `.fetch_all(&state.db).await.and_then(db::single_row)` instead of `fetch_one`
/// (or `.map(db::first_row)` instead of `fetch_optional`). sqlx's SQLite worker hands
/// back the first row before the statement finishes, and an autocommit write only
/// commits when it finishes, so a read-pool query issued right after `fetch_one`
/// can miss the write. Draining every row waits for the commit.
pub fn single_row<T>(rows: Vec<T>) -> Result<T, sqlx::Error> {
    rows.into_iter().next().ok_or(sqlx::Error::RowNotFound)
}

/// See [`single_row`].
pub fn first_row<T>(rows: Vec<T>) -> Option<T> {
    rows.into_iter().next()
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

/// Writes a consistent snapshot of the database to `output` with `VACUUM INTO`.
/// Safe while the server is running. Refuses to overwrite an existing file.
pub async fn backup(database_url: &str, output: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(!output.exists(), "backup output {} already exists", output.display());
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create backup directory {}", parent.display()))?;
    }
    let output_str = output.to_str().context("backup output path must be valid UTF-8")?;

    let mut conn = SqliteConnectOptions::from_str(database_url)
        .context("invalid DATABASE_URL")?
        .busy_timeout(BACKUP_BUSY_TIMEOUT)
        .connect()
        .await
        .context("failed to open database (does it exist?)")?;
    sqlx::query("VACUUM INTO ?")
        .bind(output_str)
        .execute(&mut conn)
        .await
        .context("VACUUM INTO failed")?;
    conn.close().await.context("failed to close database")?;

    tracing::info!(output = %output.display(), "backup written");
    Ok(())
}

/// Replaces the database at `database_url` with the backup at `input` after
/// verifying it. The server must be stopped first: it would keep using the old file.
pub async fn restore(database_url: &str, input: &Path) -> anyhow::Result<()> {
    let db_path = SqliteConnectOptions::from_str(database_url)
        .context("invalid DATABASE_URL")?
        .get_filename()
        .to_path_buf();
    if let Some(parent) = db_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create database directory {}", parent.display()))?;
    }

    let staging = with_suffix(&db_path, RESTORE_STAGING_SUFFIX);
    tokio::fs::copy(input, &staging)
        .await
        .with_context(|| format!("failed to read backup {}", input.display()))?;
    if let Err(err) = verify_backup(&staging).await {
        remove_if_exists(&staging).await?;
        return Err(err);
    }

    // A leftover WAL from the old database would be replayed into the restored one.
    for suffix in [WAL_SUFFIX, SHM_SUFFIX] {
        remove_if_exists(&with_suffix(&db_path, suffix)).await?;
    }
    tokio::fs::rename(&staging, &db_path)
        .await
        .with_context(|| format!("failed to move restored database to {}", db_path.display()))?;

    tracing::info!(database = %db_path.display(), "database restored");
    Ok(())
}

/// Accepts only an intact SQLite file that was created by ZeroBoard.
async fn verify_backup(path: &Path) -> anyhow::Result<()> {
    let mut conn: SqliteConnection = SqliteConnectOptions::new()
        .filename(path)
        .connect()
        .await
        .context("failed to open backup")?;

    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&mut conn)
        .await
        .context("backup is not a valid SQLite database")?;
    anyhow::ensure!(integrity == INTEGRITY_CHECK_OK, "backup failed integrity check");

    let migrations_tables: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?")
            .bind(MIGRATIONS_TABLE)
            .fetch_one(&mut conn)
            .await
            .context("failed to inspect backup schema")?;
    anyhow::ensure!(migrations_tables == 1, "backup is not a ZeroBoard database");

    conn.close().await.context("failed to close backup")?;
    Ok(())
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(suffix);
    PathBuf::from(name)
}

async fn remove_if_exists(path: &Path) -> anyhow::Result<()> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("failed to remove {}", path.display())),
    }
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

    /// Enough rounds that the `fetch_one` race reliably shows up (it misses dozens).
    const VISIBILITY_ROUNDS: i64 = 500;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn drained_returning_write_is_visible_to_read_pool() {
        let db = TempDb::new().await;
        let read = create_read_pool(&db.url).await.unwrap();

        let mut stale = 0;
        for round in 1..=VISIBILITY_ROUNDS {
            let id = format!("u{round}");
            let returned: String = sqlx::query_scalar(
                "INSERT INTO users (id, email, name, password, created_at, updated_at) \
                 VALUES ($1, $1 || '@b.c', 'N', 'hash', 0, 0) RETURNING id",
            )
            .bind(&id)
            .fetch_all(&db.pool)
            .await
            .and_then(single_row)
            .unwrap();
            assert_eq!(returned, id);
            if count(&read, "users").await != round {
                stale += 1;
            }
        }
        assert_eq!(
            stale, 0,
            "read pool missed {stale} of {VISIBILITY_ROUNDS} committed writes"
        );

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
            "comments", "instance_settings", "labels", "lists", "notifications", "refresh_tokens",
            "time_entries",
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

    #[tokio::test]
    async fn backup_snapshots_live_database_and_refuses_overwrite() {
        let db = TempDb::new().await;
        seed_card_graph(&db.pool).await;
        let output = db.dir.join("backups/snapshot.db");

        backup(&db.url, &output).await.unwrap();

        let snapshot = connect(&format!("sqlite://{}", output.display())).await.unwrap();
        assert_eq!(count(&snapshot, "cards").await, 1);
        assert_eq!(count(&snapshot, "users").await, 1);
        snapshot.close().await;

        let err = backup(&db.url, &output).await.unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");

        db.cleanup().await;
    }

    #[tokio::test]
    async fn backup_fails_when_database_missing() {
        let dir = std::env::temp_dir().join(format!("zeroboard-db-{}", uuid::Uuid::new_v4()));
        let url = format!("sqlite://{}", dir.join("missing.db").display());
        assert!(backup(&url, &dir.join("out.db")).await.is_err());
        assert!(!dir.join("missing.db").exists());
    }

    #[tokio::test]
    async fn restore_replaces_database_and_drops_stale_wal() {
        let db = TempDb::new().await;
        seed_card_graph(&db.pool).await;
        let snapshot = db.dir.join("snapshot.db");
        backup(&db.url, &snapshot).await.unwrap();

        sqlx::query(
            "INSERT INTO users (id, email, name, password, created_at, updated_at) \
             VALUES ('u2', 'after@backup.c', 'B', 'hash', 0, 0)",
        )
        .execute(&db.pool)
        .await
        .unwrap();
        assert_eq!(count(&db.pool, "users").await, 2);
        db.pool.close().await;
        let db_path = db.dir.join("nested/test.db");
        let stale_wal = with_suffix(&db_path, WAL_SUFFIX);
        tokio::fs::write(&stale_wal, b"stale wal from the replaced database").await.unwrap();

        restore(&db.url, &snapshot).await.unwrap();

        assert!(!stale_wal.exists());
        assert!(!with_suffix(&db_path, RESTORE_STAGING_SUFFIX).exists());
        let restored = connect(&db.url).await.unwrap();
        assert_eq!(count(&restored, "users").await, 1);
        assert_eq!(count(&restored, "cards").await, 1);
        restored.close().await;

        let _ = tokio::fs::remove_dir_all(&db.dir).await;
    }

    #[tokio::test]
    async fn restore_rejects_invalid_backups_and_keeps_database() {
        let db = TempDb::new().await;
        seed_card_graph(&db.pool).await;
        db.pool.close().await;

        let garbage = db.dir.join("garbage.db");
        tokio::fs::write(&garbage, b"definitely not sqlite").await.unwrap();
        let foreign = db.dir.join("foreign.db");
        let foreign_pool = connect(&format!("sqlite://{}", foreign.display())).await.unwrap();
        sqlx::query("CREATE TABLE other (id INTEGER)").execute(&foreign_pool).await.unwrap();
        foreign_pool.close().await;

        for (input, expected) in [
            (&garbage, "not a valid SQLite database"),
            (&foreign, "not a ZeroBoard database"),
        ] {
            let err = restore(&db.url, input).await.unwrap_err();
            assert!(format!("{err:#}").contains(expected), "{}: {err:#}", input.display());
        }

        let db_path = db.dir.join("nested/test.db");
        assert!(!with_suffix(&db_path, RESTORE_STAGING_SUFFIX).exists());
        let kept = connect(&db.url).await.unwrap();
        assert_eq!(count(&kept, "cards").await, 1);
        kept.close().await;

        let _ = tokio::fs::remove_dir_all(&db.dir).await;
    }
}
