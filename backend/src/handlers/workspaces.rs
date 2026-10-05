use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, patch, post};
use axum::{middleware, Extension, Json, Router};
use serde::Deserialize;

use super::auth::{is_valid_email, normalize_email};
use super::{parse_id, validate_name};
use crate::auth::{require_auth, AuthUser};
use crate::errors::AppError;
use crate::models::WorkspaceRole;
use crate::services::access::require_workspace_role;
use crate::services::workspaces;
use crate::AppState;

const WORKSPACE_ID_FIELD: &str = "workspace id";
const USER_ID_FIELD: &str = "user id";
const DEFAULT_INVITE_ROLE: WorkspaceRole = WorkspaceRole::Member;

pub fn router(state: &AppState) -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces",
            get(list_workspaces).post(create_workspace),
        )
        .route(
            "/api/workspaces/:id",
            patch(rename_workspace).delete(delete_workspace),
        )
        .route("/api/workspaces/:id/members", get(list_members))
        .route("/api/workspaces/:id/members/invite", post(invite_member))
        .route(
            "/api/workspaces/:id/members/:user_id",
            delete(remove_member),
        )
        .route(
            "/api/workspaces/:id/members/:user_id/role",
            patch(change_member_role),
        )
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth))
}

#[derive(Deserialize)]
pub struct WorkspaceNameRequest {
    name: String,
}

#[derive(Deserialize)]
pub struct InviteMemberRequest {
    email: String,
    #[serde(default)]
    role: Option<WorkspaceRole>,
}

impl InviteMemberRequest {
    fn validate(self) -> Result<(String, WorkspaceRole), AppError> {
        let email = normalize_email(&self.email);
        if !is_valid_email(&email) {
            return Err(AppError::BadRequest("email is invalid".into()));
        }
        Ok((email, self.role.unwrap_or(DEFAULT_INVITE_ROLE)))
    }
}

#[derive(Deserialize)]
pub struct ChangeRoleRequest {
    role: WorkspaceRole,
}

pub async fn create_workspace(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    body: Result<Json<WorkspaceNameRequest>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let Json(body) = body?;
    let name = validate_name(&body.name)?;
    let workspace = workspaces::create(&state, &auth_user.id, &name).await?;
    Ok((StatusCode::CREATED, Json(workspace)))
}

pub async fn list_workspaces(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(
        workspaces::list_for_user(&state, &auth_user.id).await?,
    ))
}

pub async fn rename_workspace(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
    body: Result<Json<WorkspaceNameRequest>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let workspace_id = parse_id(&id, WORKSPACE_ID_FIELD)?;
    let Json(body) = body?;
    let name = validate_name(&body.name)?;
    require_workspace_role(&state, &workspace_id, &auth_user.id, WorkspaceRole::Admin).await?;
    Ok(Json(
        workspaces::rename(&state, &workspace_id, &name).await?,
    ))
}

