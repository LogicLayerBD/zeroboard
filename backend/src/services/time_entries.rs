use serde::Serialize;

use super::{is_foreign_key_violation, now_ms};
use crate::db;
use crate::errors::AppError;
use crate::models::TimeEntry;
use crate::AppState;

#[derive(Debug, Serialize)]
pub struct CardTimeEntries {
    pub time_entries: Vec<TimeEntry>,
    pub total_minutes: i64,
}

#[derive(Debug, Serialize)]
pub struct LoggedTimeEntry {
    pub time_entry: TimeEntry,
    pub total_minutes: i64,
}

pub async fn list_for_card(state: &AppState, card_id: &str) -> Result<CardTimeEntries, AppError> {
    let time_entries = sqlx::query_as!(
        TimeEntry,
        r#"SELECT id AS "id!", card_id, user_id, minutes, description, logged_at
           FROM time_entries
           WHERE card_id = $1
           ORDER BY logged_at, rowid"#,
        card_id
    )
    .fetch_all(&state.read_db)
    .await?;
    let total_minutes = time_entries.iter().map(|entry| entry.minutes).sum();
    Ok(CardTimeEntries {
        time_entries,
        total_minutes,
    })
}

pub async fn get(state: &AppState, entry_id: &str) -> Result<TimeEntry, AppError> {
    let entry = sqlx::query_as!(
        TimeEntry,
        r#"SELECT id AS "id!", card_id, user_id, minutes, description, logged_at
           FROM time_entries
           WHERE id = $1"#,
        entry_id
    )
    .fetch_optional(&state.read_db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(entry)
}

pub async fn create(
    state: &AppState,
    card_id: &str,
    user_id: &str,
    minutes: i64,
    description: Option<String>,
) -> Result<LoggedTimeEntry, AppError> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_ms();
    let inserted = sqlx::query_as!(
        TimeEntry,
        r#"INSERT INTO time_entries (id, card_id, user_id, minutes, description, logged_at)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING id AS "id!", card_id AS "card_id!", user_id AS "user_id!",
                     minutes AS "minutes!", description, logged_at AS "logged_at!""#,
        id,
        card_id,
        user_id,
        minutes,
        description,
        now
    )
    .fetch_all(&state.db)
    .await
    .and_then(db::single_row);

    let time_entry = match inserted {
        Ok(entry) => entry,
        // The card was deleted after the access check.
        Err(err) if is_foreign_key_violation(&err) => return Err(AppError::NotFound),
        Err(err) => return Err(err.into()),
    };
    tracing::info!(time_entry_id = %time_entry.id, %card_id, %user_id, minutes, "time entry logged");

    // Read on the write pool so the total includes the entry just inserted.
    let total_minutes = sqlx::query!(
        r#"SELECT COALESCE(SUM(minutes), 0) AS "total!: i64" FROM time_entries WHERE card_id = $1"#,
        card_id
    )
    .fetch_one(&state.db)
    .await?
    .total;
    Ok(LoggedTimeEntry {
        time_entry,
        total_minutes,
    })
}

pub async fn delete(state: &AppState, entry_id: &str) -> Result<(), AppError> {
    let deleted = sqlx::query!("DELETE FROM time_entries WHERE id = $1", entry_id)
        .execute(&state.db)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    tracing::info!(time_entry_id = %entry_id, "time entry deleted");
    Ok(())
}
