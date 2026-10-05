//! Auth business logic: registration, credential checks, refresh-token rotation.
//!
//! The refresh cookie value is `<row id>.<secret>`. bcrypt hashes are salted, so
//! the row cannot be found by hash; the id locates it and bcrypt verifies the secret.

use std::sync::OnceLock;

use anyhow::Context;

use crate::auth::{jwt, BCRYPT_COST};
use crate::db;
use crate::errors::AppError;
use crate::models::{RefreshToken, User, UserRole};
use crate::AppState;

const MILLIS_PER_DAY: i64 = 24 * 60 * 60 * 1000;
const REFRESH_TOKEN_SEPARATOR: char = '.';
const EMAIL_TAKEN_MESSAGE: &str = "email is already registered";
/// Only used to build a hash so unknown-email logins cost the same as wrong-password ones.
const TIMING_DUMMY_PASSWORD: &str = "zeroboard-timing-equalizer";

static TIMING_DUMMY_HASH: OnceLock<String> = OnceLock::new();

pub struct Session {
    pub user: User,
    pub access_token: String,
    pub refresh_token: String,
}

pub struct RotatedTokens {
    pub access_token: String,
    pub refresh_token: String,
}

struct NewRefreshToken {
    row: RefreshToken,
    cookie_value: String,
}

/// Inputs must already be validated and normalized by the handler.
pub async fn register(
    state: &AppState,
    email: &str,
    name: &str,
    password: &str,
) -> Result<User, AppError> {
    let taken = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM users WHERE email = $1) AS "taken!: bool""#,
        email
    )
    .fetch_one(&state.read_db)
    .await?;
    if taken {
        return Err(AppError::BadRequest(EMAIL_TAKEN_MESSAGE.to_string()));
    }

    let password = password.to_owned();
    let password_hash = run_blocking(move || bcrypt::hash(password, BCRYPT_COST)).await?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_ms();
    let first_user_is_admin = state.config.first_user_is_admin;

    // The admin decision runs inside the INSERT on the single-writer pool, so two
    // concurrent first registrations cannot both become admin.
    let inserted = sqlx::query_as!(
        User,
        r#"INSERT INTO users (id, email, name, password, role, created_at, updated_at)
           VALUES ($1, $2, $3, $4,
                   CASE WHEN $5 AND NOT EXISTS (SELECT 1 FROM users) THEN 'admin' ELSE 'member' END,
                   $6, $7)
           RETURNING id AS "id!", email AS "email!", name AS "name!", password AS "password!",
                     avatar_color AS "avatar_color!", role AS "role!: UserRole",
                     created_at AS "created_at!", updated_at AS "updated_at!""#,
        id,
        email,
        name,
        password_hash,
        first_user_is_admin,
        now,
        now
    )
    .fetch_all(&state.db)
    .await
    .and_then(db::single_row);

    match inserted {
        Ok(user) => {
            tracing::info!(user_id = %user.id, role = ?user.role, "user registered");
            Ok(user)
        }
        Err(err) if is_unique_violation(&err) => {
            Err(AppError::BadRequest(EMAIL_TAKEN_MESSAGE.to_string()))
        }
        Err(err) => Err(err.into()),
    }
}

pub async fn login(state: &AppState, email: &str, password: &str) -> Result<Session, AppError> {
    let user = sqlx::query_as!(
        User,
        r#"SELECT id AS "id!", email, name, password, avatar_color, role AS "role: UserRole",
                  created_at, updated_at
           FROM users WHERE email = $1"#,
        email
    )
    .fetch_optional(&state.read_db)
    .await?;

    let password = password.to_owned();
    let Some(user) = user else {
        // Result ignored: this only equalizes response time for unknown emails.
        let _ = run_blocking(move || {
            let dummy = TIMING_DUMMY_HASH
                .get_or_init(|| bcrypt::hash(TIMING_DUMMY_PASSWORD, BCRYPT_COST).unwrap_or_default());
            bcrypt::verify(password, dummy)
        })
        .await;
        return Err(AppError::Unauthorized);
    };

    let password_hash = user.password.clone();
    if !run_blocking(move || bcrypt::verify(password, &password_hash)).await? {
        return Err(AppError::Unauthorized);
    }

    let new_token = new_refresh_token(&user.id, state.config.refresh_token_expiry_days).await?;
    let mut tx = state.db.begin().await?;
    sqlx::query!(
        "DELETE FROM refresh_tokens WHERE user_id = $1 AND expires_at <= $2",
        user.id,
        new_token.row.created_at
    )
    .execute(&mut *tx)
    .await?;
    insert_refresh_token(&mut *tx, &new_token.row).await?;
    tx.commit().await?;

    let access_token = issue_access_token(state, &user.id, &user.email)?;
    tracing::info!(user_id = %user.id, "user logged in");
    Ok(Session {
        user,
        access_token,
        refresh_token: new_token.cookie_value,
    })
}

pub async fn logout(
    state: &AppState,
    user_id: &str,
    refresh_cookie: Option<&str>,
) -> Result<(), AppError> {
    if let Some((token_id, _)) = refresh_cookie.and_then(parse_refresh_token) {
        sqlx::query!(
            "DELETE FROM refresh_tokens WHERE id = $1 AND user_id = $2",
            token_id,
            user_id
        )
        .execute(&state.db)
        .await?;
    }
    tracing::info!(user_id = %user_id, "user logged out");
    Ok(())
}

