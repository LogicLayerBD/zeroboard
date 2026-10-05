use std::net::{IpAddr, SocketAddr};

use axum::extract::rejection::JsonRejection;
use axum::extract::{ConnectInfo, State};
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{middleware, Extension, Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::auth::rate_limit::RateLimiter;
use crate::auth::{require_auth, service, AuthUser};
use crate::errors::AppError;
use crate::AppState;

const REFRESH_COOKIE_NAME: &str = "refresh_token";
/// The browser only sends the refresh cookie to auth endpoints.
const REFRESH_COOKIE_PATH: &str = "/api/auth";
const SECONDS_PER_MINUTE: i64 = 60;
const SECONDS_PER_DAY: i64 = 24 * 60 * 60;
const TOKEN_TYPE: &str = "Bearer";

const MIN_PASSWORD_CHARS: usize = 8;
/// bcrypt ignores input past 72 bytes; longer passwords would be silently truncated.
const MAX_PASSWORD_BYTES: usize = 72;
const MAX_NAME_CHARS: usize = 255;
/// RFC 5321 maximum forward-path length.
const MAX_EMAIL_LEN: usize = 254;

pub fn router(state: &AppState) -> Router<AppState> {
    let protected = Router::new()
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/me", get(me))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth));

    Router::new()
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/refresh", post(refresh))
        .merge(protected)
}

#[derive(Deserialize)]
pub struct RegisterRequest {
    email: String,
    name: String,
    password: String,
}

struct ValidRegistration {
    email: String,
    name: String,
    password: String,
}

impl RegisterRequest {
    fn validate(self) -> Result<ValidRegistration, AppError> {
        let email = normalize_email(&self.email);
        if !is_valid_email(&email) {
            return Err(AppError::BadRequest("email is invalid".into()));
        }
        let name = self.name.trim();
        if name.is_empty() {
            return Err(AppError::BadRequest("name is required".into()));
        }
        if name.chars().count() > MAX_NAME_CHARS {
            return Err(AppError::BadRequest(format!(
                "name must be at most {MAX_NAME_CHARS} characters"
            )));
        }
        if self.password.chars().count() < MIN_PASSWORD_CHARS {
            return Err(AppError::BadRequest(format!(
                "password must be at least {MIN_PASSWORD_CHARS} characters"
            )));
        }
        if self.password.len() > MAX_PASSWORD_BYTES {
            return Err(AppError::BadRequest(format!(
                "password must be at most {MAX_PASSWORD_BYTES} bytes"
            )));
        }
        Ok(ValidRegistration {
            email,
            name: name.to_string(),
            password: self.password,
        })
    }
}

#[derive(Deserialize)]
pub struct LoginRequest {
    email: String,
    password: String,
}

impl LoginRequest {
    fn validate(self) -> Result<(String, String), AppError> {
        let email = normalize_email(&self.email);
        if email.is_empty() || self.password.is_empty() {
            return Err(AppError::BadRequest("email and password are required".into()));
        }
        // Over-long input can never match a stored hash; reject before doing bcrypt work.
        if email.len() > MAX_EMAIL_LEN || self.password.len() > MAX_PASSWORD_BYTES {
            return Err(AppError::Unauthorized);
        }
        Ok((email, self.password))
    }
}

pub async fn register(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    body: Result<Json<RegisterRequest>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    enforce_rate_limit(&state.register_limiter, addr.ip(), "register")?;
    let Json(body) = body?;
    let input = body.validate()?;
    let user = service::register(&state, &input.email, &input.name, &input.password).await?;
    Ok((StatusCode::CREATED, Json(user)))
}

pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    body: Result<Json<LoginRequest>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    enforce_rate_limit(&state.login_limiter, addr.ip(), "login")?;
    let Json(body) = body?;
    let (email, password) = body.validate()?;
    let session = service::login(&state, &email, &password).await?;
    Ok((
        [(SET_COOKIE, refresh_cookie(&state, &session.refresh_token))],
        Json(json!({
            "access_token": session.access_token,
            "token_type": TOKEN_TYPE,
            "expires_in": access_token_lifetime_secs(&state),
            "user": session.user,
        })),
    ))
}

