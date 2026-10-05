use serde::Serialize;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ActivityLog {
    pub id: String,
    /// `None` once the card is deleted; history is kept.
    pub card_id: Option<String>,
    pub board_id: Option<String>,
    pub user_id: String,
    /// e.g. "moved_card", "added_attachment".
    pub action: String,
    /// JSON-encoded context.
    pub payload: Option<String>,
    pub created_at: i64,
}
