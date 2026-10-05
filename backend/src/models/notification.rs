use serde::Serialize;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Notification {
    pub id: String,
    pub user_id: String,
    #[sqlx(rename = "type")]
    #[serde(rename = "type")]
    pub kind: String,
    /// JSON-encoded context.
    pub payload: String,
    pub read: bool,
    pub created_at: i64,
}
