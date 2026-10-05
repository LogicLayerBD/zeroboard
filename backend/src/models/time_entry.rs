use serde::Serialize;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct TimeEntry {
    pub id: String,
    pub card_id: String,
    pub user_id: String,
    pub minutes: i64,
    pub description: Option<String>,
    pub logged_at: i64,
}
