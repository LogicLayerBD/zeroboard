//! Business logic for workspace-scoped resources. Inputs are validated by handlers.

pub mod access;
pub mod boards;
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
