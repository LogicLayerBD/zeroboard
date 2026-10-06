use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, patch, post};
use axum::{middleware, Extension, Json, Router};
use serde::Deserialize;

use super::auth::{is_valid_email, normalize_email};
use super::{parse_id, validate_name};
use crate::auth::{require_auth, AuthUser};
use crate::errors::AppError;
use crate::models::UserRole;
use crate::services::access::require_global_admin;
use crate::services::{admin, users};
use crate::AppState;

const USER_ID_FIELD: &str = "user id";

pub fn router(state: &AppState) -> Router<AppState> {
    Router::new()
        .route("/api/admin/info", get(server_info))
        .route("/api/admin/storage", get(storage_usage))
        .route("/api/admin/users", get(list_users).post(create_user))
        .route("/api/admin/users/:id/role", patch(change_user_role))
        .route("/api/admin/users/:id/reset-password", post(reset_password))
        .route("/api/admin/users/:id/deactivate", post(deactivate_user))
        .route("/api/admin/users/:id/reactivate", post(reactivate_user))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth))
}

#[derive(Deserialize)]
pub struct CreateUserRequest {
    email: String,
    name: String,
}

impl CreateUserRequest {
    fn validate(self) -> Result<(String, String), AppError> {
        let email = normalize_email(&self.email);
        if !is_valid_email(&email) {
            return Err(AppError::BadRequest("email is invalid".into()));
        }
        Ok((email, validate_name(&self.name)?))
    }
}

