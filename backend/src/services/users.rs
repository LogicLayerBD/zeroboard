//! Instance-wide user administration. Handlers enforce instance-admin access;
//! these functions guard against admins locking themselves out.

use serde::Serialize;

use super::now_ms;
use crate::auth::service as auth_service;
use crate::errors::AppError;
use crate::models::UserRole;
use crate::AppState;

const SELF_ROLE_MESSAGE: &str = "you cannot change your own instance role";
const SELF_DEACTIVATE_MESSAGE: &str = "you cannot deactivate your own account";
const SELF_RESET_MESSAGE: &str = "use Change password to update your own password";

#[derive(Debug, Serialize)]
pub struct AdminUser {
    pub id: String,
    pub email: String,
    pub name: String,
    pub avatar_color: String,
    pub role: UserRole,
    pub created_at: i64,
    /// `None` while the account is active.
    pub deactivated_at: Option<i64>,
    pub workspace_count: i64,
}

/// Deliberately not `Debug`: the temporary password must never reach logs.
#[derive(Serialize)]
pub struct CreatedUser {
    pub user: AdminUser,
    /// Shown to the admin once; only its bcrypt hash is stored.
    pub temporary_password: String,
}

/// Deliberately not `Debug`: the temporary password must never reach logs.
#[derive(Serialize)]
pub struct PasswordReset {
    pub temporary_password: String,
}

pub async fn list(state: &AppState) -> Result<Vec<AdminUser>, AppError> {
    let users = sqlx::query_as!(
        AdminUser,
        r#"SELECT u.id AS "id!", u.email, u.name, u.avatar_color, u.role AS "role: UserRole",
                  u.created_at, u.deactivated_at AS "deactivated_at?",
                  (SELECT COUNT(*) FROM workspace_members m WHERE m.user_id = u.id)
                      AS "workspace_count!: i64"
           FROM users u
           ORDER BY u.created_at, u.id"#
    )
    .fetch_all(&state.read_db)
    .await?;
    Ok(users)
}

/// Creates an account with a generated temporary password (works even when
/// open registration is disabled). Inputs must already be validated and normalized.
pub async fn create(state: &AppState, email: &str, name: &str) -> Result<CreatedUser, AppError> {
    let temporary_password = auth_service::generate_temporary_password();
    let user = auth_service::register(state, email, name, &temporary_password).await?;
    tracing::info!(user_id = %user.id, "user created by admin");
    Ok(CreatedUser {
        user: get(state, &user.id).await?,
        temporary_password,
    })
}

pub async fn set_role(
    state: &AppState,
    actor_id: &str,
    user_id: &str,
    role: UserRole,
) -> Result<AdminUser, AppError> {
    // An admin can only change others, so at least one admin (the actor) always remains.
    if actor_id == user_id {
        return Err(AppError::BadRequest(SELF_ROLE_MESSAGE.to_string()));
    }
    let now = now_ms();
    let updated = sqlx::query!(
        "UPDATE users SET role = $1, updated_at = $2 WHERE id = $3",
        role,
        now,
        user_id
    )
    .execute(&state.db)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    tracing::info!(%actor_id, %user_id, ?role, "instance role changed");
    get(state, user_id).await
}

/// Deactivating signs the user out everywhere; their work stays in place.
pub async fn set_active(
    state: &AppState,
    actor_id: &str,
    user_id: &str,
    active: bool,
) -> Result<AdminUser, AppError> {
    if !active && actor_id == user_id {
        return Err(AppError::BadRequest(SELF_DEACTIVATE_MESSAGE.to_string()));
    }
    let now = now_ms();
    let deactivated_at = (!active).then_some(now);

    let mut tx = state.db.begin().await?;
    // COALESCE keeps the original deactivation time if the user is already deactivated.
    let updated = sqlx::query!(
        "UPDATE users
         SET deactivated_at = CASE WHEN $1 IS NULL THEN NULL ELSE COALESCE(deactivated_at, $1) END,
             updated_at = $2
         WHERE id = $3",
        deactivated_at,
        now,
        user_id
    )
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    if !active {
        sqlx::query!("DELETE FROM refresh_tokens WHERE user_id = $1", user_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;

    tracing::info!(%actor_id, %user_id, active, "user activation changed");
    get(state, user_id).await
}

/// Replaces the password with a generated one and signs the user out everywhere.
pub async fn reset_password(
    state: &AppState,
    actor_id: &str,
    user_id: &str,
) -> Result<PasswordReset, AppError> {
    if actor_id == user_id {
        return Err(AppError::BadRequest(SELF_RESET_MESSAGE.to_string()));
    }
    let temporary_password = auth_service::generate_temporary_password();
    let password_hash = auth_service::hash_password(temporary_password.clone()).await?;
    let now = now_ms();

    let mut tx = state.db.begin().await?;
    let updated = sqlx::query!(
        "UPDATE users SET password = $1, updated_at = $2 WHERE id = $3",
        password_hash,
        now,
        user_id
    )
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    sqlx::query!("DELETE FROM refresh_tokens WHERE user_id = $1", user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    tracing::info!(%actor_id, %user_id, "password reset by admin");
    Ok(PasswordReset { temporary_password })
}

async fn get(state: &AppState, user_id: &str) -> Result<AdminUser, AppError> {
    let user = sqlx::query_as!(
        AdminUser,
        r#"SELECT u.id AS "id!", u.email, u.name, u.avatar_color, u.role AS "role: UserRole",
                  u.created_at, u.deactivated_at AS "deactivated_at?",
                  (SELECT COUNT(*) FROM workspace_members m WHERE m.user_id = u.id)
                      AS "workspace_count!: i64"
           FROM users u
           WHERE u.id = $1"#,
        user_id
    )
    .fetch_optional(&state.read_db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(user)
}
