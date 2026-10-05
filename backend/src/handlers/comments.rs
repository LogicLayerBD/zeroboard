use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, patch};
use axum::{middleware, Extension, Json, Router};
use serde::Deserialize;

use super::parse_id;
use crate::auth::{require_auth, AuthUser};
use crate::errors::AppError;
use crate::models::WorkspaceRole;
use crate::services::access::{require_card_access, require_card_role};
use crate::services::comments;
use crate::AppState;

const CARD_ID_FIELD: &str = "card id";
const COMMENT_ID_FIELD: &str = "comment id";

const MAX_BODY_CHARS: usize = 10_000;

pub fn router(state: &AppState) -> Router<AppState> {
    Router::new()
        .route(
            "/api/cards/:id/comments",
            get(list_comments).post(create_comment),
        )
        .route(
            "/api/comments/:id",
            patch(update_comment).delete(delete_comment),
        )
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth))
}

#[derive(Deserialize)]
pub struct CommentRequest {
    body: String,
}

/// Trims and enforces the 1..=10,000 character limit.
fn validate_body(raw: &str) -> Result<String, AppError> {
    let body = raw.trim();
    if body.is_empty() {
        return Err(AppError::BadRequest("body is required".into()));
    }
    if body.chars().count() > MAX_BODY_CHARS {
        return Err(AppError::BadRequest(format!(
            "body must be at most {MAX_BODY_CHARS} characters"
        )));
    }
    Ok(body.to_string())
}

pub async fn list_comments(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let card_id = parse_id(&id, CARD_ID_FIELD)?;
    require_card_role(&state, &card_id, &auth_user.id, WorkspaceRole::Viewer).await?;
    Ok(Json(comments::list_for_card(&state, &card_id).await?))
}

pub async fn create_comment(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
    body: Result<Json<CommentRequest>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let card_id = parse_id(&id, CARD_ID_FIELD)?;
    let Json(body) = body?;
    let text = validate_body(&body.body)?;
    let card = require_card_role(&state, &card_id, &auth_user.id, WorkspaceRole::Member).await?;
    let comment = comments::create(&state, &card, &auth_user.id, &text).await?;
    Ok((StatusCode::CREATED, Json(comment)))
}

/// Only the author may edit a comment.
pub async fn update_comment(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
    body: Result<Json<CommentRequest>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let comment_id = parse_id(&id, COMMENT_ID_FIELD)?;
    let Json(body) = body?;
    let text = validate_body(&body.body)?;
    let comment = comments::get(&state, &comment_id).await?;
    require_card_role(
        &state,
        &comment.card_id,
        &auth_user.id,
        WorkspaceRole::Member,
    )
    .await?;
    if comment.user_id != auth_user.id {
        tracing::warn!(%comment_id, user_id = %auth_user.id, "comment edit denied: not author");
        return Err(AppError::Forbidden);
    }
    Ok(Json(comments::update(&state, &comment_id, &text).await?))
}

