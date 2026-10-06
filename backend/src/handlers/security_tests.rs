//! Cross-cutting security and authorization tests that span several routers.

use std::cell::RefCell;
use std::io;
use std::sync::{Arc, Mutex, Once};

use axum::http::{Method, StatusCode};
use serde_json::{json, Value};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::EnvFilter;

use super::test_support::{BoardFixture, TestApp, TestUser, TEST_PASSWORD};
use crate::auth::{jwt, service as auth_service};
use crate::errors::AppError;

const PLACEHOLDER_ID: &str = "00000000-0000-4000-8000-000000000000";
const WRONG_SECRET: &str = "another-secret-that-is-at-least-32-chars";
const SECONDS_PER_HOUR: i64 = 3600;
/// base64url of `{"alg":"none","typ":"JWT"}`.
const ALG_NONE_HEADER: &str = "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0";
/// Every bcrypt hash starts with this; none may ever leave the server.
const BCRYPT_PREFIX: &str = "$2";

/// Every route behind `require_auth`. `:id` segments use a placeholder UUID because
/// authentication must fail before any lookup happens.
const PROTECTED_ROUTES: &[(&str, &str)] = &[
    ("POST", "/api/auth/logout"),
    ("GET", "/api/auth/me"),
    ("POST", "/api/auth/password"),
    ("GET", "/api/admin/info"),
    ("GET", "/api/admin/storage"),
    ("GET", "/api/admin/settings"),
    ("PATCH", "/api/admin/settings"),
    ("GET", "/api/admin/users"),
    ("POST", "/api/admin/users"),
    ("PATCH", "/api/admin/users/:id/role"),
    ("POST", "/api/admin/users/:id/reset-password"),
    ("POST", "/api/admin/users/:id/deactivate"),
    ("POST", "/api/admin/users/:id/reactivate"),
    ("GET", "/api/workspaces"),
    ("POST", "/api/workspaces"),
    ("PATCH", "/api/workspaces/:id"),
    ("DELETE", "/api/workspaces/:id"),
    ("GET", "/api/workspaces/:id/members"),
    ("POST", "/api/workspaces/:id/members/invite"),
    ("GET", "/api/workspaces/:id/members/candidates"),
    ("DELETE", "/api/workspaces/:id/members/:id"),
    ("PATCH", "/api/workspaces/:id/members/:id/role"),
    ("GET", "/api/workspaces/:id/boards"),
    ("POST", "/api/workspaces/:id/boards"),
    ("GET", "/api/boards/:id"),
    ("PATCH", "/api/boards/:id"),
    ("DELETE", "/api/boards/:id"),
    ("GET", "/api/boards/:id/lists"),
    ("POST", "/api/boards/:id/lists"),
    ("PATCH", "/api/lists/:id"),
    ("DELETE", "/api/lists/:id"),
    ("PATCH", "/api/lists/:id/position"),
    ("GET", "/api/lists/:id/cards"),
    ("POST", "/api/lists/:id/cards"),
    ("GET", "/api/cards/:id"),
    ("PATCH", "/api/cards/:id"),
    ("DELETE", "/api/cards/:id"),
    ("PATCH", "/api/cards/:id/move"),
    ("POST", "/api/cards/:id/assignees"),
    ("DELETE", "/api/cards/:id/assignees/:id"),
    ("POST", "/api/cards/:id/labels"),
    ("DELETE", "/api/cards/:id/labels/:id"),
    ("POST", "/api/cards/:id/attachments"),
    ("GET", "/api/attachments/:id/download"),
    ("DELETE", "/api/attachments/:id"),
    ("GET", "/api/cards/:id/comments"),
    ("POST", "/api/cards/:id/comments"),
    ("PATCH", "/api/comments/:id"),
    ("DELETE", "/api/comments/:id"),
    ("GET", "/api/boards/:id/labels"),
    ("POST", "/api/boards/:id/labels"),
    ("PATCH", "/api/labels/:id"),
    ("DELETE", "/api/labels/:id"),
    ("GET", "/api/cards/:id/time-entries"),
    ("POST", "/api/cards/:id/time-entries"),
    ("DELETE", "/api/time-entries/:id"),
    ("GET", "/api/notifications"),
    ("PATCH", "/api/notifications/:id/read"),
    ("POST", "/api/notifications/read-all"),
];

