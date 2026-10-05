use serde::Serialize;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Card {
    pub id: String,
    pub list_id: String,
    pub board_id: String,
    pub title: String,
    pub description: Option<String>,
    /// Fractional index for ordering within the list.
    pub position: f64,
    pub due_date: Option<i64>,
    pub created_by: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct CardAssignee {
    pub card_id: String,
    pub user_id: String,
}
