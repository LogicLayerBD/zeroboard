use serde::Serialize;

use super::{is_foreign_key_violation, is_unique_violation, now_ms};
use crate::db;
use crate::errors::AppError;
use crate::models::{Workspace, WorkspaceRole};
use crate::ws::events;
use crate::AppState;

const USER_NOT_FOUND_MESSAGE: &str = "no user is registered with that email";
const ALREADY_MEMBER_MESSAGE: &str = "user is already a member of this workspace";
const LAST_ADMIN_REMOVE_MESSAGE: &str = "cannot remove the last admin of a workspace";
const LAST_ADMIN_DEMOTE_MESSAGE: &str = "cannot demote the last admin of a workspace";

/// A workspace membership joined with the member's public profile.
#[derive(Debug, Clone, Serialize)]
pub struct MemberDetails {
    pub user_id: String,
    pub name: String,
    pub email: String,
    pub avatar_color: String,
    pub role: WorkspaceRole,
    pub joined_at: i64,
}

/// Creates the workspace and makes the creator its admin in one transaction.
pub async fn create(state: &AppState, user_id: &str, name: &str) -> Result<Workspace, AppError> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_ms();
    let admin = WorkspaceRole::Admin;

    let mut tx = state.db.begin().await?;
    let workspace = sqlx::query_as!(
        Workspace,
        r#"INSERT INTO workspaces (id, name, created_by, created_at, updated_at)
           VALUES ($1, $2, $3, $4, $5)
           RETURNING id AS "id!", name AS "name!", created_by AS "created_by!",
                     created_at AS "created_at!", updated_at AS "updated_at!""#,
        id,
        name,
        user_id,
        now,
        now
    )
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query!(
        "INSERT INTO workspace_members (workspace_id, user_id, role, joined_at)
         VALUES ($1, $2, $3, $4)",
        workspace.id,
        user_id,
        admin,
        now
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    tracing::info!(workspace_id = %workspace.id, %user_id, "workspace created");
    Ok(workspace)
}

pub async fn list_for_user(state: &AppState, user_id: &str) -> Result<Vec<Workspace>, AppError> {
    let workspaces = sqlx::query_as!(
        Workspace,
        r#"SELECT w.id AS "id!", w.name, w.created_by, w.created_at, w.updated_at
           FROM workspaces w
           JOIN workspace_members m ON m.workspace_id = w.id
           WHERE m.user_id = $1
           ORDER BY w.created_at, w.id"#,
        user_id
    )
    .fetch_all(&state.read_db)
    .await?;
    Ok(workspaces)
}

pub async fn rename(
    state: &AppState,
    workspace_id: &str,
    name: &str,
) -> Result<Workspace, AppError> {
    let now = now_ms();
    let workspace = sqlx::query_as!(
        Workspace,
        r#"UPDATE workspaces SET name = $1, updated_at = $2
           WHERE id = $3
           RETURNING id AS "id!", name AS "name!", created_by AS "created_by!",
                     created_at AS "created_at!", updated_at AS "updated_at!""#,
        name,
        now,
        workspace_id
    )
    .fetch_all(&state.db)
    .await
    .map(db::first_row)?
    .ok_or(AppError::NotFound)?;

    tracing::info!(%workspace_id, "workspace renamed");
    Ok(workspace)
}

pub async fn delete(state: &AppState, workspace_id: &str) -> Result<(), AppError> {
    Workspace::delete_archiving_boards(&state.db, workspace_id, now_ms()).await?;
    tracing::info!(%workspace_id, "workspace deleted, boards archived");
    Ok(())
}

pub async fn list_members(
    state: &AppState,
    workspace_id: &str,
) -> Result<Vec<MemberDetails>, AppError> {
    let members = sqlx::query_as!(
        MemberDetails,
        r#"SELECT u.id AS "user_id!", u.name, u.email, u.avatar_color,
                  m.role AS "role: WorkspaceRole", m.joined_at
           FROM workspace_members m
           JOIN users u ON u.id = m.user_id
           WHERE m.workspace_id = $1
           ORDER BY m.joined_at, u.id"#,
        workspace_id
    )
    .fetch_all(&state.read_db)
    .await?;
    Ok(members)
}