pub async fn refresh(state: &AppState, refresh_cookie: &str) -> Result<RotatedTokens, AppError> {
    let (token_id, secret) = parse_refresh_token(refresh_cookie).ok_or(AppError::Unauthorized)?;

    let stored = sqlx::query_as!(
        RefreshToken,
        r#"SELECT id AS "id!", user_id, token_hash, expires_at, created_at
           FROM refresh_tokens WHERE id = $1"#,
        token_id
    )
    .fetch_optional(&state.read_db)
    .await?
    .ok_or(AppError::Unauthorized)?;

    let secret = secret.to_owned();
    let token_hash = stored.token_hash.clone();
    if !run_blocking(move || bcrypt::verify(secret, &token_hash)).await? {
        return Err(AppError::Unauthorized);
    }

    if stored.expires_at <= now_ms() {
        sqlx::query!("DELETE FROM refresh_tokens WHERE id = $1", stored.id)
            .execute(&state.db)
            .await?;
        return Err(AppError::Unauthorized);
    }

    let email = sqlx::query_scalar!("SELECT email FROM users WHERE id = $1", stored.user_id)
        .fetch_optional(&state.read_db)
        .await?
        .ok_or(AppError::Unauthorized)?;

    let new_token = new_refresh_token(&stored.user_id, state.config.refresh_token_expiry_days).await?;
    let mut tx = state.db.begin().await?;
    let deleted = sqlx::query!("DELETE FROM refresh_tokens WHERE id = $1", stored.id)
        .execute(&mut *tx)
        .await?;
    if deleted.rows_affected() == 0 {
        // Already rotated by a concurrent request; each token is single-use.
        return Err(AppError::Unauthorized);
    }
    insert_refresh_token(&mut *tx, &new_token.row).await?;
    tx.commit().await?;

    Ok(RotatedTokens {
        access_token: issue_access_token(state, &stored.user_id, &email)?,
        refresh_token: new_token.cookie_value,
    })
}

pub async fn current_user(state: &AppState, user_id: &str) -> Result<User, AppError> {
    sqlx::query_as!(
        User,
        r#"SELECT id AS "id!", email, name, password, avatar_color, role AS "role: UserRole",
                  created_at, updated_at
           FROM users WHERE id = $1"#,
        user_id
    )
    .fetch_optional(&state.read_db)
    .await?
    // A valid token for a deleted user is treated as no longer authenticated.
    .ok_or(AppError::Unauthorized)
}

/// Splits a refresh cookie into `(row id, secret)`, rejecting anything malformed.
fn parse_refresh_token(value: &str) -> Option<(&str, &str)> {
    let (id, secret) = value.split_once(REFRESH_TOKEN_SEPARATOR)?;
    uuid::Uuid::parse_str(id).ok()?;
    jwt::is_well_formed_refresh_token(secret).then_some((id, secret))
}

async fn new_refresh_token(user_id: &str, expiry_days: i64) -> Result<NewRefreshToken, AppError> {
    let secret = jwt::generate_refresh_token();
    let token_secret = secret.clone();
    let token_hash = run_blocking(move || jwt::hash_refresh_token(&token_secret)).await?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_ms();
    Ok(NewRefreshToken {
        cookie_value: format!("{id}{REFRESH_TOKEN_SEPARATOR}{secret}"),
        row: RefreshToken {
            id,
            user_id: user_id.to_owned(),
            token_hash,
            expires_at: now.saturating_add(expiry_days.saturating_mul(MILLIS_PER_DAY)),
            created_at: now,
        },
    })
}

async fn insert_refresh_token<'e>(
    executor: impl sqlx::SqliteExecutor<'e>,
    token: &RefreshToken,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO refresh_tokens (id, user_id, token_hash, expires_at, created_at)
         VALUES ($1, $2, $3, $4, $5)",
        token.id,
        token.user_id,
        token.token_hash,
        token.expires_at,
        token.created_at
    )
    .execute(executor)
    .await?;
    Ok(())
}

fn issue_access_token(state: &AppState, user_id: &str, email: &str) -> Result<String, AppError> {
    Ok(jwt::generate_access_token(
        user_id,
        email,
        &state.config.jwt_secret,
        state.config.jwt_expiry_minutes,
    )?)
}

/// bcrypt is deliberately slow; keep it off the async worker threads.
async fn run_blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, bcrypt::BcryptError> + Send + 'static,
) -> Result<T, AppError> {
    let result = tokio::task::spawn_blocking(work)
        .await
        .context("bcrypt task failed to complete")?
        .context("bcrypt operation failed")?;
    Ok(result)
}

fn is_unique_violation(err: &sqlx::Error) -> bool {
    err.as_database_error()
        .is_some_and(|db_err| db_err.is_unique_violation())
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_ID: &str = "8f14e45f-ceea-467a-9575-0d9c2c7b6a51";

    #[test]
    fn parses_well_formed_refresh_cookie() {
        let secret = jwt::generate_refresh_token();
        let value = format!("{VALID_ID}.{secret}");
        assert_eq!(parse_refresh_token(&value), Some((VALID_ID, secret.as_str())));
    }

    #[test]
    fn rejects_malformed_refresh_cookie() {
        let secret = jwt::generate_refresh_token();
        assert_eq!(parse_refresh_token(&secret), None);
        assert_eq!(parse_refresh_token(&format!("not-a-uuid.{secret}")), None);
        assert_eq!(parse_refresh_token(&format!("{VALID_ID}.short")), None);
        assert_eq!(parse_refresh_token(""), None);
    }
}
