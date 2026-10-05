//! Card history. Writes are best-effort: failures are logged, never returned.

use super::now_ms;
use crate::AppState;

pub const CREATED_CARD: &str = "created_card";
pub const MOVED_CARD: &str = "moved_card";
pub const ADDED_ASSIGNEE: &str = "added_assignee";
pub const REMOVED_ASSIGNEE: &str = "removed_assignee";
pub const ADDED_LABEL: &str = "added_label";
pub const REMOVED_LABEL: &str = "removed_label";

/// Uses the write pool, so never call this while holding an open write transaction.
pub async fn record(
    state: &AppState,
    card_id: &str,
    board_id: &str,
    user_id: &str,
    action: &str,
    payload: serde_json::Value,
) {
    let id = uuid::Uuid::new_v4().to_string();
    let payload = payload.to_string();
    let now = now_ms();
    let inserted = sqlx::query!(
        "INSERT INTO activity_log (id, card_id, board_id, user_id, action, payload, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
        id,
        card_id,
        board_id,
        user_id,
        action,
        payload,
        now
    )
    .execute(&state.db)
    .await;

    if let Err(err) = inserted {
        tracing::error!(
            error = ?err,
            %card_id,
            %board_id,
            %user_id,
            %action,
            "failed to record activity"
        );
    }
}
