use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub created_by: String,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Workspace {
    /// Archives every board in the workspace, then deletes the workspace, atomically.
    /// Boards keep their lists/cards; `boards.workspace_id` becomes NULL and
    /// memberships cascade away. Returns `RowNotFound` if the workspace does not exist.
    pub async fn delete_archiving_boards(
        pool: &SqlitePool,
        workspace_id: &str,
        now_ms: i64,
    ) -> Result<(), sqlx::Error> {
        let mut tx = pool.begin().await?;

        sqlx::query("UPDATE boards SET archived = 1, updated_at = ? WHERE workspace_id = ?")
            .bind(now_ms)
            .bind(workspace_id)
            .execute(&mut *tx)
            .await?;

        let deleted = sqlx::query("DELETE FROM workspaces WHERE id = ?")
            .bind(workspace_id)
            .execute(&mut *tx)
            .await?;
        if deleted.rows_affected() == 0 {
            return Err(sqlx::Error::RowNotFound);
        }

        tx.commit().await
    }
}

/// Mirrors the `CHECK(role IN ('admin','member','viewer'))` constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub enum WorkspaceRole {
    Admin,
    Member,
    Viewer,
}

#[cfg(test)]
mod tests {
    use super::*;

    const CREATED_MS: i64 = 1_700_000_000_000;
    const DELETED_MS: i64 = 1_700_000_999_000;

    async fn seeded_pool() -> SqlitePool {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        crate::db::run_migrations(&pool).await.unwrap();
        let ts = CREATED_MS;
        let statements = [
            format!("INSERT INTO users (id, email, name, password, created_at, updated_at) VALUES ('u1', 'a@b.c', 'A', 'hash', {ts}, {ts})"),
            format!("INSERT INTO workspaces (id, name, created_by, created_at, updated_at) VALUES ('w1', 'W1', 'u1', {ts}, {ts})"),
            format!("INSERT INTO workspaces (id, name, created_by, created_at, updated_at) VALUES ('w2', 'W2', 'u1', {ts}, {ts})"),
            format!("INSERT INTO workspace_members (workspace_id, user_id, role, joined_at) VALUES ('w1', 'u1', 'admin', {ts})"),
            format!("INSERT INTO boards (id, workspace_id, name, created_by, created_at, updated_at) VALUES ('b1', 'w1', 'B1', 'u1', {ts}, {ts})"),
            format!("INSERT INTO boards (id, workspace_id, name, created_by, created_at, updated_at) VALUES ('b2', 'w2', 'B2', 'u1', {ts}, {ts})"),
            format!("INSERT INTO lists (id, board_id, name, position, created_at, updated_at) VALUES ('l1', 'b1', 'L', 1.0, {ts}, {ts})"),
            format!("INSERT INTO cards (id, list_id, board_id, title, position, created_by, created_at, updated_at) VALUES ('c1', 'l1', 'b1', 'C', 1.0, 'u1', {ts}, {ts})"),
        ];
        for sql in statements {
            sqlx::query(&sql).execute(&pool).await.unwrap();
        }
        pool
    }

    async fn board_state(pool: &SqlitePool, id: &str) -> (Option<String>, bool, i64) {
        sqlx::query_as("SELECT workspace_id, archived, updated_at FROM boards WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn count(pool: &SqlitePool, sql: &str) -> i64 {
        sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
    }

    #[tokio::test]
    async fn delete_archives_boards_keeps_their_data_and_removes_workspace() {
        let pool = seeded_pool().await;

        Workspace::delete_archiving_boards(&pool, "w1", DELETED_MS)
            .await
            .unwrap();

        assert_eq!(count(&pool, "SELECT COUNT(*) FROM workspaces WHERE id = 'w1'").await, 0);
        assert_eq!(
            count(&pool, "SELECT COUNT(*) FROM workspace_members WHERE workspace_id = 'w1'").await,
            0
        );
        assert_eq!(board_state(&pool, "b1").await, (None, true, DELETED_MS));
        assert_eq!(count(&pool, "SELECT COUNT(*) FROM lists WHERE board_id = 'b1'").await, 1);
        assert_eq!(count(&pool, "SELECT COUNT(*) FROM cards WHERE board_id = 'b1'").await, 1);

        assert_eq!(
            board_state(&pool, "b2").await,
            (Some("w2".to_string()), false, CREATED_MS),
            "boards in other workspaces must be untouched"
        );
    }

    #[tokio::test]
    async fn delete_missing_workspace_is_not_found_and_changes_nothing() {
        let pool = seeded_pool().await;

        let err = Workspace::delete_archiving_boards(&pool, "missing", DELETED_MS)
            .await
            .unwrap_err();

        assert!(matches!(err, sqlx::Error::RowNotFound));
        assert_eq!(count(&pool, "SELECT COUNT(*) FROM workspaces").await, 2);
        assert_eq!(count(&pool, "SELECT COUNT(*) FROM boards WHERE archived = 1").await, 0);
    }

    #[test]
    fn role_serializes_to_db_check_values() {
        for (role, text) in [
            (WorkspaceRole::Admin, "admin"),
            (WorkspaceRole::Member, "member"),
            (WorkspaceRole::Viewer, "viewer"),
        ] {
            assert_eq!(serde_json::to_value(role).unwrap(), text);
            assert_eq!(
                serde_json::from_value::<WorkspaceRole>(text.into()).unwrap(),
                role
            );
        }
        assert!(serde_json::from_value::<WorkspaceRole>("owner".into()).is_err());
    }
}