pub async fn delete_workspace(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let workspace_id = parse_id(&id, WORKSPACE_ID_FIELD)?;
    require_workspace_role(&state, &workspace_id, &auth_user.id, WorkspaceRole::Admin).await?;
    workspaces::delete(&state, &workspace_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_members(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let workspace_id = parse_id(&id, WORKSPACE_ID_FIELD)?;
    require_workspace_role(&state, &workspace_id, &auth_user.id, WorkspaceRole::Viewer).await?;
    Ok(Json(workspaces::list_members(&state, &workspace_id).await?))
}

pub async fn invite_member(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
    body: Result<Json<InviteMemberRequest>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let workspace_id = parse_id(&id, WORKSPACE_ID_FIELD)?;
    let Json(body) = body?;
    let (email, role) = body.validate()?;
    require_workspace_role(&state, &workspace_id, &auth_user.id, WorkspaceRole::Admin).await?;
    let member = workspaces::invite(&state, &workspace_id, &email, role).await?;
    Ok((StatusCode::CREATED, Json(member)))
}

pub async fn remove_member(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((id, user_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, AppError> {
    let workspace_id = parse_id(&id, WORKSPACE_ID_FIELD)?;
    let user_id = parse_id(&user_id, USER_ID_FIELD)?;
    require_workspace_role(&state, &workspace_id, &auth_user.id, WorkspaceRole::Admin).await?;
    workspaces::remove_member(&state, &workspace_id, &user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn change_member_role(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((id, user_id)): Path<(String, String)>,
    body: Result<Json<ChangeRoleRequest>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let workspace_id = parse_id(&id, WORKSPACE_ID_FIELD)?;
    let user_id = parse_id(&user_id, USER_ID_FIELD)?;
    let Json(body) = body?;
    require_workspace_role(&state, &workspace_id, &auth_user.id, WorkspaceRole::Admin).await?;
    let member = workspaces::change_role(&state, &workspace_id, &user_id, body.role).await?;
    Ok(Json(member))
}

#[cfg(test)]
mod tests {
    use axum::http::Method;
    use serde_json::{json, Value};

    use super::super::test_support::TestApp;
    use super::*;

    const MISSING_ID: &str = "00000000-0000-4000-8000-000000000000";
    const MAX_NAME_CHARS: usize = 255;

    fn role_of(members: &Value, user_id: &str) -> Option<String> {
        members
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["user_id"] == user_id)
            .map(|m| m["role"].as_str().unwrap().to_string())
    }

    #[tokio::test]
    async fn create_makes_creator_admin_and_list_is_scoped_to_member() {
        let t = TestApp::new().await;
        let alice = t.user("alice@example.com").await;
        let bob = t.user("bob@example.com").await;

        let (status, created) = t
            .post("/api/workspaces", &alice, json!({ "name": "  Acme  " }))
            .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(created["name"], "Acme");
        assert_eq!(created["created_by"], alice.id.as_str());
        let ws = created["id"].as_str().unwrap();

        let (_, members) = t
            .get(&format!("/api/workspaces/{ws}/members"), &alice)
            .await;
        assert_eq!(members.as_array().unwrap().len(), 1);
        assert_eq!(role_of(&members, &alice.id).as_deref(), Some("admin"));
        assert_eq!(members[0]["email"], "alice@example.com");
        assert!(members[0].get("password").is_none());

        t.workspace(&bob, "Bob's").await;
        let (status, listed) = t.get("/api/workspaces", &alice).await;
        assert_eq!(status, StatusCode::OK);
        let names: Vec<&str> = listed
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["Acme"]);

        t.cleanup().await;
    }

    #[tokio::test]
    async fn workspace_routes_require_authentication() {
        let t = TestApp::new().await;
        let (status, _) = t.send(Method::GET, "/api/workspaces", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _) = t
            .send(
                Method::POST,
                "/api/workspaces",
                None,
                Some(json!({ "name": "x" })),
            )
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        t.cleanup().await;
    }

    #[tokio::test]
    async fn workspace_name_must_be_1_to_255_chars() {
        let t = TestApp::new().await;
        let alice = t.user("alice@example.com").await;

        for bad in [
            json!({ "name": "" }),
            json!({ "name": "   " }),
            json!({ "name": "a".repeat(MAX_NAME_CHARS + 1) }),
            json!({}),
        ] {
            let (status, _) = t.post("/api/workspaces", &alice, bad.clone()).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "expected 400 for {bad}");
        }
        let (status, _) = t
            .post(
                "/api/workspaces",
                &alice,
                json!({ "name": "a".repeat(MAX_NAME_CHARS) }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);

        let ws = t.workspace(&alice, "W").await;
        let (status, _) = t
            .patch(
                &format!("/api/workspaces/{ws}"),
                &alice,
                json!({ "name": " " }),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        t.cleanup().await;
    }

    #[tokio::test]
    async fn rename_is_admin_only_with_403_vs_404() {
        let t = TestApp::new().await;
        let alice = t.user("alice@example.com").await;
        let bob = t.user("bob@example.com").await;
        let eve = t.user("eve@example.com").await;
        let ws = t.workspace(&alice, "Old").await;
        t.add_member(&ws, &alice, &bob, "member").await;
        let uri = format!("/api/workspaces/{ws}");
        let body = json!({ "name": "New" });

        assert_eq!(
            t.patch(&uri, &bob, body.clone()).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            t.patch(&uri, &eve, body.clone()).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            t.patch(
                &format!("/api/workspaces/{MISSING_ID}"),
                &alice,
                body.clone()
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            t.patch("/api/workspaces/not-a-uuid", &alice, body.clone())
                .await
                .0,
            StatusCode::BAD_REQUEST
        );

        let (status, renamed) = t.patch(&uri, &alice, body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(renamed["name"], "New");
        assert!(renamed["updated_at"].as_i64() >= renamed["created_at"].as_i64());

        t.cleanup().await;
    }

    #[tokio::test]
    async fn members_list_is_visible_to_any_member_but_not_outsiders() {
        let t = TestApp::new().await;
        let alice = t.user("alice@example.com").await;
        let viewer = t.user("viewer@example.com").await;
        let eve = t.user("eve@example.com").await;
        let ws = t.workspace(&alice, "W").await;
        t.add_member(&ws, &alice, &viewer, "viewer").await;
        let uri = format!("/api/workspaces/{ws}/members");

        let (status, members) = t.get(&uri, &viewer).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(role_of(&members, &viewer.id).as_deref(), Some("viewer"));
        assert_eq!(t.get(&uri, &eve).await.0, StatusCode::FORBIDDEN);
        assert_eq!(
            t.get(&format!("/api/workspaces/{MISSING_ID}/members"), &alice)
                .await
                .0,
            StatusCode::NOT_FOUND
        );

        t.cleanup().await;
    }

    #[tokio::test]
    async fn invite_validates_email_existence_and_duplicates() {
        let t = TestApp::new().await;
        let alice = t.user("alice@example.com").await;
        let bob = t.user("bob@example.com").await;
        let carol = t.user("carol@example.com").await;
        let ws = t.workspace(&alice, "W").await;
        let uri = format!("/api/workspaces/{ws}/members/invite");

        let (status, member) = t
            .post(&uri, &alice, json!({ "email": "  BOB@Example.com " }))
            .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(member["user_id"], bob.id.as_str());
        assert_eq!(member["role"], "member", "role defaults to member");

        let (status, body) = t
            .post(&uri, &alice, json!({ "email": "bob@example.com" }))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "user is already a member of this workspace");

        let (status, body) = t
            .post(&uri, &alice, json!({ "email": "nobody@example.com" }))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "no user is registered with that email");

        for bad in [
            json!({ "email": "not-an-email" }),
            json!({ "email": "c@d.co", "role": "owner" }),
            json!({}),
        ] {
            assert_eq!(
                t.post(&uri, &alice, bad.clone()).await.0,
                StatusCode::BAD_REQUEST,
                "{bad}"
            );
        }

        let (status, _) = t.post(&uri, &bob, json!({ "email": carol.email })).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "members cannot invite");
        let (status, _) = t
            .post(
                &format!("/api/workspaces/{MISSING_ID}/members/invite"),
                &alice,
                json!({ "email": carol.email }),
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        t.cleanup().await;
    }

    #[tokio::test]
    async fn role_change_cannot_demote_last_admin() {
        let t = TestApp::new().await;
        let alice = t.user("alice@example.com").await;
        let bob = t.user("bob@example.com").await;
        let ws = t.workspace(&alice, "W").await;
        t.add_member(&ws, &alice, &bob, "member").await;
        let role_uri = |user_id: &str| format!("/api/workspaces/{ws}/members/{user_id}/role");

        let (status, body) = t
            .patch(&role_uri(&alice.id), &alice, json!({ "role": "member" }))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "cannot demote the last admin of a workspace");

        assert_eq!(
            t.patch(&role_uri(&alice.id), &bob, json!({ "role": "viewer" }))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            t.patch(&role_uri(MISSING_ID), &alice, json!({ "role": "viewer" }))
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            t.patch(&role_uri(&bob.id), &alice, json!({ "role": "owner" }))
                .await
                .0,
            StatusCode::BAD_REQUEST
        );

        let (status, promoted) = t
            .patch(&role_uri(&bob.id), &alice, json!({ "role": "admin" }))
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(promoted["role"], "admin");

        let (status, demoted) = t
            .patch(&role_uri(&alice.id), &alice, json!({ "role": "viewer" }))
            .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "demotion allowed once another admin exists"
        );
        assert_eq!(demoted["role"], "viewer");

        let (status, _) = t
            .patch(&role_uri(&bob.id), &bob, json!({ "role": "member" }))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "bob is now the last admin");

        t.cleanup().await;
    }

    #[tokio::test]
    async fn remove_member_is_admin_only_and_keeps_last_admin() {
        let t = TestApp::new().await;
        let alice = t.user("alice@example.com").await;
        let bob = t.user("bob@example.com").await;
        let ws = t.workspace(&alice, "W").await;
        t.add_member(&ws, &alice, &bob, "member").await;
        let member_uri = |user_id: &str| format!("/api/workspaces/{ws}/members/{user_id}");

        assert_eq!(
            t.delete(&member_uri(&alice.id), &bob).await.0,
            StatusCode::FORBIDDEN
        );
        let (status, body) = t.delete(&member_uri(&alice.id), &alice).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "cannot remove the last admin of a workspace");
        assert_eq!(
            t.delete(&member_uri(MISSING_ID), &alice).await.0,
            StatusCode::NOT_FOUND
        );

        assert_eq!(
            t.delete(&member_uri(&bob.id), &alice).await.0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            t.get(&format!("/api/workspaces/{ws}/members"), &bob)
                .await
                .0,
            StatusCode::FORBIDDEN,
            "removed member loses access"
        );
        assert_eq!(
            t.delete(&member_uri(&bob.id), &alice).await.0,
            StatusCode::NOT_FOUND
        );

        t.cleanup().await;
    }

    #[tokio::test]
    async fn delete_is_admin_only_and_archives_boards() {
        let t = TestApp::new().await;
        let alice = t.user("alice@example.com").await;
        let bob = t.user("bob@example.com").await;
        let ws = t.workspace(&alice, "W").await;
        t.add_member(&ws, &alice, &bob, "member").await;
        let (status, board) = t
            .post(
                &format!("/api/workspaces/{ws}/boards"),
                &alice,
                json!({ "name": "B" }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        let board_id = board["id"].as_str().unwrap().to_string();
        let uri = format!("/api/workspaces/{ws}");

        assert_eq!(t.delete(&uri, &bob).await.0, StatusCode::FORBIDDEN);
        assert_eq!(
            t.delete(&format!("/api/workspaces/{MISSING_ID}"), &alice)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(t.delete(&uri, &alice).await.0, StatusCode::NO_CONTENT);

        let row = sqlx::query!(
            r#"SELECT workspace_id, archived AS "archived: bool" FROM boards WHERE id = $1"#,
            board_id
        )
        .fetch_one(&t.state.db)
        .await
        .unwrap();
        assert_eq!(
            (row.workspace_id, row.archived),
            (None, true),
            "board kept but archived"
        );

        assert_eq!(t.delete(&uri, &alice).await.0, StatusCode::NOT_FOUND);
        assert_eq!(
            t.get(&format!("/api/boards/{board_id}"), &alice).await.0,
            StatusCode::NOT_FOUND
        );
        let (_, listed) = t.get("/api/workspaces", &alice).await;
        assert_eq!(listed, json!([]));

        t.cleanup().await;
    }
}
