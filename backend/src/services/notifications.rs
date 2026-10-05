//! In-app notifications. Creation is best-effort: failures are logged, never
//! returned, so a notification problem never fails the action that caused it.

use serde_json::json;

use super::now_ms;
use crate::errors::AppError;
use crate::models::{Card, Notification};
use crate::ws::events;
use crate::AppState;

pub const ASSIGNED_TO_CARD: &str = "assigned_to_card";
pub const CARD_DUE_SOON: &str = "card_due_soon";
pub const COMMENT_ON_ASSIGNED_CARD: &str = "comment_on_assigned_card";

/// Most recent notifications returned by the list endpoint.
const LIST_LIMIT: i64 = 50;
/// A card is "due soon" when its due date falls within this window from now.
pub const DUE_SOON_WINDOW_MS: i64 = 24 * 60 * 60 * 1000;

pub async fn list_for_user(state: &AppState, user_id: &str) -> Result<Vec<Notification>, AppError> {
    let notifications = sqlx::query_as!(
        Notification,
        r#"SELECT id AS "id!", user_id, type AS kind, payload, read AS "read: bool", created_at
           FROM notifications
           WHERE user_id = $1
           ORDER BY created_at DESC, rowid DESC
           LIMIT $2"#,
        user_id,
        LIST_LIMIT
    )
    .fetch_all(&state.read_db)
    .await?;
    Ok(notifications)
}

/// Another user's notification is reported as missing, not forbidden.
pub async fn mark_read(
    state: &AppState,
    user_id: &str,
    notification_id: &str,
) -> Result<Notification, AppError> {
    let notification = sqlx::query_as!(
        Notification,
        r#"UPDATE notifications SET read = 1
           WHERE id = $1 AND user_id = $2
           RETURNING id AS "id!", user_id AS "user_id!", type AS "kind!", payload AS "payload!",
                     read AS "read!: bool", created_at AS "created_at!""#,
        notification_id,
        user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(notification)
}

/// Returns how many notifications changed from unread to read.
pub async fn mark_all_read(state: &AppState, user_id: &str) -> Result<u64, AppError> {
    let updated = sqlx::query!(
        "UPDATE notifications SET read = 1 WHERE user_id = $1 AND read = 0",
        user_id
    )
    .execute(&state.db)
    .await?;
    tracing::info!(%user_id, updated = updated.rows_affected(), "notifications marked read");
    Ok(updated.rows_affected())
}

/// Notifies `assignee_id` that `actor_id` assigned them to `card`.
pub async fn notify_assigned(state: &AppState, actor_id: &str, card: &Card, assignee_id: &str) {
    if assignee_id == actor_id {
        return;
    }
    let message = format!("You were assigned to \"{}\"", card.title);
    create(state, assignee_id, ASSIGNED_TO_CARD, card, &message).await;
}

/// Notifies the card's assignees (except `author_id`) about a new comment.
pub async fn notify_comment(state: &AppState, author_id: &str, card: &Card) {
    let message = format!("New comment on \"{}\"", card.title);
    for user_id in assignees_except(state, &card.id, author_id).await {
        create(state, &user_id, COMMENT_ON_ASSIGNED_CARD, card, &message).await;
    }
}

/// If `card` is due within the next 24 hours, notifies its assignees (except
/// `actor_id`) who have not already been notified for the current due window.
pub async fn notify_if_due_soon(state: &AppState, actor_id: &str, card: &Card) {
    let Some(due_date) = card.due_date else {
        return;
    };
    let now = now_ms();
    if due_date <= now || due_date > now + DUE_SOON_WINDOW_MS {
        return;
    }

    let window_start = due_date - DUE_SOON_WINDOW_MS;
    let recipients = sqlx::query!(
        r#"SELECT a.user_id AS "user_id!"
           FROM card_assignees a
           WHERE a.card_id = $1
             AND a.user_id != $2
             AND NOT EXISTS (
               SELECT 1 FROM notifications n
               WHERE n.user_id = a.user_id
                 AND n.type = $3
                 AND json_extract(n.payload, '$.card_id') = $1
                 AND n.created_at >= $4
             )
           ORDER BY a.user_id"#,
        card.id,
        actor_id,
        CARD_DUE_SOON,
        window_start
    )
    .fetch_all(&state.read_db)
    .await;

    let recipients = match recipients {
        Ok(rows) => rows,
        Err(err) => {
            tracing::error!(error = ?err, card_id = %card.id, "failed to load due-soon recipients");
            return;
        }
    };
    let message = format!("\"{}\" is due within 24 hours", card.title);
    for row in recipients {
        create(state, &row.user_id, CARD_DUE_SOON, card, &message).await;
    }
}

async fn assignees_except(state: &AppState, card_id: &str, excluded_user_id: &str) -> Vec<String> {
    let rows = sqlx::query!(
        r#"SELECT user_id AS "user_id!"
           FROM card_assignees
           WHERE card_id = $1 AND user_id != $2
           ORDER BY user_id"#,
        card_id,
        excluded_user_id
    )
    .fetch_all(&state.read_db)
    .await;

    match rows {
        Ok(rows) => rows.into_iter().map(|row| row.user_id).collect(),
        Err(err) => {
            tracing::error!(error = ?err, %card_id, "failed to load card assignees");
            Vec::new()
        }
    }
}

/// Uses the write pool, so never call this while holding an open write transaction.
async fn create(state: &AppState, user_id: &str, kind: &str, card: &Card, message: &str) {
    let id = uuid::Uuid::new_v4().to_string();
    let payload = json!({
        "card_id": card.id,
        "board_id": card.board_id,
        "message": message,
    })
    .to_string();
    let now = now_ms();
    let inserted = sqlx::query!(
        "INSERT INTO notifications (id, user_id, type, payload, read, created_at)
         VALUES ($1, $2, $3, $4, 0, $5)",
        id,
        user_id,
        kind,
        payload,
        now
    )
    .execute(&state.db)
    .await;

    match inserted {
        Ok(_) => {
            tracing::info!(notification_id = %id, %user_id, %kind, card_id = %card.id, "notification created");
            let notification = Notification {
                id,
                user_id: user_id.to_string(),
                kind: kind.to_string(),
                payload,
                read: false,
                created_at: now,
            };
            state
                .ws_hub
                .send_to_user(user_id, &events::notification(&notification));
        }
        Err(err) => tracing::error!(
            error = ?err,
            %user_id,
            %kind,
            card_id = %card.id,
            "failed to create notification"
        ),
    }
}