fn token_with(user: &TestUser, secret: &str, algorithm: jsonwebtoken::Algorithm, exp_offset_secs: i64) -> String {
    let now = chrono::Utc::now().timestamp();
    let claims = jwt::Claims {
        sub: user.id.clone(),
        email: user.email.clone(),
        exp: usize::try_from(now + exp_offset_secs).unwrap(),
        iat: usize::try_from(now).unwrap(),
    };
    jsonwebtoken::encode(
        &jsonwebtoken::Header::new(algorithm),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap()
}

/// Same claims as a valid token, but declaring `alg: none` with no signature.
fn unsigned_token(valid_token: &str) -> String {
    let payload = valid_token.split('.').nth(1).unwrap();
    format!("{ALG_NONE_HEADER}.{payload}.")
}

fn bearer(token: String) -> TestUser {
    TestUser {
        id: String::new(),
        email: String::new(),
        token,
    }
}

/// Percent-encodes everything except unreserved characters (RFC 3986).
fn query_encode(raw: &str) -> String {
    raw.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Fails if any object key is `password` or any string looks like a bcrypt hash.
fn assert_no_password_material(value: &Value, context: &str) {
    match value {
        Value::Object(map) => {
            for (key, nested) in map {
                assert_ne!(key, "password", "{context}: response exposes a password field");
                assert_no_password_material(nested, context);
            }
        }
        Value::Array(items) => items.iter().for_each(|v| assert_no_password_material(v, context)),
        Value::String(s) => assert!(!s.starts_with(BCRYPT_PREFIX), "{context}: response exposes a hash"),
        _ => {}
    }
}

async fn session_count(t: &TestApp, user_id: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM refresh_tokens WHERE user_id = $1")
        .bind(user_id)
        .fetch_one(&t.state.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn every_protected_route_rejects_requests_without_a_valid_token() {
    let t = TestApp::new().await;
    let user = t.user("user@example.com").await;
    let deleted = t.user("deleted@example.com").await;
    let deactivated = t.user("deactivated@example.com").await;
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(&deleted.id)
        .execute(&t.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET deactivated_at = 1 WHERE id = $1")
        .bind(&deactivated.id)
        .execute(&t.state.db)
        .await
        .unwrap();

    let secret = t.state.config.jwt_secret.clone();
    let hs256 = jsonwebtoken::Algorithm::HS256;
    let invalid_tokens = [
        ("garbage", bearer("not-a-jwt".into())),
        ("wrong secret", bearer(token_with(&user, WRONG_SECRET, hs256, SECONDS_PER_HOUR))),
        ("expired", bearer(token_with(&user, &secret, hs256, -SECONDS_PER_HOUR))),
        ("alg none", bearer(unsigned_token(&user.token))),
        (
            "algorithm swap",
            bearer(token_with(&user, &secret, jsonwebtoken::Algorithm::HS512, SECONDS_PER_HOUR)),
        ),
        ("deleted user", bearer(deleted.token.clone())),
        ("deactivated user", bearer(deactivated.token.clone())),
    ];

    for (method, template) in PROTECTED_ROUTES {
        let uri = template.replace(":id", PLACEHOLDER_ID);
        let method = Method::from_bytes(method.as_bytes()).unwrap();
        let body = (method == Method::POST || method == Method::PATCH).then(|| json!({}));

        let (status, response) = t.send(method.clone(), &uri, None, body.clone()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri} without token");
        assert_eq!(response, json!({ "error": "unauthorized" }), "{method} {uri} leaks detail");

        for (label, token) in &invalid_tokens {
            let (status, _) = t.send(method.clone(), &uri, Some(token), body.clone()).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri} with {label} token");
        }
    }

    // Sanity check: the valid token is accepted, so the 401s above are about the tokens.
    assert_eq!(t.get("/api/auth/me", &user).await.0, StatusCode::OK);

    t.cleanup().await;
}

#[tokio::test]
async fn instance_admin_rights_are_rechecked_on_every_request() {
    let f = BoardFixture::new().await;
    let role_uri = |user: &TestUser| format!("/api/admin/users/{}/role", user.id);

    let (status, _) = f.t.patch(&role_uri(&f.member), &f.admin, json!({ "role": "admin" })).await;
    assert_eq!(status, StatusCode::OK);
    let own_ws = f.t.workspace(&f.member, "Mine").await;
    let candidates = format!("/api/workspaces/{own_ws}/members/candidates?q=");
    assert_eq!(f.t.get("/api/admin/users", &f.member).await.0, StatusCode::OK);
    assert_eq!(f.t.get(&candidates, &f.member).await.0, StatusCode::OK);

    // Demotion takes effect immediately for the token the member already holds.
    let (status, _) = f.t.patch(&role_uri(&f.member), &f.admin, json!({ "role": "member" })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(f.t.get("/api/admin/users", &f.member).await.0, StatusCode::FORBIDDEN);
    assert_eq!(f.t.get(&candidates, &f.member).await.0, StatusCode::FORBIDDEN);
    let (status, _) = f.t.patch(&role_uri(&f.admin), &f.member, json!({ "role": "member" })).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "demoted admin cannot demote others");

    // A deactivated admin loses everything, even with a still-valid token.
    f.t.patch(&role_uri(&f.viewer), &f.admin, json!({ "role": "admin" })).await;
    let deactivate = format!("/api/admin/users/{}/deactivate", f.viewer.id);
    assert_eq!(f.t.post(&deactivate, &f.admin, json!({})).await.0, StatusCode::OK);
    assert_eq!(f.t.get("/api/admin/users", &f.viewer).await.0, StatusCode::UNAUTHORIZED);
    let own_deactivate = format!("/api/admin/users/{}/deactivate", f.admin.id);
    assert_eq!(f.t.post(&own_deactivate, &f.viewer, json!({})).await.0, StatusCode::UNAUTHORIZED);

    let (_, admin) = f.t.get("/api/auth/me", &f.admin).await;
    assert_eq!(admin["role"], "admin", "the original admin is untouched");

    f.t.cleanup().await;
}

#[tokio::test]
async fn privileged_fields_in_request_bodies_are_ignored() {
    let f = BoardFixture::new().await;
    const CHOSEN_PASSWORD: &str = "attacker-chosen-password";

    let body = json!({
        "email": "sneaky@example.com",
        "name": "Sneaky",
        "role": "admin",
        "id": PLACEHOLDER_ID,
        "deactivated_at": 1,
        "password": CHOSEN_PASSWORD,
        "avatar_color": "#000000"
    });
    let (status, created) = f.t.post("/api/admin/users", &f.admin, body).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let user = &created["user"];
    assert_eq!(user["role"], "member");
    assert_ne!(user["id"], PLACEHOLDER_ID);
    assert!(user["deactivated_at"].is_null());
    assert!(matches!(
        auth_service::login(&f.t.state, "sneaky@example.com", CHOSEN_PASSWORD).await,
        Err(AppError::Unauthorized)
    ));

    let invite = format!("/api/workspaces/{}/members/invite", f.workspace_id);
    let owner = json!({ "email": f.outsider.email, "role": "owner" });
    assert_eq!(f.t.post(&invite, &f.admin, owner).await.0, StatusCode::BAD_REQUEST);

    f.t.cleanup().await;
}

#[tokio::test]
async fn user_facing_responses_never_contain_password_material() {
    let f = BoardFixture::new().await;
    let new_user = json!({ "email": "new@example.com", "name": "New" });
    let user_uri = |action: &str| format!("/api/admin/users/{}/{action}", f.outsider.id);

    let responses = [
        ("me", f.t.get("/api/auth/me", &f.member).await),
        ("list users", f.t.get("/api/admin/users", &f.admin).await),
        ("create user", f.t.post("/api/admin/users", &f.admin, new_user).await),
        ("set role", f.t.patch(&user_uri("role"), &f.admin, json!({ "role": "member" })).await),
        ("reset", f.t.post(&user_uri("reset-password"), &f.admin, json!({})).await),
        ("deactivate", f.t.post(&user_uri("deactivate"), &f.admin, json!({})).await),
        ("reactivate", f.t.post(&user_uri("reactivate"), &f.admin, json!({})).await),
        (
            "members",
            f.t.get(&format!("/api/workspaces/{}/members", f.workspace_id), &f.admin).await,
        ),
        (
            "candidates",
            f.t.get(&format!("/api/workspaces/{}/members/candidates?q=", f.workspace_id), &f.admin)
                .await,
        ),
    ];
    for (label, (status, body)) in &responses {
        assert!(status.is_success(), "{label}: {status} {body}");
        assert_no_password_material(body, label);
    }

    f.t.cleanup().await;
}

#[tokio::test]
async fn search_and_login_inputs_are_not_injectable() {
    let f = BoardFixture::new().await;
    let payloads = [
        "' OR '1'='1",
        "%' OR 1=1 --",
        "\" OR \"\"=\"",
        "'; DROP TABLE users; --",
        "\\%",
    ];

    for payload in payloads {
        let uri = format!(
            "/api/workspaces/{}/members/candidates?q={}",
            f.workspace_id,
            query_encode(payload)
        );
        let (status, body) = f.t.get(&uri, &f.admin).await;
        assert_eq!(status, StatusCode::OK, "{payload}: {body}");
        assert_eq!(body, json!([]), "{payload} matched users");

        assert!(matches!(
            auth_service::login(&f.t.state, payload, payload).await,
            Err(AppError::Unauthorized)
        ));
    }

    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&f.t.state.db)
        .await
        .unwrap();
    assert_eq!(users, 4, "users table intact");

    f.t.cleanup().await;
}

/// Our own code is captured at every level; dependencies only from INFO up.
const CAPTURE_FILTER: &str = "zeroboard=trace,info";

thread_local! {
    static CAPTURE_BUFFER: RefCell<Option<Arc<Mutex<Vec<u8>>>>> = const { RefCell::new(None) };
}

/// Writes log lines into the current thread's capture buffer, if one is active.
/// A process-wide subscriber is used because a thread-scoped one (`set_default`)
/// misses events whose callsite interest other test threads cached first.
struct ThreadCaptureWriter;

impl io::Write for ThreadCaptureWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        CAPTURE_BUFFER.with(|slot| {
            if let Some(buffer) = slot.borrow().as_ref() {
                buffer.lock().unwrap().extend_from_slice(buf);
            }
        });
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct ThreadCaptureMakeWriter;

impl<'a> MakeWriter<'a> for ThreadCaptureMakeWriter {
    type Writer = ThreadCaptureWriter;

    fn make_writer(&'a self) -> Self::Writer {
        ThreadCaptureWriter
    }
}

/// Captures log output emitted on this thread until dropped.
struct LogCapture(Arc<Mutex<Vec<u8>>>);

impl LogCapture {
    fn start() -> Self {
        static INSTALL: Once = Once::new();
        INSTALL.call_once(|| {
            let subscriber = tracing_subscriber::fmt()
                .with_env_filter(EnvFilter::new(CAPTURE_FILTER))
                .with_ansi(false)
                .with_writer(ThreadCaptureMakeWriter)
                .finish();
            tracing::subscriber::set_global_default(subscriber).expect("no other global subscriber in tests");
        });
        let buffer = Arc::new(Mutex::new(Vec::new()));
        CAPTURE_BUFFER.with(|slot| *slot.borrow_mut() = Some(buffer.clone()));
        Self(buffer)
    }

    fn contents(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

impl Drop for LogCapture {
    fn drop(&mut self) {
        CAPTURE_BUFFER.with(|slot| *slot.borrow_mut() = None);
    }
}

#[tokio::test]
async fn passwords_tokens_and_hashes_never_reach_the_logs() {
    // #[tokio::test] is single-threaded, so every event this test triggers lands here.
    let capture = LogCapture::start();
    const NEW_PASSWORD: &str = "my-new-secret-password";
    const WRONG_GUESS: &str = "wrong-guess-password";

    let t = TestApp::new().await;
    let admin = t.user("admin@example.com").await;
    let member = t.user("member@example.com").await;

    let session = auth_service::login(&t.state, &admin.email, TEST_PASSWORD).await.unwrap();
    let rotated = auth_service::refresh(&t.state, &session.refresh_token).await.unwrap();
    let _ = auth_service::login(&t.state, &admin.email, WRONG_GUESS).await;

    let (_, created) = t
        .post("/api/admin/users", &admin, json!({ "email": "new@example.com", "name": "New" }))
        .await;
    let created_password = created["temporary_password"].as_str().unwrap().to_string();
    let (_, reset) = t
        .post(&format!("/api/admin/users/{}/reset-password", member.id), &admin, json!({}))
        .await;
    let reset_password = reset["temporary_password"].as_str().unwrap().to_string();
    let (status, _) = t
        .post(
            "/api/auth/password",
            &admin,
            json!({ "current_password": TEST_PASSWORD, "new_password": NEW_PASSWORD }),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    t.post(&format!("/api/admin/users/{}/deactivate", member.id), &admin, json!({})).await;
    let _ = auth_service::login(&t.state, &member.email, &reset_password).await;
    assert_eq!(session_count(&t, &member.id).await, 0);

    let logs = capture.contents();
    assert!(logs.contains("user created by admin"), "capture works: {logs}");
    assert!(logs.contains("login refused: account deactivated"));
    let secrets = [
        ("login password", TEST_PASSWORD),
        ("new password", NEW_PASSWORD),
        ("wrong guess", WRONG_GUESS),
        ("temporary password", created_password.as_str()),
        ("reset password", reset_password.as_str()),
        ("access token", session.access_token.as_str()),
        ("admin access token", admin.token.as_str()),
        ("refresh cookie", session.refresh_token.as_str()),
        ("rotated refresh cookie", rotated.refresh_token.as_str()),
        ("bcrypt hash", "$2b$"),
    ];
    for (label, secret) in secrets {
        assert!(!logs.contains(secret), "{label} was logged");
    }

    t.cleanup().await;
}
