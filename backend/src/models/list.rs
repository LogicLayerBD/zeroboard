use serde::Serialize;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct List {
    pub id: String,
    pub board_id: String,
    pub name: String,
    /// Fractional index for ordering within the board.
    pub position: f64,
    pub created_at: i64,
    pub updated_at: i64,
}
