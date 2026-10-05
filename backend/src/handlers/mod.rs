pub mod admin;
pub mod attachments;
pub mod auth;
pub mod boards;
pub mod cards;
pub mod comments;
pub mod labels;
pub mod lists;
pub mod notifications;
pub mod time_entries;
pub mod workspaces;

#[cfg(test)]
pub(crate) mod test_support;

use crate::errors::AppError;

/// Shared limit for workspace, board and list names (see security rules).
const MAX_NAME_CHARS: usize = 255;

/// Trims a required name and enforces the 1..=255 character limit.
pub(crate) fn validate_name(raw: &str) -> Result<String, AppError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest("name is required".into()));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(AppError::BadRequest(format!(
            "name must be at most {MAX_NAME_CHARS} characters"
        )));
    }
    Ok(name.to_string())
}

/// Path IDs must be UUIDs; returns the canonical lowercase hyphenated form used in the DB.
pub(crate) fn parse_id(raw: &str, field: &str) -> Result<String, AppError> {
    uuid::Uuid::parse_str(raw)
        .map(|id| id.to_string())
        .map_err(|_| AppError::BadRequest(format!("{field} is invalid")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_name_trims_and_enforces_length() {
        assert_eq!(validate_name("  Roadmap  ").unwrap(), "Roadmap");
        assert!(validate_name("").is_err());
        assert!(validate_name("   ").is_err());
        assert!(validate_name(&"é".repeat(MAX_NAME_CHARS)).is_ok());
        assert!(validate_name(&"a".repeat(MAX_NAME_CHARS + 1)).is_err());
    }

    #[test]
    fn parse_id_accepts_only_uuids_and_canonicalizes() {
        let id = "8F14E45F-CEEA-467A-9575-0D9C2C7B6A51";
        assert_eq!(parse_id(id, "id").unwrap(), id.to_lowercase());
        assert!(parse_id("not-a-uuid", "id").is_err());
        assert!(parse_id("1 OR 1=1", "id").is_err());
    }
}