/// The author or a workspace admin may delete a comment.
pub async fn delete_comment(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let comment_id = parse_id(&id, COMMENT_ID_FIELD)?;
    let comment = comments::get(&state, &comment_id).await?;
    let (_, role) = require_card_access(
        &state,
        &comment.card_id,
        &auth_user.id,
        WorkspaceRole::Member,
    )
    .await?;
    if comment.user_id != auth_user.id && role != WorkspaceRole::Admin {
        tracing::warn!(%comment_id, user_id = %auth_user.id, "comment delete denied: not author");
        return Err(AppError::Forbidden);
    }
    comments::delete(&state, &comment_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::super::test_support::BoardFixture;
    use super::*;

    const MISSING_ID: &str = "00000000-0000-4000-8000-000000000000";

    async fn notification_types(f: &BoardFixture, user_id: &str) -> Vec<String> {
        sqlx::query!(
            r#"SELECT type AS "kind!" FROM notifications WHERE user_id = $1 ORDER BY created_at, rowid"#,
            user_id
        )
        .fetch_all(&f.t.state.db)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.kind)
        .collect()
    }

    #[tokio::test]
    async fn create_lists_oldest_first_logs_and_notifies_other_assignees() {
        let f = BoardFixture::new().await;
        let card_id = f.card("T").await;
        f.assign(&card_id, &f.member.id).await;
        f.assign(&card_id, &f.viewer.id).await;
        let uri = format!("/api/cards/{card_id}/comments");

        let (status, first) =
            f.t.post(&uri, &f.member, json!({ "body": "  first  " }))
                .await;
        assert_eq!(status, StatusCode::CREATED, "{first}");
        assert_eq!(first["body"], "first");
        assert_eq!(first["user_id"], f.member.id.as_str());
        assert_eq!(first["card_id"], card_id.as_str());
        let (status, _) = f.t.post(&uri, &f.admin, json!({ "body": "second" })).await;
        assert_eq!(status, StatusCode::CREATED);

        let (status, list) = f.t.get(&uri, &f.viewer).await;
        assert_eq!(status, StatusCode::OK);
        let bodies: Vec<&str> = list
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["body"].as_str().unwrap())
            .collect();
        assert_eq!(bodies, ["first", "second"]);

        let logged = sqlx::query!(
            r#"SELECT COUNT(*) AS "n!: i64" FROM activity_log WHERE card_id = $1 AND action = 'added_comment'"#,
            card_id
        )
        .fetch_one(&f.t.state.db)
        .await
        .unwrap();
        assert_eq!(logged.n, 2);

        // Member authored one comment and was assigned by admin; viewer got both comments.
        assert_eq!(
            notification_types(&f, &f.member.id).await,
            ["assigned_to_card", "comment_on_assigned_card"]
        );
        assert_eq!(
            notification_types(&f, &f.viewer.id).await,
            [
                "assigned_to_card",
                "comment_on_assigned_card",
                "comment_on_assigned_card"
            ]
        );
        assert!(notification_types(&f, &f.admin.id).await.is_empty());

        for bad in [
            json!({ "body": "   " }),
            json!({ "body": "a".repeat(MAX_BODY_CHARS + 1) }),
            json!({}),
        ] {
            assert_eq!(
                f.t.post(&uri, &f.member, bad).await.0,
                StatusCode::BAD_REQUEST
            );
        }
        assert!(f
            .t
            .post(
                &uri,
                &f.member,
                json!({ "body": "a".repeat(MAX_BODY_CHARS) })
            )
            .await
            .0
            .is_success());
        let ok = json!({ "body": "x" });
        assert_eq!(
            f.t.post(&uri, &f.viewer, ok.clone()).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(f.t.get(&uri, &f.outsider).await.0, StatusCode::FORBIDDEN);
        assert_eq!(
            f.t.post(&format!("/api/cards/{MISSING_ID}/comments"), &f.member, ok)
                .await
                .0,
            StatusCode::NOT_FOUND
        );

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn only_author_edits_and_author_or_admin_deletes() {
        let f = BoardFixture::new().await;
        let card_id = f.card("T").await;
        let uri = format!("/api/cards/{card_id}/comments");
        let (_, comment) = f.t.post(&uri, &f.member, json!({ "body": "draft" })).await;
        let comment_uri = format!("/api/comments/{}", comment["id"].as_str().unwrap());

        let edit = json!({ "body": "edited" });
        assert_eq!(
            f.t.patch(&comment_uri, &f.admin, edit.clone()).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            f.t.patch(&comment_uri, &f.viewer, edit.clone()).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            f.t.patch(&comment_uri, &f.member, json!({ "body": "" }))
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
        let (status, edited) = f.t.patch(&comment_uri, &f.member, edit).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(edited["body"], "edited");
        assert!(edited["updated_at"].as_i64() >= edited["created_at"].as_i64());

        let (_, admin_comment) = f.t.post(&uri, &f.admin, json!({ "body": "admin" })).await;
        let admin_uri = format!("/api/comments/{}", admin_comment["id"].as_str().unwrap());
        assert_eq!(
            f.t.delete(&admin_uri, &f.member).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            f.t.delete(&comment_uri, &f.viewer).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            f.t.delete(&comment_uri, &f.admin).await.0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            f.t.delete(&admin_uri, &f.admin).await.0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            f.t.delete(&admin_uri, &f.admin).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            f.t.patch(
                &format!("/api/comments/{MISSING_ID}"),
                &f.member,
                json!({ "body": "x" })
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );

        let (_, list) = f.t.get(&uri, &f.member).await;
        assert_eq!(list, Value::Array(vec![]));

        f.t.cleanup().await;
    }
}
