use std::fmt;

use serde::Serialize;

#[derive(Clone, Serialize, sqlx::FromRow)]
pub struct User {
    pub id: String,
    pub email: String,
    pub name: String,
    /// bcrypt hash; never serialized or logged.
    #[serde(skip_serializing)]
    pub password: String,
    pub avatar_color: String,
    pub created_at: i64,
    pub updated_at: i64,
}

impl fmt::Debug for User {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("User")
            .field("id", &self.id)
            .field("email", &self.email)
            .field("name", &self.name)
            .field("password", &"<redacted>")
            .field("avatar_color", &self.avatar_color)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "$2b$12$secretbcrypthashvalue";

    fn user() -> User {
        User {
            id: "u1".into(),
            email: "a@b.c".into(),
            name: "A".into(),
            password: HASH.into(),
            avatar_color: "#6366f1".into(),
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn password_hash_is_never_serialized_or_debug_printed() {
        let json = serde_json::to_value(user()).unwrap();
        assert!(json.get("password").is_none());
        assert_eq!(json["email"], "a@b.c");
        assert!(!format!("{:?}", user()).contains(HASH));
    }
}
