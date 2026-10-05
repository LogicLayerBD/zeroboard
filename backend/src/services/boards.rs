use std::collections::HashMap;

use serde::Serialize;

use super::{is_foreign_key_violation, now_ms};
use crate::db;
use crate::errors::AppError;
use crate::models::{Board, Card, List};
use crate::AppState;

/// A card with just enough relations for board previews and list-view sorting.
#[derive(Debug, Serialize)]
pub struct BoardCard {
    #[serde(flatten)]
    pub card: Card,
    pub assignee_ids: Vec<String>,
    pub label_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ListWithCards {
    #[serde(flatten)]
    pub list: List,
    /// Ordered by position.
    pub cards: Vec<BoardCard>,
}

#[derive(Debug, Serialize)]
pub struct BoardDetails {
    #[serde(flatten)]
    pub board: Board,
    /// Ordered by position.
    pub lists: Vec<ListWithCards>,
}

pub async fn list_for_workspace(
    state: &AppState,
    workspace_id: &str,
) -> Result<Vec<Board>, AppError> {
    let boards = sqlx::query_as!(
        Board,
        r#"SELECT id AS "id!", workspace_id, name, archived AS "archived: bool",
                  created_by, created_at, updated_at
           FROM boards
           WHERE workspace_id = $1 AND archived = 0
           ORDER BY created_at, id"#,
        workspace_id
    )
    .fetch_all(&state.read_db)
    .await?;
    Ok(boards)
}

pub async fn create(
    state: &AppState,
    workspace_id: &str,
    user_id: &str,
    name: &str,
) -> Result<Board, AppError> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_ms();
    let inserted = sqlx::query_as!(
        Board,
        r#"INSERT INTO boards (id, workspace_id, name, created_by, created_at, updated_at)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING id AS "id!", workspace_id, name AS "name!", archived AS "archived!: bool",
                     created_by AS "created_by!", created_at AS "created_at!",
                     updated_at AS "updated_at!""#,
        id,
        workspace_id,
        name,
        user_id,
        now,
        now
    )
    .fetch_all(&state.db)
    .await
    .and_then(db::single_row);

    let board = match inserted {
        Ok(board) => board,
        // The workspace was deleted after the membership check.
        Err(err) if is_foreign_key_violation(&err) => return Err(AppError::NotFound),
        Err(err) => return Err(err.into()),
    };
    tracing::info!(board_id = %board.id, %workspace_id, %user_id, "board created");
    Ok(board)
}

/// Attaches the board's lists and their cards, both ordered by position.
pub async fn details(state: &AppState, board: Board) -> Result<BoardDetails, AppError> {
    let lists = sqlx::query_as!(
        List,
        r#"SELECT id AS "id!", board_id, name, position, created_at, updated_at
           FROM lists
           WHERE board_id = $1
           ORDER BY position, id"#,
        board.id
    )
    .fetch_all(&state.read_db)
    .await?;

    let cards = sqlx::query_as!(
        Card,
        r#"SELECT id AS "id!", list_id, board_id, title, description, position, due_date,
                  created_by, created_at, updated_at
           FROM cards
           WHERE board_id = $1
           ORDER BY position, id"#,
        board.id
    )
    .fetch_all(&state.read_db)
    .await?;

    let assignees = sqlx::query!(
        r#"SELECT a.card_id AS "card_id!", a.user_id AS "user_id!"
           FROM card_assignees a
           JOIN cards c ON c.id = a.card_id
           WHERE c.board_id = $1
           ORDER BY a.user_id"#,
        board.id
    )
    .fetch_all(&state.read_db)
    .await?;
    let mut assignees_by_card: HashMap<String, Vec<String>> = HashMap::new();
    for row in assignees {
        assignees_by_card.entry(row.card_id).or_default().push(row.user_id);
    }

    let labels = sqlx::query!(
        r#"SELECT cl.card_id AS "card_id!", cl.label_id AS "label_id!"
           FROM card_labels cl
           JOIN cards c ON c.id = cl.card_id
           WHERE c.board_id = $1
           ORDER BY cl.label_id"#,
        board.id
    )
    .fetch_all(&state.read_db)
    .await?;
    let mut labels_by_card: HashMap<String, Vec<String>> = HashMap::new();
    for row in labels {
        labels_by_card.entry(row.card_id).or_default().push(row.label_id);
    }

    let mut cards_by_list: HashMap<String, Vec<BoardCard>> = HashMap::new();
    for card in cards {
        let board_card = BoardCard {
            assignee_ids: assignees_by_card.remove(&card.id).unwrap_or_default(),
            label_ids: labels_by_card.remove(&card.id).unwrap_or_default(),
            card,
        };
        cards_by_list
            .entry(board_card.card.list_id.clone())
            .or_default()
            .push(board_card);
    }
    let lists = lists
        .into_iter()
        .map(|list| ListWithCards {
            cards: cards_by_list.remove(&list.id).unwrap_or_default(),
            list,
        })
        .collect();

    Ok(BoardDetails { board, lists })
}

pub async fn rename(state: &AppState, board_id: &str, name: &str) -> Result<Board, AppError> {
    let now = now_ms();
    let board = sqlx::query_as!(
        Board,
        r#"UPDATE boards SET name = $1, updated_at = $2
           WHERE id = $3 AND archived = 0
           RETURNING id AS "id!", workspace_id, name AS "name!", archived AS "archived!: bool",
                     created_by AS "created_by!", created_at AS "created_at!",
                     updated_at AS "updated_at!""#,
        name,
        now,
        board_id
    )
    .fetch_all(&state.db)
    .await
    .map(db::first_row)?
    .ok_or(AppError::NotFound)?;

    tracing::info!(%board_id, "board renamed");
    Ok(board)
}

/// Soft delete: lists and cards are kept.
pub async fn archive(state: &AppState, board_id: &str) -> Result<(), AppError> {
    let now = now_ms();
    let archived = sqlx::query!(
        "UPDATE boards SET archived = 1, updated_at = $1 WHERE id = $2 AND archived = 0",
        now,
        board_id
    )
    .execute(&state.db)
    .await?;
    if archived.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    tracing::info!(%board_id, "board archived");
    Ok(())
}
