use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{middleware, Extension, Json, Router};

use crate::auth::{require_auth, AuthUser};
use crate::errors::AppError;
use crate::services::access::require_global_admin;
use crate::services::admin;
use crate::AppState;

pub fn router(state: &AppState) -> Router<AppState> {
    Router::new()
        .route("/api/admin/info", get(server_info))
        .route("/api/admin/storage", get(storage_usage))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth))
}

pub async fn server_info(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<impl IntoResponse, AppError> {
    require_global_admin(&state, &auth_user.id).await?;
    Ok(Json(admin::server_info(&state).await?))
}

pub async fn storage_usage(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<impl IntoResponse, AppError> {
    require_global_admin(&state, &auth_user.id).await?;
    Ok(Json(admin::storage_usage(&state).await?))
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use serde_json::json;

    use super::super::test_support::BoardFixture;

    const TS: i64 = 1_800_000_000_000;

    async fn seed_attachment(f: &BoardFixture, card_id: &str, size_bytes: i64) {
        let id = uuid::Uuid::new_v4().to_string();
        let stored_path = format!("{card_id}/{id}_f.txt");
        sqlx::query!(
            "INSERT INTO attachments (id, card_id, filename, stored_path, size_bytes, uploaded_by, uploaded_at)
             VALUES ($1, $2, 'f.txt', $3, $4, $5, $6)",
            id,
            card_id,
            stored_path,
            size_bytes,
            f.member.id,
            TS
        )
        .execute(&f.t.state.db)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn info_requires_global_admin_and_reports_server_stats() {
        let f = BoardFixture::new().await;

        let (status, info) = f.t.get("/api/admin/info", &f.admin).await;
        assert_eq!(status, StatusCode::OK, "{info}");
        assert_eq!(info["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(info["user_count"], 4);
        assert!(info["db_size_bytes"].as_u64().unwrap() > 0);
        assert!(info["uptime_seconds"].is_u64());

        // Workspace admin rights do not grant instance admin.
        let other_ws = f.t.workspace(&f.member, "Mine").await;
        assert!(!other_ws.is_empty());
        for user in [&f.member, &f.viewer, &f.outsider] {
            assert_eq!(
                f.t.get("/api/admin/info", user).await.0,
                StatusCode::FORBIDDEN
            );
            assert_eq!(
                f.t.get("/api/admin/storage", user).await.0,
                StatusCode::FORBIDDEN
            );
        }

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn storage_breaks_down_attachment_bytes_per_workspace() {
        let f = BoardFixture::new().await;
        let (_, empty) = f.t.get("/api/admin/storage", &f.admin).await;
        assert_eq!(
            empty,
            json!({ "total_bytes": 0, "attachment_count": 0, "workspaces": [] })
        );

        let card_a = f.card("A").await;
        let card_b = f.card("B").await;
        seed_attachment(&f, &card_a, 100).await;
        seed_attachment(&f, &card_b, 50).await;

        let other_ws = f.t.workspace(&f.admin, "Other").await;
        let other_board = f.t.board(&f.admin, &other_ws, "OB").await;
        let (_, list) =
            f.t.post(
                &format!("/api/boards/{other_board}/lists"),
                &f.admin,
                json!({ "name": "L" }),
            )
            .await;
        let (_, card) =
            f.t.post(
                &format!("/api/lists/{}/cards", list["id"].as_str().unwrap()),
                &f.admin,
                json!({ "title": "C" }),
            )
            .await;
        seed_attachment(&f, card["id"].as_str().unwrap(), 400).await;

        let (status, usage) = f.t.get("/api/admin/storage", &f.admin).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(usage["total_bytes"], 550);
        assert_eq!(usage["attachment_count"], 3);
        assert_eq!(
            usage["workspaces"],
            json!([
                { "workspace_id": other_ws, "workspace_name": "Other", "attachment_count": 1, "size_bytes": 400 },
                { "workspace_id": f.workspace_id, "workspace_name": "W", "attachment_count": 2, "size_bytes": 150 }
            ])
        );

        assert_eq!(
            f.t.delete(&format!("/api/workspaces/{other_ws}"), &f.admin)
                .await
                .0,
            StatusCode::NO_CONTENT
        );
        let (_, usage) = f.t.get("/api/admin/storage", &f.admin).await;
        assert_eq!(usage["total_bytes"], 550);
        assert_eq!(
            usage["workspaces"][0],
            json!({ "workspace_id": null, "workspace_name": null, "attachment_count": 1, "size_bytes": 400 })
        );

        f.t.cleanup().await;
    }
}