#[derive(Deserialize)]
pub struct ChangeUserRoleRequest {
    role: UserRole,
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

pub async fn list_users(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<impl IntoResponse, AppError> {
    require_global_admin(&state, &auth_user.id).await?;
    Ok(Json(users::list(&state).await?))
}

pub async fn create_user(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    body: Result<Json<CreateUserRequest>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let Json(body) = body?;
    let (email, name) = body.validate()?;
    require_global_admin(&state, &auth_user.id).await?;
    Ok((
        StatusCode::CREATED,
        Json(users::create(&state, &email, &name).await?),
    ))
}

pub async fn change_user_role(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
    body: Result<Json<ChangeUserRoleRequest>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let user_id = parse_id(&id, USER_ID_FIELD)?;
    let Json(body) = body?;
    require_global_admin(&state, &auth_user.id).await?;
    Ok(Json(
        users::set_role(&state, &auth_user.id, &user_id, body.role).await?,
    ))
}

pub async fn reset_password(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let user_id = parse_id(&id, USER_ID_FIELD)?;
    require_global_admin(&state, &auth_user.id).await?;
    Ok(Json(
        users::reset_password(&state, &auth_user.id, &user_id).await?,
    ))
}

pub async fn deactivate_user(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let user_id = parse_id(&id, USER_ID_FIELD)?;
    require_global_admin(&state, &auth_user.id).await?;
    Ok(Json(
        users::set_active(&state, &auth_user.id, &user_id, false).await?,
    ))
}

pub async fn reactivate_user(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let user_id = parse_id(&id, USER_ID_FIELD)?;
    require_global_admin(&state, &auth_user.id).await?;
    Ok(Json(
        users::set_active(&state, &auth_user.id, &user_id, true).await?,
    ))
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use serde_json::json;

    use super::super::test_support::{BoardFixture, TEST_PASSWORD};
    use crate::auth::service as auth_service;
    use crate::errors::AppError;

    const TS: i64 = 1_800_000_000_000;
    const MISSING_USER_ID: &str = "00000000-0000-4000-8000-000000000000";

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

    #[tokio::test]
    async fn user_admin_endpoints_require_instance_admin() {
        let f = BoardFixture::new().await;
        let target = format!("/api/admin/users/{}", f.outsider.id);

        // f.member is a workspace member, not an instance admin.
        let u = &f.member;
        assert_eq!(f.t.get("/api/admin/users", u).await.0, StatusCode::FORBIDDEN);
        let new_user = json!({ "email": "new@example.com", "name": "New" });
        assert_eq!(
            f.t.post("/api/admin/users", u, new_user).await.0,
            StatusCode::FORBIDDEN
        );
        let role = json!({ "role": "admin" });
        assert_eq!(
            f.t.patch(&format!("{target}/role"), u, role).await.0,
            StatusCode::FORBIDDEN
        );
        for action in ["reset-password", "deactivate", "reactivate"] {
            let (status, _) = f.t.post(&format!("{target}/{action}"), u, json!({})).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{action}");
        }

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn lists_users_with_workspace_counts_and_changes_roles() {
        let f = BoardFixture::new().await;

        let (status, users) = f.t.get("/api/admin/users", &f.admin).await;
        assert_eq!(status, StatusCode::OK, "{users}");
        let summary: Vec<_> = users
            .as_array()
            .unwrap()
            .iter()
            .map(|u| (u["email"].clone(), u["role"].clone(), u["workspace_count"].clone()))
            .collect();
        assert_eq!(
            summary,
            vec![
                (json!("admin@example.com"), json!("admin"), json!(1)),
                (json!("member@example.com"), json!("member"), json!(1)),
                (json!("viewer@example.com"), json!("member"), json!(1)),
                (json!("outsider@example.com"), json!("member"), json!(0)),
            ]
        );
        assert!(users[0]["deactivated_at"].is_null());
        assert!(users[0].get("password").is_none());

        let promote = format!("/api/admin/users/{}/role", f.member.id);
        let (status, user) = f.t.patch(&promote, &f.admin, json!({ "role": "admin" })).await;
        assert_eq!(status, StatusCode::OK, "{user}");
        assert_eq!(user["role"], "admin");
        // The promoted user now has instance-admin access.
        assert_eq!(f.t.get("/api/admin/users", &f.member).await.0, StatusCode::OK);

        let own_role = format!("/api/admin/users/{}/role", f.admin.id);
        let (status, body) = f.t.patch(&own_role, &f.admin, json!({ "role": "member" })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "you cannot change your own instance role");

        let missing = format!("/api/admin/users/{MISSING_USER_ID}/role");
        let role = json!({ "role": "member" });
        assert_eq!(f.t.patch(&missing, &f.admin, role).await.0, StatusCode::NOT_FOUND);
        let bad_role = json!({ "role": "owner" });
        assert_eq!(
            f.t.patch(&promote, &f.admin, bad_role).await.0,
            StatusCode::BAD_REQUEST
        );
        let bad_id = json!({ "role": "member" });
        assert_eq!(
            f.t.patch("/api/admin/users/not-a-uuid/role", &f.admin, bad_id).await.0,
            StatusCode::BAD_REQUEST
        );

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn created_and_reset_users_sign_in_with_the_temporary_password() {
        let f = BoardFixture::new().await;

        let new_user = json!({ "email": " New@Example.com ", "name": "New Person" });
        let (status, created) = f.t.post("/api/admin/users", &f.admin, new_user).await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
        assert_eq!(created["user"]["email"], "new@example.com");
        assert_eq!(created["user"]["role"], "member");
        let temporary = created["temporary_password"].as_str().unwrap().to_string();
        assert_eq!(temporary.len(), auth_service::TEMPORARY_PASSWORD_LEN);
        auth_service::login(&f.t.state, "new@example.com", &temporary)
            .await
            .expect("temporary password signs in");

        let duplicate = json!({ "email": "new@example.com", "name": "Again" });
        assert_eq!(
            f.t.post("/api/admin/users", &f.admin, duplicate).await.0,
            StatusCode::BAD_REQUEST
        );
        let invalid = json!({ "email": "not-an-email", "name": "X" });
        assert_eq!(
            f.t.post("/api/admin/users", &f.admin, invalid).await.0,
            StatusCode::BAD_REQUEST
        );

        auth_service::login(&f.t.state, &f.member.email, TEST_PASSWORD)
            .await
            .unwrap();
        let reset = format!("/api/admin/users/{}/reset-password", f.member.id);
        let (status, body) = f.t.post(&reset, &f.admin, json!({})).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let new_password = body["temporary_password"].as_str().unwrap();
        assert!(matches!(
            auth_service::login(&f.t.state, &f.member.email, TEST_PASSWORD).await,
            Err(AppError::Unauthorized)
        ));
        auth_service::login(&f.t.state, &f.member.email, new_password)
            .await
            .expect("reset password signs in");
        let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM refresh_tokens WHERE user_id = $1")
            .bind(&f.member.id)
            .fetch_one(&f.t.state.db)
            .await
            .unwrap();
        assert_eq!(sessions, 1, "only the session created after the reset survives");

        let own_reset = format!("/api/admin/users/{}/reset-password", f.admin.id);
        assert_eq!(
            f.t.post(&own_reset, &f.admin, json!({})).await.0,
            StatusCode::BAD_REQUEST
        );
        let missing = format!("/api/admin/users/{MISSING_USER_ID}/reset-password");
        assert_eq!(
            f.t.post(&missing, &f.admin, json!({})).await.0,
            StatusCode::NOT_FOUND
        );

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn deactivated_users_are_locked_out_until_reactivated() {
        let f = BoardFixture::new().await;
        assert_eq!(f.t.get("/api/workspaces", &f.outsider).await.0, StatusCode::OK);
        auth_service::login(&f.t.state, &f.outsider.email, TEST_PASSWORD)
            .await
            .unwrap();

        let deactivate = format!("/api/admin/users/{}/deactivate", f.outsider.id);
        let (status, user) = f.t.post(&deactivate, &f.admin, json!({})).await;
        assert_eq!(status, StatusCode::OK, "{user}");
        let deactivated_at = user["deactivated_at"].as_i64().expect("timestamp set");

        assert_eq!(
            f.t.get("/api/workspaces", &f.outsider).await.0,
            StatusCode::UNAUTHORIZED
        );
        assert!(matches!(
            auth_service::login(&f.t.state, &f.outsider.email, TEST_PASSWORD).await,
            Err(AppError::ForbiddenReason(_))
        ));
        let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM refresh_tokens WHERE user_id = $1")
            .bind(&f.outsider.id)
            .fetch_one(&f.t.state.db)
            .await
            .unwrap();
        assert_eq!(sessions, 0);

        // Deactivating twice keeps the original timestamp.
        let (_, again) = f.t.post(&deactivate, &f.admin, json!({})).await;
        assert_eq!(again["deactivated_at"], deactivated_at);

        let invite = format!("/api/workspaces/{}/members/invite", f.workspace_id);
        let body = json!({ "email": f.outsider.email, "role": "member" });
        let (status, err) = f.t.post(&invite, &f.admin, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(err["error"], "that user is deactivated");

        let own = format!("/api/admin/users/{}/deactivate", f.admin.id);
        assert_eq!(f.t.post(&own, &f.admin, json!({})).await.0, StatusCode::BAD_REQUEST);

        let reactivate = format!("/api/admin/users/{}/reactivate", f.outsider.id);
        let (status, user) = f.t.post(&reactivate, &f.admin, json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert!(user["deactivated_at"].is_null());
        assert_eq!(f.t.get("/api/workspaces", &f.outsider).await.0, StatusCode::OK);
        auth_service::login(&f.t.state, &f.outsider.email, TEST_PASSWORD)
            .await
            .expect("reactivated user signs in");

        f.t.cleanup().await;
    }
}