/// `email` must already be normalized and validated.
pub async fn invite(
    state: &AppState,
    workspace_id: &str,
    email: &str,
    role: WorkspaceRole,
) -> Result<MemberDetails, AppError> {
    let user = sqlx::query!(r#"SELECT id AS "id!" FROM users WHERE email = $1"#, email)
        .fetch_optional(&state.read_db)
        .await?
        .ok_or_else(|| AppError::BadRequest(USER_NOT_FOUND_MESSAGE.to_string()))?;

    let now = now_ms();
    let inserted = sqlx::query!(
        "INSERT INTO workspace_members (workspace_id, user_id, role, joined_at)
         VALUES ($1, $2, $3, $4)",
        workspace_id,
        user.id,
        role,
        now
    )
    .execute(&state.db)
    .await;
    match inserted {
        Ok(_) => {}
        Err(err) if is_unique_violation(&err) => {
            return Err(AppError::BadRequest(ALREADY_MEMBER_MESSAGE.to_string()));
        }
        // The workspace or user was deleted after the checks above.
        Err(err) if is_foreign_key_violation(&err) => return Err(AppError::NotFound),
        Err(err) => return Err(err.into()),
    }

    tracing::info!(%workspace_id, user_id = %user.id, ?role, "workspace member added");
    let member = member(state, workspace_id, &user.id).await?;
    for board_id in live_board_ids(state, workspace_id).await {
        state
            .ws_hub
            .broadcast_to_board_all(&board_id, &events::member_joined(&member, &board_id));
    }
    Ok(member)
}

/// Refuses to remove the workspace's only admin.
pub async fn remove_member(
    state: &AppState,
    workspace_id: &str,
    user_id: &str,
) -> Result<(), AppError> {
    let admin = WorkspaceRole::Admin;
    // The last-admin guard lives in the statement so it cannot race other writes.
    let deleted = sqlx::query!(
        "DELETE FROM workspace_members
         WHERE workspace_id = $1 AND user_id = $2
           AND (role != $3
                OR (SELECT COUNT(*) FROM workspace_members
                    WHERE workspace_id = $1 AND role = $3) > 1)",
        workspace_id,
        user_id,
        admin
    )
    .execute(&state.db)
    .await?;

    if deleted.rows_affected() == 0 {
        return Err(explain_unchanged_member(
            state,
            workspace_id,
            user_id,
            LAST_ADMIN_REMOVE_MESSAGE,
        )
        .await);
    }
    tracing::info!(%workspace_id, %user_id, "workspace member removed");
    for board_id in live_board_ids(state, workspace_id).await {
        // Detach first so the removed user stops receiving this board's events.
        state.ws_hub.remove_user_from_board(user_id, &board_id);
        state
            .ws_hub
            .broadcast_to_board_all(&board_id, &events::member_left(user_id, &board_id));
    }
    Ok(())
}

/// Boards whose members get real-time events. Best-effort: a failed lookup is
/// logged and yields no boards, so it never fails the membership change.
async fn live_board_ids(state: &AppState, workspace_id: &str) -> Vec<String> {
    let rows = sqlx::query!(
        r#"SELECT id AS "id!" FROM boards
           WHERE workspace_id = $1 AND archived = 0
           ORDER BY id"#,
        workspace_id
    )
    .fetch_all(&state.read_db)
    .await;

    match rows {
        Ok(rows) => rows.into_iter().map(|row| row.id).collect(),
        Err(err) => {
            tracing::error!(error = ?err, %workspace_id, "failed to load boards for ws broadcast");
            Vec::new()
        }
    }
}

/// Refuses to demote the workspace's only admin.
pub async fn change_role(
    state: &AppState,
    workspace_id: &str,
    user_id: &str,
    role: WorkspaceRole,
) -> Result<MemberDetails, AppError> {
    let admin = WorkspaceRole::Admin;
    // The last-admin guard lives in the statement so it cannot race other writes.
    let updated = sqlx::query!(
        "UPDATE workspace_members SET role = $3
         WHERE workspace_id = $1 AND user_id = $2
           AND ($3 = $4
                OR role != $4
                OR (SELECT COUNT(*) FROM workspace_members
                    WHERE workspace_id = $1 AND role = $4) > 1)",
        workspace_id,
        user_id,
        role,
        admin
    )
    .execute(&state.db)
    .await?;

    if updated.rows_affected() == 0 {
        return Err(explain_unchanged_member(
            state,
            workspace_id,
            user_id,
            LAST_ADMIN_DEMOTE_MESSAGE,
        )
        .await);
    }
    tracing::info!(%workspace_id, %user_id, ?role, "workspace member role changed");
    member(state, workspace_id, user_id).await
}

async fn member(
    state: &AppState,
    workspace_id: &str,
    user_id: &str,
) -> Result<MemberDetails, AppError> {
    let member = sqlx::query_as!(
        MemberDetails,
        r#"SELECT u.id AS "user_id!", u.name, u.email, u.avatar_color,
                  m.role AS "role: WorkspaceRole", m.joined_at
           FROM workspace_members m
           JOIN users u ON u.id = m.user_id
           WHERE m.workspace_id = $1 AND m.user_id = $2"#,
        workspace_id,
        user_id
    )
    .fetch_optional(&state.read_db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(member)
}

/// A guarded member write matched no row: either the target is not a member (404)
/// or the last-admin guard blocked it (400).
async fn explain_unchanged_member(
    state: &AppState,
    workspace_id: &str,
    user_id: &str,
    last_admin_message: &str,
) -> AppError {
    let exists = sqlx::query!(
        r#"SELECT user_id AS "user_id!" FROM workspace_members
           WHERE workspace_id = $1 AND user_id = $2"#,
        workspace_id,
        user_id
    )
    .fetch_optional(&state.read_db)
    .await;
    match exists {
        Ok(Some(_)) => {
            tracing::info!(%workspace_id, %user_id, "last admin guard blocked member change");
            AppError::BadRequest(last_admin_message.to_string())
        }
        Ok(None) => AppError::NotFound,
        Err(err) => err.into(),
    }
}
