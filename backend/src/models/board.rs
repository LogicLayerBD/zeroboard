use serde::Serialize;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Board {
    pub id: String,
    /// `None` once the owning workspace has been deleted (board is archived first).
    pub workspace_id: Option<String>,
    pub name: String,
    pub archived: bool,
    pub created_by: String,
    pub created_at: i64,
    pub updated_at: i64,
}
