use serde::Serialize;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Attachment {
    pub id: String,
    pub card_id: String,
    pub filename: String,
    /// Server-side storage path; not exposed to clients.
    #[serde(skip_serializing)]
    pub stored_path: String,
    pub size_bytes: i64,
    pub uploaded_by: String,
    pub uploaded_at: i64,
}
