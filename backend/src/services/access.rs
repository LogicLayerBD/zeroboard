//! Workspace membership checks. A missing resource is 404; an existing one the
//! caller may not touch is 403.

use crate::errors::AppError;
use crate::models::{Board, Card, Label, List, WorkspaceRole};
use crate::AppState;

/// Requires the workspace to exist and `user_id` to hold at least `required` in it.
pub async fn require_workspace_role(
    state: &AppState,
    workspace_id: &str,
    user_id: &str,
    required: WorkspaceRole,
) -> Result<WorkspaceRole, AppError> {
    let row = sqlx::query!(
        r#"SELECT m.role AS "role?: WorkspaceRole"
           FROM workspaces w
           LEFT JOIN workspace_members m ON m.workspace_id = w.id AND m.user_id = $2
           WHERE w.id = $1"#,
        workspace_id,
        user_id
    )
    .fetch_optional(&state.read_db)
    .await?
    .ok_or(AppError::NotFound)?;

    ensure_role(row.role, required, workspace_id, user_id)
}

/// Loads a live board (not archived, still in a workspace) and requires `user_id`
/// to hold at least `required` in its workspace.
pub async fn require_board_role(
    state: &AppState,
    board_id: &str,
    user_id: &str,
    required: WorkspaceRole,
) -> Result<Board, AppError> {
    let board = sqlx::query_as!(
        Board,
        r#"SELECT id AS "id!", workspace_id, name, archived AS "archived: bool",
                  created_by, created_at, updated_at
           FROM boards
           WHERE id = $1 AND archived = 0 AND workspace_id IS NOT NULL"#,
        board_id
    )
    .fetch_optional(&state.read_db)
    .await?
    .ok_or(AppError::NotFound)?;
    let workspace_id = board.workspace_id.as_deref().ok_or(AppError::NotFound)?;

    let role = sqlx::query!(
        r#"SELECT role AS "role: WorkspaceRole"
           FROM workspace_members
           WHERE workspace_id = $1 AND user_id = $2"#,
        workspace_id,
        user_id
    )
    .fetch_optional(&state.read_db)
    .await?
    .map(|row| row.role);

    ensure_role(role, required, workspace_id, user_id)?;
    Ok(board)
}

/// Loads a list on a live board and requires `user_id` to hold at least `required` there.
pub async fn require_list_role(
    state: &AppState,
    list_id: &str,
    user_id: &str,
    required: WorkspaceRole,
) -> Result<List, AppError> {
    let list = sqlx::query_as!(
        List,
        r#"SELECT id AS "id!", board_id, name, position, created_at, updated_at
           FROM lists
           WHERE id = $1"#,
        list_id
    )
    .fetch_optional(&state.read_db)
    .await?
    .ok_or(AppError::NotFound)?;

    require_board_role(state, &list.board_id, user_id, required).await?;
    Ok(list)
}

/// Loads a card on a live board and requires `user_id` to hold at least `required` there.
pub async fn require_card_role(
    state: &AppState,
    card_id: &str,
    user_id: &str,
    required: WorkspaceRole,
) -> Result<Card, AppError> {
    let card = sqlx::query_as!(
        Card,
        r#"SELECT id AS "id!", list_id, board_id, title, description, position, due_date,
                  created_by, created_at, updated_at
           FROM cards
           WHERE id = $1"#,
        card_id
    )
    .fetch_optional(&state.read_db)
    .await?
    .ok_or(AppError::NotFound)?;

    require_board_role(state, &card.board_id, user_id, required).await?;
    Ok(card)
}

/// Loads a label on a live board and requires `user_id` to hold at least `required` there.
pub async fn require_label_role(
    state: &AppState,
    label_id: &str,
    user_id: &str,
    required: WorkspaceRole,
) -> Result<Label, AppError> {
    let label = sqlx::query_as!(
        Label,
        r#"SELECT id AS "id!", board_id, name, color
           FROM labels
           WHERE id = $1"#,
        label_id
    )
    .fetch_optional(&state.read_db)
    .await?
    .ok_or(AppError::NotFound)?;

    require_board_role(state, &label.board_id, user_id, required).await?;
    Ok(label)
}

fn ensure_role(
    actual: Option<WorkspaceRole>,
    required: WorkspaceRole,
    workspace_id: &str,
    user_id: &str,
) -> Result<WorkspaceRole, AppError> {
    match actual {
        Some(role) if satisfies(role, required) => Ok(role),
        _ => {
            tracing::warn!(
                %workspace_id,
                %user_id,
                actual = ?actual,
                required = ?required,
                "workspace access denied"
            );
            Err(AppError::Forbidden)
        }
    }
}

/// Roles are hierarchical: admin ⊇ member ⊇ viewer.
fn satisfies(actual: WorkspaceRole, required: WorkspaceRole) -> bool {
    rank(actual) >= rank(required)
}

fn rank(role: WorkspaceRole) -> u8 {
    match role {
        WorkspaceRole::Viewer => 0,
        WorkspaceRole::Member => 1,
        WorkspaceRole::Admin => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use WorkspaceRole::{Admin, Member, Viewer};

    #[test]
    fn role_hierarchy() {
        for (actual, required, allowed) in [
            (Admin, Admin, true),
            (Admin, Member, true),
            (Admin, Viewer, true),
            (Member, Admin, false),
            (Member, Member, true),
            (Member, Viewer, true),
            (Viewer, Admin, false),
            (Viewer, Member, false),
            (Viewer, Viewer, true),
        ] {
            assert_eq!(
                satisfies(actual, required),
                allowed,
                "{actual:?} vs {required:?}"
            );
        }
    }

    #[test]
    fn non_member_is_forbidden() {
        assert!(matches!(
            ensure_role(None, Viewer, "w", "u"),
            Err(AppError::Forbidden)
        ));
    }
}
