mod auth;
mod config;
mod db;
mod errors;
mod frontend;
mod handlers;
mod models;
mod services;
mod ws;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Context;
use axum::extract::State;
use axum::http::header::CACHE_CONTROL;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use axum::routing::get;
use axum::Router;
use sqlx::SqlitePool;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::auth::rate_limit::{
    self, RateLimiter, LOGIN_MAX_ATTEMPTS, LOGIN_WINDOW, REGISTER_MAX_ATTEMPTS, REGISTER_WINDOW,
};
use crate::config::Config;
use crate::ws::WsHub;

const REQUEST_ID_HEADER: &str = "x-request-id";
const DEFAULT_LOG_FILTER: &str = "zeroboard=info,tower_http=info";

const CONTENT_SECURITY_POLICY_HEADER: &str = "content-security-policy";
/// Sent on every response (see security rules), together with the CSP from
/// `content_security_policy()`.
const SECURITY_HEADERS: &[(&str, &str)] = &[
    ("x-content-type-options", "nosniff"),
    ("x-frame-options", "DENY"),
    ("x-xss-protection", "1; mode=block"),
    ("referrer-policy", "strict-origin-when-cross-origin"),
    // Browsers ignore HSTS over plain HTTP; the refresh cookie is `Secure` anyway.
    ("strict-transport-security", "max-age=31536000"),
    ("permissions-policy", "camera=(), microphone=(), geolocation=()"),
    ("cross-origin-opener-policy", "same-origin"),
    ("cross-origin-resource-policy", "same-origin"),
];
/// API responses carry user data; handlers may set their own Cache-Control.
const DEFAULT_CACHE_CONTROL: &str = "no-store";

#[derive(Clone)]
pub struct AppState {
    /// Write pool: all INSERT/UPDATE/DELETE (and reads that must see in-transaction writes).
    pub db: SqlitePool,
    /// Read-only pool: SELECT-only queries.
    pub read_db: SqlitePool,
    pub config: Config,
    pub login_limiter: Arc<RateLimiter>,
    pub register_limiter: Arc<RateLimiter>,
    /// WebSocket connection manager.
    pub ws_hub: Arc<WsHub>,
    /// Process start, for uptime reporting.
    pub started_at: Instant,
}

impl AppState {
    pub fn new(db: SqlitePool, read_db: SqlitePool, config: Config) -> Self {
        Self {
            db,
            read_db,
            config,
            login_limiter: Arc::new(RateLimiter::new(LOGIN_MAX_ATTEMPTS, LOGIN_WINDOW)),
            register_limiter: Arc::new(RateLimiter::new(REGISTER_MAX_ATTEMPTS, REGISTER_WINDOW)),
            ws_hub: Arc::new(WsHub::new()),
            started_at: Instant::now(),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER)),
        )
        .init();

    let config = Config::from_env().context("failed to load configuration")?;
    tracing::info!(?config, "configuration loaded");

    tokio::fs::create_dir_all(&config.attachments_dir)
        .await
        .with_context(|| {
            format!(
                "failed to create attachments directory {}",
                config.attachments_dir.display()
            )
        })?;

    let write_pool = db::connect(&config.database_url).await?;
    db::run_migrations(&write_pool).await?;
    let read_pool = db::create_read_pool(&config.database_url).await?;
    let addr = SocketAddr::new(config.host, config.port);
    let state = AppState::new(write_pool, read_pool, config);
    rate_limit::spawn_pruner(vec![
        state.login_limiter.clone(),
        state.register_limiter.clone(),
    ]);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    tracing::info!(%addr, "zeroboard listening");

    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;

    Ok(())
}

fn router(state: AppState) -> Router {
    let request_id_header = HeaderName::from_static(REQUEST_ID_HEADER);

    let app = Router::new()
        .route("/live", get(live))
        .route("/ready", get(ready))
        .merge(handlers::auth::router(&state))
        .merge(handlers::workspaces::router(&state))
        .merge(handlers::boards::router(&state))
        .merge(handlers::lists::router(&state))
        .merge(handlers::cards::router(&state))
        .merge(handlers::labels::router(&state))
        .merge(handlers::attachments::router(&state))
        .merge(handlers::time_entries::router(&state))
        .merge(handlers::comments::router(&state))
        .merge(handlers::notifications::router(&state))
        .merge(handlers::admin::router(&state))
        .merge(ws::router())
        .fallback(frontend::serve)
        .with_state(state);

    with_security_headers(app)
        .layer(
            TraceLayer::new_for_http().make_span_with(|req: &Request<_>| {
                let request_id = req
                    .headers()
                    .get(REQUEST_ID_HEADER)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default();
                // Path only: query strings may carry tokens (e.g. /ws?token=...).
                tracing::info_span!(
                    "request",
                    request_id = %request_id,
                    method = %req.method(),
                    path = %req.uri().path(),
                )
            }),
        )
        .layer(PropagateRequestIdLayer::new(request_id_header.clone()))
        .layer(SetRequestIdLayer::new(request_id_header, MakeRequestUuid))
}

/// `style-src 'unsafe-inline'` is required by Svelte's inline styles. Scripts stay
/// restricted to 'self' plus the hashes of SvelteKit's inline bootstrap script.
fn content_security_policy() -> String {
    let script_hashes: String = frontend::inline_script_hashes()
        .iter()
        .map(|hash| format!(" '{hash}'"))
        .collect();
    format!(
        "default-src 'self'; script-src 'self'{script_hashes}; style-src 'self' 'unsafe-inline'; \
         object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'"
    )
}

