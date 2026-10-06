//! Instance settings that admins can change at runtime. Env vars provide the
//! defaults until a setting is first changed from the admin panel.

use serde::Serialize;

use super::now_ms;
use crate::errors::AppError;
use crate::AppState;

/// `instance_settings` holds at most this one row.
const SETTINGS_ROW_ID: i64 = 1;

#[derive(Debug, Serialize)]
pub struct InstanceSettings {
    /// Whether anyone may register. The first account can always register.
    pub registration_enabled: bool,
}

pub async fn get(state: &AppState) -> Result<InstanceSettings, AppError> {
    Ok(InstanceSettings {
        registration_enabled: registration_enabled(state).await?,
    })
}

/// The admin's choice if one was saved, otherwise `REGISTRATION_ENABLED`.
pub async fn registration_enabled(state: &AppState) -> Result<bool, AppError> {
    let saved = sqlx::query_scalar!(
        r#"SELECT registration_enabled AS "registration_enabled?: bool"
           FROM instance_settings WHERE id = $1"#,
        SETTINGS_ROW_ID
    )
    .fetch_optional(&state.read_db)
    .await?
    .flatten();
    Ok(saved.unwrap_or(state.config.registration_enabled))
}

pub async fn set_registration_enabled(
    state: &AppState,
    actor_id: &str,
    enabled: bool,
) -> Result<InstanceSettings, AppError> {
    let now = now_ms();
    sqlx::query!(
        "INSERT INTO instance_settings (id, registration_enabled, updated_at) VALUES ($1, $2, $3)
         ON CONFLICT (id) DO UPDATE SET registration_enabled = excluded.registration_enabled,
                                        updated_at = excluded.updated_at",
        SETTINGS_ROW_ID,
        enabled,
        now
    )
    .execute(&state.db)
    .await?;
    tracing::info!(%actor_id, registration_enabled = enabled, "instance setting changed");
    get(state).await
}
