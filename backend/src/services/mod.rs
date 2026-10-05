//! Business logic for workspace-scoped resources. Inputs are validated by handlers.

pub mod access;
pub mod activity;
pub mod admin;
pub mod attachments;
pub mod boards;
pub mod cards;
pub mod comments;
pub mod labels;
pub mod lists;
pub mod notifications;
pub mod positions;
pub mod time_entries;
pub mod workspaces;

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn is_unique_violation(err: &sqlx::Error) -> bool {
    err.as_database_error()
        .is_some_and(|db_err| db_err.is_unique_violation())
}

fn is_foreign_key_violation(err: &sqlx::Error) -> bool {
    err.as_database_error()
        .is_some_and(|db_err| db_err.is_foreign_key_violation())
}