pub async fn logout(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    service::logout(&state, &auth_user.id, read_cookie(&headers, REFRESH_COOKIE_NAME)).await?;
    Ok((StatusCode::NO_CONTENT, [(SET_COOKIE, cleared_refresh_cookie())]))
}

pub async fn refresh(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(cookie) = read_cookie(&headers, REFRESH_COOKIE_NAME) else {
        return AppError::Unauthorized.into_response();
    };
    match service::refresh(&state, cookie).await {
        Ok(tokens) => (
            [(SET_COOKIE, refresh_cookie(&state, &tokens.refresh_token))],
            Json(json!({
                "access_token": tokens.access_token,
                "token_type": TOKEN_TYPE,
                "expires_in": access_token_lifetime_secs(&state),
            })),
        )
            .into_response(),
        // Drop the dead cookie so the client stops retrying with it.
        Err(AppError::Unauthorized) => (
            [(SET_COOKIE, cleared_refresh_cookie())],
            AppError::Unauthorized,
        )
            .into_response(),
        Err(err) => err.into_response(),
    }
}

pub async fn me(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(service::current_user(&state, &auth_user.id).await?))
}

/// Uses the socket peer address only; proxy headers like X-Forwarded-For are not trusted.
fn enforce_rate_limit(limiter: &RateLimiter, ip: IpAddr, endpoint: &str) -> Result<(), AppError> {
    if limiter.check(ip) {
        return Ok(());
    }
    tracing::warn!(%ip, endpoint, "auth rate limit exceeded");
    Err(AppError::TooManyRequests)
}

pub(crate) fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

pub(crate) fn is_valid_email(email: &str) -> bool {
    if email.len() > MAX_EMAIL_LEN
        || email.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return false;
    }
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.contains('@')
        && domain.contains('.')
        && domain.split('.').all(|label| !label.is_empty())
}

fn refresh_cookie(state: &AppState, value: &str) -> String {
    let max_age = state
        .config
        .refresh_token_expiry_days
        .saturating_mul(SECONDS_PER_DAY);
    build_refresh_cookie(value, max_age)
}

fn cleared_refresh_cookie() -> String {
    build_refresh_cookie("", 0)
}

fn build_refresh_cookie(value: &str, max_age_secs: i64) -> String {
    format!(
        "{REFRESH_COOKIE_NAME}={value}; HttpOnly; Secure; SameSite=Strict; \
         Path={REFRESH_COOKIE_PATH}; Max-Age={max_age_secs}"
    )
}

fn read_cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(key, value)| *key == name && !value.is_empty())
        .map(|(_, value)| value)
}

