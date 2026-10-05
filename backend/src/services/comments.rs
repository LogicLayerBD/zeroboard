use serde_json::json;

use super::{activity, is_foreign_key_violation, notifications, now_ms};
use crate::errors::AppError;
use crate::models::{Card, Comment};
use crate::AppState;

/// Oldest first; `rowid` keeps insertion order for comments created in the same millisecond.
pub async fn list_for_card(state: &AppState, card_id: &str) -> Result<Vec<Comment>, AppError> {
    let comments = sqlx::query_as!(
        Comment,
        r#"SELECT id AS "id!", card_id, user_id, body, created_at, updated_at
           FROM comments
           WHERE card_id = $1
           ORDER BY created_at, rowid"#,
        card_id
    )
    .fetch_all(&state.read_db)
    .await?;
    Ok(comments)
}

pub async fn get(state: &AppState, comment_id: &str) -> Result<Comment, AppError> {
    let comment = sqlx::query_as!(
        Comment,
        r#"SELECT id AS "id!", card_id, user_id, body, created_at, updated_at
           FROM comments
           WHERE id = $1"#,
        comment_id
    )
    .fetch_optional(&state.read_db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(comment)
}

/// Logs `added_comment` and notifies the card's other assignees.
pub async fn create(
    state: &AppState,
    card: &Card,
    user_id: &str,
    body: &str,
) -> Result<Comment, AppError> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_ms();
    let inserted = sqlx::query_as!(
        Comment,
        r#"INSERT INTO comments (id, card_id, user_id, body, created_at, updated_at)
           VALUES ($1, $2, $3, $4, $5, $5)
           RETURNING id AS "id!", card_id AS "card_id!", user_id AS "user_id!", body AS "body!",
                     created_at AS "created_at!", updated_at AS "updated_at!""#,
        id,
        card.id,
        user_id,
        body,
        now
    )
    .fetch_one(&state.db)
    .await;

    let comment = match inserted {
        Ok(comment) => comment,
        // The card was deleted after the access check.
        Err(err) if is_foreign_key_violation(&err) => return Err(AppError::NotFound),
        Err(err) => return Err(err.into()),
    };
    tracing::info!(comment_id = %comment.id, card_id = %card.id, %user_id, "comment created");
    activity::record(
        state,
        &card.id,
        &card.board_id,
        user_id,
        activity::ADDED_COMMENT,
        json!({ "comment_id": comment.id }),
    )
    .await;
    notifications::notify_comment(state, user_id, card).await;
    Ok(comment)
}

pub async fn update(state: &AppState, comment_id: &str, body: &str) -> Result<Comment, AppError> {
    let now = now_ms();
    let comment = sqlx::query_as!(
        Comment,
        r#"UPDATE comments SET body = $1, updated_at = $2
           WHERE id = $3
           RETURNING id AS "id!", card_id AS "card_id!", user_id AS "user_id!", body AS "body!",
                     created_at AS "created_at!", updated_at AS "updated_at!""#,
        body,
        now,
        comment_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    tracing::info!(%comment_id, "comment updated");
    Ok(comment)
}

pub async fn delete(state: &AppState, comment_id: &str) -> Result<(), AppError> {
    let deleted = sqlx::query!("DELETE FROM comments WHERE id = $1", comment_id)
        .execute(&state.db)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    tracing::info!(%comment_id, "comment deleted");
    Ok(())
}
