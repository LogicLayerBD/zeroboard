mod config;
mod db;
mod errors;
mod models;

use std::net::SocketAddr;

use anyhow::Context;
use axum::extract::State;
use axum::http::{HeaderName, Request, StatusCode};
use axum::routing::get;
use axum::Router;
use sqlx::SqlitePool;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::errors::AppError;

const REQUEST_ID_HEADER: &str = "x-request-id";
const DEFAULT_LOG_FILTER: &str = "zeroboard=info,tower_http=info";

#[derive(Clone)]
pub struct AppState {
    /// Write pool: all INSERT/UPDATE/DELETE (and reads that must see in-transaction writes).
    pub db: SqlitePool,
    /// Read-only pool: SELECT-only queries.
    pub read_db: SqlitePool,
    pub config: Config,
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
    let state = AppState {
        db: write_pool,
        read_db: read_pool,
        config,
    };

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    tracing::info!(%addr, "zeroboard listening");

    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;

    Ok(())
}

fn router(state: AppState) -> Router {
    let request_id_header = HeaderName::from_static(REQUEST_ID_HEADER);

    Router::new()
        .route("/live", get(live))
        .route("/ready", get(ready))
        .fallback(|| async { AppError::NotFound })
        .with_state(state)
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
        AppState {
            db,
            read_db,
            config,
        }
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
    async fn unknown_route_returns_json_404() {
        let (status, _) = get_status(test_state(std::env::temp_dir()).await, "/api/nope").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
