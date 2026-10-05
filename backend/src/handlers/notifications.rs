use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::{get, patch, post};
use axum::{middleware, Extension, Json, Router};
use serde_json::json;

use super::parse_id;
use crate::auth::{require_auth, AuthUser};
use crate::errors::AppError;
use crate::services::notifications;
use crate::AppState;

const NOTIFICATION_ID_FIELD: &str = "notification id";

pub fn router(state: &AppState) -> Router<AppState> {
    Router::new()
        .route("/api/notifications", get(list_notifications))
        .route("/api/notifications/:id/read", patch(mark_read))
        .route("/api/notifications/read-all", post(mark_all_read))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth))
}

/// The caller's 50 most recent notifications, newest first.
pub async fn list_notifications(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(
        notifications::list_for_user(&state, &auth_user.id).await?,
    ))
}

pub async fn mark_read(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let notification_id = parse_id(&id, NOTIFICATION_ID_FIELD)?;
    Ok(Json(
        notifications::mark_read(&state, &auth_user.id, &notification_id).await?,
    ))
}

pub async fn mark_all_read(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> Result<impl IntoResponse, AppError> {
    let updated = notifications::mark_all_read(&state, &auth_user.id).await?;
    Ok(Json(json!({ "updated": updated })))
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use serde_json::{json, Value};

    use super::super::test_support::BoardFixture;
    use super::*;
    use crate::services::notifications::DUE_SOON_WINDOW_MS;

    const MISSING_ID: &str = "00000000-0000-4000-8000-000000000000";
    const ONE_HOUR_MS: i64 = 60 * 60 * 1000;

    fn now_ms() -> i64 {
        chrono::Utc::now().timestamp_millis()
    }

    async fn list(f: &BoardFixture, user: &super::super::test_support::TestUser) -> Vec<Value> {
        let (status, body) = f.t.get("/api/notifications", user).await;
        assert_eq!(status, StatusCode::OK);
        body.as_array().unwrap().clone()
    }

    #[tokio::test]
    async fn assignment_notifies_assignee_but_not_actor() {
        let f = BoardFixture::new().await;
        let card_id = f.card("Ship it").await;

        f.assign(&card_id, &f.member.id).await;
        f.assign(&card_id, &f.admin.id).await;

        let notes = list(&f, &f.member).await;
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0]["type"], "assigned_to_card");
        assert_eq!(notes[0]["read"], false);
        assert_eq!(notes[0]["user_id"], f.member.id.as_str());
        let payload: Value = serde_json::from_str(notes[0]["payload"].as_str().unwrap()).unwrap();
        assert_eq!(
            payload,
            json!({
                "card_id": card_id,
                "board_id": f.board_id,
                "message": "You were assigned to \"Ship it\""
            })
        );
        assert!(
            list(&f, &f.admin).await.is_empty(),
            "self-assignment is not notified"
        );

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn due_soon_update_notifies_assignees_once_per_due_window() {
        let f = BoardFixture::new().await;
        let card_id = f.card("T").await;
        f.assign(&card_id, &f.member.id).await;
        f.assign(&card_id, &f.viewer.id).await;
        let uri = format!("/api/cards/{card_id}");
        let due_types = |notes: Vec<Value>| {
            notes
                .into_iter()
                .filter(|n| n["type"] == "card_due_soon")
                .count()
        };

        let far = now_ms() + DUE_SOON_WINDOW_MS + 10 * ONE_HOUR_MS;
        f.t.patch(&uri, &f.member, json!({ "due_date": far })).await;
        assert_eq!(due_types(list(&f, &f.viewer).await), 0);

        let soon = now_ms() + 2 * ONE_HOUR_MS;
        let (status, _) =
            f.t.patch(&uri, &f.member, json!({ "due_date": soon }))
                .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(due_types(list(&f, &f.viewer).await), 1);
        assert_eq!(
            due_types(list(&f, &f.member).await),
            0,
            "actor is not notified"
        );

        f.t.patch(&uri, &f.member, json!({ "title": "Renamed" }))
            .await;
        assert_eq!(
            due_types(list(&f, &f.viewer).await),
            1,
            "no duplicate in same window"
        );

        f.t.patch(&uri, &f.admin, json!({ "title": "Again" })).await;
        assert_eq!(due_types(list(&f, &f.member).await), 1);

        let past = now_ms() - ONE_HOUR_MS;
        f.t.patch(&uri, &f.admin, json!({ "due_date": past })).await;
        assert_eq!(due_types(list(&f, &f.viewer).await), 1);

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn mark_read_is_scoped_to_owner_and_read_all_marks_everything() {
        let f = BoardFixture::new().await;
        let first = f.card("A").await;
        let second = f.card("B").await;
        f.assign(&first, &f.member.id).await;
        f.assign(&second, &f.member.id).await;

        let notes = list(&f, &f.member).await;
        assert_eq!(notes.len(), 2);
        let newest_message = serde_json::from_str::<Value>(notes[0]["payload"].as_str().unwrap())
            .unwrap()["message"]
            .clone();
        assert_eq!(newest_message, "You were assigned to \"B\"", "newest first");
        let id = notes[1]["id"].as_str().unwrap();
        let read_uri = format!("/api/notifications/{id}/read");

        assert_eq!(
            f.t.patch(&read_uri, &f.viewer, json!({})).await.0,
            StatusCode::NOT_FOUND
        );
        let (status, read) = f.t.patch(&read_uri, &f.member, json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(read["read"], true);
        assert_eq!(read["id"], id);
        assert_eq!(
            f.t.patch(
                &format!("/api/notifications/{MISSING_ID}/read"),
                &f.member,
                json!({})
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            f.t.patch("/api/notifications/nope/read", &f.member, json!({}))
                .await
                .0,
            StatusCode::BAD_REQUEST
        );

        let (status, body) =
            f.t.post("/api/notifications/read-all", &f.member, json!({}))
                .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["updated"], 1);
        assert!(list(&f, &f.member).await.iter().all(|n| n["read"] == true));

        let (status, _) =
            f.t.send(axum::http::Method::GET, "/api/notifications", None, None)
                .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn list_is_capped_at_fifty() {
        let f = BoardFixture::new().await;
        let card_id = f.card("T").await;
        let card = crate::services::access::require_card_role(
            &f.t.state,
            &card_id,
            &f.admin.id,
            crate::models::WorkspaceRole::Viewer,
        )
        .await
        .unwrap();
        for _ in 0..55 {
            notifications::notify_assigned(&f.t.state, &f.admin.id, &card, &f.member.id).await;
        }
        assert_eq!(list(&f, &f.member).await.len(), 50);

        f.t.cleanup().await;
    }
}
