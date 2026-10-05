use std::fmt;

/// Server-only row; intentionally not `Serialize`.
#[derive(Clone, sqlx::FromRow)]
pub struct RefreshToken {
    pub id: String,
    pub user_id: String,
    pub token_hash: String,
    pub expires_at: i64,
    pub created_at: i64,
}

impl fmt::Debug for RefreshToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RefreshToken")
            .field("id", &self.id)
            .field("user_id", &self.user_id)
            .field("token_hash", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("created_at", &self.created_at)
            .finish()
    }
}