fn with_security_headers(app: Router) -> Router {
    let app = SECURITY_HEADERS.iter().fold(app, |app, (name, value)| {
        app.layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        ))
    });
    let csp = HeaderValue::from_str(&content_security_policy())
        .expect("CSP contains only header-safe characters");
    let app = app.layer(SetResponseHeaderLayer::overriding(
        HeaderName::from_static(CONTENT_SECURITY_POLICY_HEADER),
        csp,
    ));
    app.layer(SetResponseHeaderLayer::if_not_present(
        CACHE_CONTROL,
        HeaderValue::from_static(DEFAULT_CACHE_CONTROL),
    ))
}

async fn live() -> StatusCode {
    StatusCode::OK
}

async fn ready(State(state): State<AppState>) -> StatusCode {
    match check_ready(&state).await {
        Ok(()) => StatusCode::OK,
        Err(err) => {
            tracing::error!(error = ?err, "readiness check failed");
            StatusCode::SERVICE_UNAVAILABLE
        }
    }
}

async fn check_ready(state: &AppState) -> anyhow::Result<()> {
    probe_pool(&state.db).await.context("write pool unavailable")?;
    probe_pool(&state.read_db).await.context("read pool unavailable")?;

    let attachments = tokio::fs::metadata(&state.config.attachments_dir)
        .await
        .context("attachments dir unavailable")?;
    anyhow::ensure!(attachments.is_dir(), "attachments path is not a directory");

    Ok(())
}

async fn probe_pool(pool: &SqlitePool) -> anyhow::Result<()> {
    let one: i64 = sqlx::query_scalar("SELECT 1")
        .fetch_one(pool)
        .await
        .context("SELECT 1 failed")?;
    anyhow::ensure!(one == 1, "SELECT 1 returned {one}");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::error!(error = ?err, "failed to listen for ctrl-c");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(err) => tracing::error!(error = ?err, "failed to listen for SIGTERM"),
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use std::path::PathBuf;
    use tower::ServiceExt;

    async fn test_state(attachments_dir: PathBuf) -> AppState {
        let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let read_db = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let config = Config {
            host: "127.0.0.1".parse().unwrap(),
            port: 0,
            jwt_secret: "a-test-secret-that-is-at-least-32-chars".into(),
            jwt_expiry_minutes: 15,
            refresh_token_expiry_days: 30,
            database_url: "sqlite::memory:".into(),
            attachments_dir,
            max_attachment_size_mb: 25,
            app_name: "ZeroBoard".into(),
            first_user_is_admin: true,
        };
        AppState::new(db, read_db, config)
    }

    async fn get_status(state: AppState, uri: &str) -> (StatusCode, Option<String>) {
        let response = router(state)
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let request_id = response
            .headers()
            .get(REQUEST_ID_HEADER)
            .map(|v| v.to_str().unwrap().to_string());
        (response.status(), request_id)
    }

    #[tokio::test]
    async fn live_returns_ok_with_request_id() {
        let (status, request_id) = get_status(test_state(std::env::temp_dir()).await, "/live").await;
        assert_eq!(status, StatusCode::OK);
        assert!(request_id.is_some_and(|id| !id.is_empty()));
    }

    #[tokio::test]
    async fn ready_returns_ok_when_db_and_storage_available() {
        let (status, _) = get_status(test_state(std::env::temp_dir()).await, "/ready").await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn ready_fails_when_storage_missing() {
        let missing = std::env::temp_dir().join(format!("zeroboard-missing-{}", uuid::Uuid::new_v4()));
        let (status, _) = get_status(test_state(missing).await, "/ready").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn ready_fails_when_db_closed() {
        let state = test_state(std::env::temp_dir()).await;
        state.db.close().await;
        let (status, _) = get_status(state, "/ready").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn ready_fails_when_read_db_closed() {
        let state = test_state(std::env::temp_dir()).await;
        state.read_db.close().await;
        let (status, _) = get_status(state, "/ready").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn every_response_carries_security_headers() {
        let state = test_state(std::env::temp_dir()).await;
        for (uri, expected_status) in [
            ("/live", StatusCode::OK),
            ("/api/nope", StatusCode::NOT_FOUND),
            ("/api/workspaces", StatusCode::UNAUTHORIZED),
        ] {
            let response = router(state.clone())
                .oneshot(Request::get(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), expected_status, "{uri}");
            let headers = response.headers();
            for (name, value) in SECURITY_HEADERS {
                assert_eq!(headers[*name], *value, "{uri}: {name}");
            }
            assert_eq!(headers[CACHE_CONTROL], DEFAULT_CACHE_CONTROL, "{uri}");
            let csp = headers[CONTENT_SECURITY_POLICY_HEADER].to_str().unwrap();
            assert!(csp.contains("frame-ancestors 'none'"), "{uri}");
            assert!(csp.contains("script-src 'self' 'sha256-"), "{uri}");
        }
    }

    #[tokio::test]
    async fn unknown_route_returns_json_404() {
        let (status, _) = get_status(test_state(std::env::temp_dir()).await, "/api/nope").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
