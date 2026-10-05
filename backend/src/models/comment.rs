use serde::Serialize;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Comment {
    pub id: String,
    pub card_id: String,
    pub user_id: String,
    pub body: String,
    pub created_at: i64,
    pub updated_at: i64,
}