fn access_token_lifetime_secs(state: &AppState) -> i64 {
    state
        .config
        .jwt_expiry_minutes
        .saturating_mul(SECONDS_PER_MINUTE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::rate_limit::{LOGIN_MAX_ATTEMPTS, REGISTER_MAX_ATTEMPTS};
    use crate::config::Config;
    use axum::body::{to_bytes, Body};
    use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
    use axum::http::Request;
    use serde_json::Value;
    use std::path::PathBuf;
    use tower::ServiceExt;

    const MAX_TEST_BODY_BYTES: usize = 64 * 1024;
    const PASSWORD: &str = "correct-horse";
    const CLIENT_PORT: u16 = 40000;
    const DEFAULT_CLIENT_OCTET: u8 = 1;
    const VALIDATION_CLIENT_OCTET_START: u8 = 100;
    const EMAIL_TAKEN_FOR_TEST: &str = "email is already registered";

    struct TestApp {
        app: Router,
        state: AppState,
        dir: PathBuf,
    }

    impl TestApp {
        async fn new() -> Self {
            Self::with_first_user_admin(true).await
        }

        async fn with_first_user_admin(first_user_is_admin: bool) -> Self {
            let dir = std::env::temp_dir().join(format!("zeroboard-auth-{}", uuid::Uuid::new_v4()));
            let url = format!("sqlite://{}", dir.join("test.db").display());
            let db = crate::db::connect(&url).await.unwrap();
            crate::db::run_migrations(&db).await.unwrap();
            let read_db = crate::db::create_read_pool(&url).await.unwrap();
            let config = Config {
                host: "127.0.0.1".parse().unwrap(),
                port: 0,
                jwt_secret: "a-test-secret-that-is-at-least-32-chars".into(),
                jwt_expiry_minutes: 15,
                refresh_token_expiry_days: 30,
                database_url: url,
                attachments_dir: dir.clone(),
                max_attachment_size_mb: 25,
                app_name: "ZeroBoard".into(),
                first_user_is_admin,
            };
            let state = AppState::new(db, read_db, config);
            let app = crate::router(state.clone());
            Self { app, state, dir }
        }

        async fn send(&self, req: Request<Body>) -> (StatusCode, HeaderMap, Value) {
            self.send_from(req, DEFAULT_CLIENT_OCTET).await
        }

        async fn send_from(
            &self,
            mut req: Request<Body>,
            client_octet: u8,
        ) -> (StatusCode, HeaderMap, Value) {
            let client = SocketAddr::from(([127, 0, 0, client_octet], CLIENT_PORT));
            req.extensions_mut().insert(ConnectInfo(client));
            let response = self.app.clone().oneshot(req).await.unwrap();
            let status = response.status();
            let headers = response.headers().clone();
            let bytes = to_bytes(response.into_body(), MAX_TEST_BODY_BYTES).await.unwrap();
            let body = if bytes.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(&bytes).unwrap()
            };
            (status, headers, body)
        }

        async fn post_json(&self, uri: &str, body: Value) -> (StatusCode, HeaderMap, Value) {
            self.post_json_from(uri, body, DEFAULT_CLIENT_OCTET).await
        }

        async fn post_json_from(
            &self,
            uri: &str,
            body: Value,
            client_octet: u8,
        ) -> (StatusCode, HeaderMap, Value) {
            self.send_from(
                Request::post(uri)
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
                client_octet,
            )
            .await
        }

        async fn register(&self, email: &str) -> (StatusCode, Value) {
            let (status, _, body) = self
                .post_json(
                    "/api/auth/register",
                    json!({ "email": email, "name": "Test User", "password": PASSWORD }),
                )
                .await;
            (status, body)
        }

        /// Returns (access token, refresh cookie value).
        async fn login(&self, email: &str) -> (String, String) {
            let (status, headers, body) = self
                .post_json("/api/auth/login", json!({ "email": email, "password": PASSWORD }))
                .await;
            assert_eq!(status, StatusCode::OK, "login failed: {body}");
            (body["access_token"].as_str().unwrap().to_string(), cookie_value(&headers))
        }

        async fn refresh_with(&self, cookie: &str) -> (StatusCode, HeaderMap, Value) {
            self.send(
                Request::post("/api/auth/refresh")
                    .header(COOKIE, format!("{REFRESH_COOKIE_NAME}={cookie}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
        }

        async fn get_me(&self, access_token: Option<&str>) -> (StatusCode, Value) {
            let mut req = Request::get("/api/auth/me");
            if let Some(token) = access_token {
                req = req.header(AUTHORIZATION, format!("Bearer {token}"));
            }
            let (status, _, body) = self.send(req.body(Body::empty()).unwrap()).await;
            (status, body)
        }

        async fn refresh_token_count(&self) -> i64 {
            sqlx::query_scalar("SELECT COUNT(*) FROM refresh_tokens")
                .fetch_one(&self.state.db)
                .await
                .unwrap()
        }

        async fn cleanup(self) {
            self.state.db.close().await;
            self.state.read_db.close().await;
            let _ = tokio::fs::remove_dir_all(&self.dir).await;
        }
    }

    fn set_cookie(headers: &HeaderMap) -> String {
        headers.get(SET_COOKIE).unwrap().to_str().unwrap().to_string()
    }

    fn cookie_value(headers: &HeaderMap) -> String {
        let cookie = set_cookie(headers);
        let pair = cookie.split(';').next().unwrap();
        pair.split_once('=').unwrap().1.to_string()
    }

    #[tokio::test]
    async fn register_returns_user_without_password_and_first_user_is_admin() {
        let t = TestApp::new().await;

        let (status, first) = t.register("  First@Example.com ").await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(first["email"], "first@example.com");
        assert_eq!(first["role"], "admin");
        assert!(first.get("password").is_none());

        let (status, second) = t.register("second@example.com").await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(second["role"], "member");

        let stored: String = sqlx::query_scalar("SELECT password FROM users WHERE email = 'second@example.com'")
            .fetch_one(&t.state.db)
            .await
            .unwrap();
        assert_ne!(stored, PASSWORD);
        assert!(bcrypt::verify(PASSWORD, &stored).unwrap());

        t.cleanup().await;
    }

    #[tokio::test]
    async fn first_user_is_member_when_admin_flag_disabled() {
        let t = TestApp::with_first_user_admin(false).await;
        let (status, user) = t.register("first@example.com").await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(user["role"], "member");
        t.cleanup().await;
    }

    #[tokio::test]
    async fn register_rejects_invalid_input_and_duplicate_email() {
        let t = TestApp::new().await;
        let cases = [
            json!({ "email": "not-an-email", "name": "A", "password": PASSWORD }),
            json!({ "email": "a@nodot", "name": "A", "password": PASSWORD }),
            json!({ "email": "a@b.co", "name": "   ", "password": PASSWORD }),
            json!({ "email": "a@b.co", "name": "A", "password": "short" }),
            json!({ "email": "a@b.co", "name": "A", "password": "x".repeat(MAX_PASSWORD_BYTES + 1) }),
            json!({ "email": "a@b.co" }),
        ];
        // Each case uses its own client IP so the register rate limit is not the cause of failure.
        for (octet, body) in (VALIDATION_CLIENT_OCTET_START..).zip(cases) {
            let (status, _, _) = t.post_json_from("/api/auth/register", body.clone(), octet).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "expected 400 for {body}");
        }

        assert_eq!(t.register("dup@example.com").await.0, StatusCode::CREATED);
        let (status, body) = t.register("DUP@example.com").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], EMAIL_TAKEN_FOR_TEST);

        t.cleanup().await;
    }

    #[tokio::test]
    async fn login_sets_secure_cookie_and_stores_only_hash() {
        let t = TestApp::new().await;
        t.register("user@example.com").await;

        let (status, headers, body) = t
            .post_json("/api/auth/login", json!({ "email": "user@example.com", "password": PASSWORD }))
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["token_type"], "Bearer");
        assert_eq!(body["user"]["email"], "user@example.com");
        assert!(body["user"].get("password").is_none());

        let cookie = set_cookie(&headers);
        for attr in ["HttpOnly", "Secure", "SameSite=Strict", "Path=/api/auth"] {
            assert!(cookie.contains(attr), "cookie missing {attr}: {cookie}");
        }

        let value = cookie_value(&headers);
        let (_, secret) = value.split_once('.').unwrap();
        let stored_hash: String = sqlx::query_scalar("SELECT token_hash FROM refresh_tokens")
            .fetch_one(&t.state.db)
            .await
            .unwrap();
        assert_ne!(stored_hash, secret);
        assert!(bcrypt::verify(secret, &stored_hash).unwrap());

        t.cleanup().await;
    }

    #[tokio::test]
    async fn login_rejects_bad_credentials_with_401() {
        let t = TestApp::new().await;
        t.register("user@example.com").await;

        for body in [
            json!({ "email": "user@example.com", "password": "wrong-password" }),
            json!({ "email": "nobody@example.com", "password": PASSWORD }),
        ] {
            let (status, headers, _) = t.post_json("/api/auth/login", body).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
            assert!(headers.get(SET_COOKIE).is_none());
        }
        assert_eq!(t.refresh_token_count().await, 0);

        t.cleanup().await;
    }

    #[tokio::test]
    async fn me_requires_valid_bearer_token() {
        let t = TestApp::new().await;
        t.register("user@example.com").await;
        let (access, _) = t.login("user@example.com").await;

        assert_eq!(t.get_me(None).await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(t.get_me(Some("garbage")).await.0, StatusCode::UNAUTHORIZED);

        let (status, user) = t.get_me(Some(&access)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(user["email"], "user@example.com");
        assert!(user.get("password").is_none());

        t.cleanup().await;
    }

    #[tokio::test]
    async fn refresh_rotates_token_and_old_token_is_single_use() {
        let t = TestApp::new().await;
        t.register("user@example.com").await;
        let (_, old_cookie) = t.login("user@example.com").await;

        let (status, headers, body) = t.refresh_with(&old_cookie).await;
        assert_eq!(status, StatusCode::OK);
        let new_access = body["access_token"].as_str().unwrap();
        assert_eq!(t.get_me(Some(new_access)).await.0, StatusCode::OK);
        let new_cookie = cookie_value(&headers);
        assert_ne!(new_cookie, old_cookie);
        assert_eq!(t.refresh_token_count().await, 1);

        let (status, headers, _) = t.refresh_with(&old_cookie).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(set_cookie(&headers).contains("Max-Age=0"));

        assert_eq!(t.refresh_with(&new_cookie).await.0, StatusCode::OK);

        t.cleanup().await;
    }

    #[tokio::test]
    async fn refresh_rejects_missing_malformed_and_expired_tokens() {
        let t = TestApp::new().await;
        t.register("user@example.com").await;
        let (_, cookie) = t.login("user@example.com").await;

        let (status, _, _) = t
            .send(Request::post("/api/auth/refresh").body(Body::empty()).unwrap())
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(t.refresh_with("garbage").await.0, StatusCode::UNAUTHORIZED);

        let (id, _) = cookie.split_once('.').unwrap();
        let forged = format!("{id}.{}", crate::auth::jwt::generate_refresh_token());
        assert_eq!(t.refresh_with(&forged).await.0, StatusCode::UNAUTHORIZED);

        sqlx::query("UPDATE refresh_tokens SET expires_at = 0")
            .execute(&t.state.db)
            .await
            .unwrap();
        assert_eq!(t.refresh_with(&cookie).await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(t.refresh_token_count().await, 0, "expired token is removed");

        t.cleanup().await;
    }

    #[tokio::test]
    async fn logout_deletes_refresh_token_and_clears_cookie() {
        let t = TestApp::new().await;
        t.register("user@example.com").await;
        let (access, cookie) = t.login("user@example.com").await;

        let unauthenticated = Request::post("/api/auth/logout").body(Body::empty()).unwrap();
        assert_eq!(t.send(unauthenticated).await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(t.refresh_token_count().await, 1);

        let (status, headers, _) = t
            .send(
                Request::post("/api/auth/logout")
                    .header(AUTHORIZATION, format!("Bearer {access}"))
                    .header(COOKIE, format!("{REFRESH_COOKIE_NAME}={cookie}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(set_cookie(&headers).contains("Max-Age=0"));
        assert_eq!(t.refresh_token_count().await, 0);
        assert_eq!(t.refresh_with(&cookie).await.0, StatusCode::UNAUTHORIZED);

        t.cleanup().await;
    }

    #[tokio::test]
    async fn login_and_register_are_rate_limited_per_ip() {
        let t = TestApp::new().await;
        let invalid = json!({});

        for _ in 0..LOGIN_MAX_ATTEMPTS {
            let (status, _, _) = t.post_json("/api/auth/login", invalid.clone()).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
        }
        let (status, _, _) = t.post_json("/api/auth/login", invalid.clone()).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);

        for _ in 0..REGISTER_MAX_ATTEMPTS {
            let (status, _, _) = t.post_json("/api/auth/register", invalid.clone()).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
        }
        let (status, _, _) = t.post_json("/api/auth/register", invalid).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);

        t.cleanup().await;
    }

    #[test]
    fn email_validation() {
        for ok in ["a@b.co", "first.last+tag@sub.example.com"] {
            assert!(is_valid_email(ok), "{ok} should be valid");
        }
        for bad in ["", "a", "@b.co", "a@", "a@b", "a@@b.co", "a b@c.co", "a@b..co", "a@.co"] {
            assert!(!is_valid_email(bad), "{bad} should be invalid");
        }
    }

    #[test]
    fn reads_named_cookie_among_others() {
        let mut headers = HeaderMap::new();
        headers.insert(COOKIE, "theme=dark; refresh_token=abc.def; x=1".parse().unwrap());
        assert_eq!(read_cookie(&headers, REFRESH_COOKIE_NAME), Some("abc.def"));
        assert_eq!(read_cookie(&headers, "missing"), None);
    }
}
