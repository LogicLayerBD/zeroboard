use super::positions::{reposition, slot_after, Sibling, POSITION_STEP};
use super::{cards, is_foreign_key_violation, now_ms};
use crate::errors::AppError;
use crate::models::List;
use crate::AppState;

const AFTER_ID_MESSAGE: &str = "after_id must be another list on this board";

pub async fn list_for_board(state: &AppState, board_id: &str) -> Result<Vec<List>, AppError> {
    let lists = sqlx::query_as!(
        List,
        r#"SELECT id AS "id!", board_id, name, position, created_at, updated_at
           FROM lists
           WHERE board_id = $1
           ORDER BY position, id"#,
        board_id
    )
    .fetch_all(&state.read_db)
    .await?;
    Ok(lists)
}

/// Appends the list after the board's last list.
pub async fn create(state: &AppState, board_id: &str, name: &str) -> Result<List, AppError> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_ms();
    let inserted = sqlx::query_as!(
        List,
        r#"INSERT INTO lists (id, board_id, name, position, created_at, updated_at)
           VALUES ($1, $2, $3,
                   COALESCE((SELECT MAX(position) FROM lists WHERE board_id = $2), 0.0) + $4,
                   $5, $5)
           RETURNING id AS "id!", board_id AS "board_id!", name AS "name!",
                     position AS "position!", created_at AS "created_at!",
                     updated_at AS "updated_at!""#,
        id,
        board_id,
        name,
        POSITION_STEP,
        now
    )
    .fetch_one(&state.db)
    .await;

    let list = match inserted {
        Ok(list) => list,
        Err(err) if is_foreign_key_violation(&err) => return Err(AppError::NotFound),
        Err(err) => return Err(err.into()),
    };
    tracing::info!(list_id = %list.id, %board_id, "list created");
    Ok(list)
}

pub async fn rename(state: &AppState, list_id: &str, name: &str) -> Result<List, AppError> {
    let now = now_ms();
    let list = sqlx::query_as!(
        List,
        r#"UPDATE lists SET name = $1, updated_at = $2
           WHERE id = $3
           RETURNING id AS "id!", board_id AS "board_id!", name AS "name!",
                     position AS "position!", created_at AS "created_at!",
                     updated_at AS "updated_at!""#,
        name,
        now,
        list_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    tracing::info!(%list_id, "list renamed");
    Ok(list)
}

/// Deletes the list; its cards and their dependent rows cascade. Attachment files
/// are removed after the commit.
pub async fn delete(state: &AppState, list_id: &str) -> Result<(), AppError> {
    let mut tx = state.db.begin().await?;
    let card_ids = sqlx::query!(
        r#"SELECT id AS "id!" FROM cards WHERE list_id = $1 ORDER BY id"#,
        list_id
    )
    .fetch_all(&mut *tx)
    .await?;
    let deleted = sqlx::query!("DELETE FROM lists WHERE id = $1", list_id)
        .execute(&mut *tx)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    tx.commit().await?;

    tracing::info!(%list_id, cards = card_ids.len(), "list deleted");
    for row in card_ids {
        cards::remove_attachment_files(state, &row.id).await;
    }
    Ok(())
}

/// Moves the list directly after `after_id` (or first when `None`) on its board.
pub async fn reorder(
    state: &AppState,
    list: &List,
    after_id: Option<&str>,
) -> Result<List, AppError> {
    let mut tx = state.db.begin().await?;
    let siblings = sqlx::query_as!(
        Sibling,
        r#"SELECT id AS "id!", position
           FROM lists
           WHERE board_id = $1 AND id != $2
           ORDER BY position, id"#,
        list.board_id,
        list.id
    )
    .fetch_all(&mut *tx)
    .await?;
    let slot = slot_after(&siblings, after_id)
        .ok_or_else(|| AppError::BadRequest(AFTER_ID_MESSAGE.to_string()))?;
    let plan = reposition(&siblings, slot);

    for (id, position) in &plan.rebalanced {
        sqlx::query!("UPDATE lists SET position = $1 WHERE id = $2", position, id)
            .execute(&mut *tx)
            .await?;
    }
    let now = now_ms();
    let moved = sqlx::query_as!(
        List,
        r#"UPDATE lists SET position = $1, updated_at = $2
           WHERE id = $3
           RETURNING id AS "id!", board_id AS "board_id!", name AS "name!",
                     position AS "position!", created_at AS "created_at!",
                     updated_at AS "updated_at!""#,
        plan.position,
        now,
        list.id
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;
    tx.commit().await?;

    tracing::info!(
        list_id = %moved.id,
        position = moved.position,
        rebalanced = !plan.rebalanced.is_empty(),
        "list reordered"
    );
    Ok(moved)
}
