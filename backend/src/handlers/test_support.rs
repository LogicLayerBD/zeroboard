//! Harness for handler integration tests: a temp-file database, the full router,
//! and users with ready-made access tokens.

use std::path::PathBuf;

use axum::body::{to_bytes, Body};
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use serde_json::Value;
use tower::ServiceExt;

use crate::config::Config;
use crate::AppState;

const MAX_TEST_BODY_BYTES: usize = 1024 * 1024;
const TEST_PASSWORD: &str = "correct-horse";

pub struct TestUser {
    pub id: String,
    pub email: String,
    pub token: String,
}

pub struct TestApp {
    app: Router,
    pub state: AppState,
    dir: PathBuf,
}

impl TestApp {
    pub async fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("zeroboard-handlers-{}", uuid::Uuid::new_v4()));
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
            first_user_is_admin: true,
        };
        let state = AppState::new(db, read_db, config);
        let app = crate::router(state.clone());
        Self { app, state, dir }
    }

    /// Registers a user directly through the auth service and signs an access token.
    pub async fn user(&self, email: &str) -> TestUser {
        let user = crate::auth::service::register(&self.state, email, "Test User", TEST_PASSWORD)
            .await
            .unwrap();
        let token = crate::auth::jwt::generate_access_token(
            &user.id,
            &user.email,
            &self.state.config.jwt_secret,
            self.state.config.jwt_expiry_minutes,
        )
        .unwrap();
        TestUser {
            id: user.id,
            email: user.email,
            token,
        }
    }

    pub async fn send(
        &self,
        method: Method,
        uri: &str,
        user: Option<&TestUser>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(uri);
        if let Some(user) = user {
            req = req.header(AUTHORIZATION, format!("Bearer {}", user.token));
        }
        let req = match body {
            Some(body) => req
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string())),
            None => req.body(Body::empty()),
        }
        .unwrap();

        let response = self.app.clone().oneshot(req).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), MAX_TEST_BODY_BYTES)
            .await
            .unwrap();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, body)
    }

    pub async fn get(&self, uri: &str, user: &TestUser) -> (StatusCode, Value) {
        self.send(Method::GET, uri, Some(user), None).await
    }

    pub async fn post(&self, uri: &str, user: &TestUser, body: Value) -> (StatusCode, Value) {
        self.send(Method::POST, uri, Some(user), Some(body)).await
    }

    pub async fn patch(&self, uri: &str, user: &TestUser, body: Value) -> (StatusCode, Value) {
        self.send(Method::PATCH, uri, Some(user), Some(body)).await
    }

    pub async fn delete(&self, uri: &str, user: &TestUser) -> (StatusCode, Value) {
        self.send(Method::DELETE, uri, Some(user), None).await
    }

    /// Creates a workspace owned (as admin) by `owner` and returns its id.
    pub async fn workspace(&self, owner: &TestUser, name: &str) -> String {
        let (status, body) = self
            .post(
                "/api/workspaces",
                owner,
                serde_json::json!({ "name": name }),
            )
            .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "create workspace failed: {body}"
        );
        body["id"].as_str().unwrap().to_string()
    }

    /// Adds `user` to the workspace with `role` via the invite endpoint.
    pub async fn add_member(
        &self,
        workspace_id: &str,
        admin: &TestUser,
        user: &TestUser,
        role: &str,
    ) {
        let (status, body) = self
            .post(
                &format!("/api/workspaces/{workspace_id}/members/invite"),
                admin,
                serde_json::json!({ "email": user.email, "role": role }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "invite failed: {body}");
    }

    pub async fn cleanup(self) {
        self.state.db.close().await;
        self.state.read_db.close().await;
        let _ = tokio::fs::remove_dir_all(&self.dir).await;
    }
}
