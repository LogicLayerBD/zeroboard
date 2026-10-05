use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get};
use axum::{middleware, Extension, Json, Router};
use serde::Deserialize;

use super::parse_id;
use crate::auth::{require_auth, AuthUser};
use crate::errors::AppError;
use crate::models::WorkspaceRole;
use crate::services::access::{require_card_access, require_card_role};
use crate::services::time_entries;
use crate::AppState;

const CARD_ID_FIELD: &str = "card id";
const TIME_ENTRY_ID_FIELD: &str = "time entry id";

const MIN_MINUTES: i64 = 1;
/// 24 hours: one entry cannot exceed a day.
const MAX_MINUTES: i64 = 24 * 60;
const MAX_DESCRIPTION_CHARS: usize = 500;

pub fn router(state: &AppState) -> Router<AppState> {
    Router::new()
        .route(
            "/api/cards/:id/time-entries",
            get(list_time_entries).post(create_time_entry),
        )
        .route("/api/time-entries/:id", delete(delete_time_entry))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth))
}

#[derive(Deserialize)]
pub struct CreateTimeEntryRequest {
    minutes: i64,
    #[serde(default)]
    description: Option<String>,
}

fn validate_minutes(minutes: i64) -> Result<i64, AppError> {
    if !(MIN_MINUTES..=MAX_MINUTES).contains(&minutes) {
        return Err(AppError::BadRequest(format!(
            "minutes must be between {MIN_MINUTES} and {MAX_MINUTES}"
        )));
    }
    Ok(minutes)
}

/// Trimmed; blank descriptions are stored as NULL.
fn validate_description(raw: Option<String>) -> Result<Option<String>, AppError> {
    let Some(description) = raw.as_deref().map(str::trim).filter(|d| !d.is_empty()) else {
        return Ok(None);
    };
    if description.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(AppError::BadRequest(format!(
            "description must be at most {MAX_DESCRIPTION_CHARS} characters"
        )));
    }
    Ok(Some(description.to_string()))
}

pub async fn list_time_entries(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let card_id = parse_id(&id, CARD_ID_FIELD)?;
    require_card_role(&state, &card_id, &auth_user.id, WorkspaceRole::Viewer).await?;
    Ok(Json(time_entries::list_for_card(&state, &card_id).await?))
}

pub async fn create_time_entry(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
    body: Result<Json<CreateTimeEntryRequest>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let card_id = parse_id(&id, CARD_ID_FIELD)?;
    let Json(body) = body?;
    let minutes = validate_minutes(body.minutes)?;
    let description = validate_description(body.description)?;
    let card = require_card_role(&state, &card_id, &auth_user.id, WorkspaceRole::Member).await?;
    let logged =
        time_entries::create(&state, &card.id, &auth_user.id, minutes, description).await?;
    Ok((StatusCode::CREATED, Json(logged)))
}

/// Members may delete their own entries; workspace admins may delete any.
pub async fn delete_time_entry(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let entry_id = parse_id(&id, TIME_ENTRY_ID_FIELD)?;
    let entry = time_entries::get(&state, &entry_id).await?;
    let (_, role) =
        require_card_access(&state, &entry.card_id, &auth_user.id, WorkspaceRole::Member).await?;
    if entry.user_id != auth_user.id && role != WorkspaceRole::Admin {
        tracing::warn!(time_entry_id = %entry_id, user_id = %auth_user.id, "time entry delete denied: not owner");
        return Err(AppError::Forbidden);
    }
    time_entries::delete(&state, &entry_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::test_support::BoardFixture;
    use super::*;

    const MISSING_ID: &str = "00000000-0000-4000-8000-000000000000";

    #[tokio::test]
    async fn create_validates_and_returns_running_total() {
        let f = BoardFixture::new().await;
        let card_id = f.card("T").await;
        let uri = format!("/api/cards/{card_id}/time-entries");

        let (status, logged) =
            f.t.post(
                &uri,
                &f.member,
                json!({ "minutes": 30, "description": "  review  " }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{logged}");
        assert_eq!(logged["time_entry"]["minutes"], 30);
        assert_eq!(logged["time_entry"]["description"], "review");
        assert_eq!(logged["time_entry"]["user_id"], f.member.id.as_str());
        assert_eq!(logged["time_entry"]["card_id"], card_id.as_str());
        assert_eq!(logged["total_minutes"], 30);

        let (status, logged) =
            f.t.post(&uri, &f.admin, json!({ "minutes": MAX_MINUTES }))
                .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(logged["time_entry"]["description"], serde_json::Value::Null);
        assert_eq!(logged["total_minutes"], 30 + MAX_MINUTES);

        for bad in [
            json!({ "minutes": 0 }),
            json!({ "minutes": -5 }),
            json!({ "minutes": MAX_MINUTES + 1 }),
            json!({ "minutes": 10, "description": "a".repeat(MAX_DESCRIPTION_CHARS + 1) }),
            json!({ "minutes": "ten" }),
            json!({}),
        ] {
            assert_eq!(
                f.t.post(&uri, &f.member, bad).await.0,
                StatusCode::BAD_REQUEST
            );
        }
        let ok = json!({ "minutes": 5 });
        assert_eq!(
            f.t.post(&uri, &f.viewer, ok.clone()).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            f.t.post(&uri, &f.outsider, ok.clone()).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            f.t.post(
                &format!("/api/cards/{MISSING_ID}/time-entries"),
                &f.member,
                ok
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );

        let (status, list) = f.t.get(&uri, &f.viewer).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(list["total_minutes"], 30 + MAX_MINUTES);
        assert_eq!(list["time_entries"].as_array().unwrap().len(), 2);
        assert_eq!(list["time_entries"][0]["minutes"], 30);
        assert_eq!(f.t.get(&uri, &f.outsider).await.0, StatusCode::FORBIDDEN);

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn delete_allows_owner_or_workspace_admin_only() {
        let f = BoardFixture::new().await;
        let card_id = f.card("T").await;
        let uri = format!("/api/cards/{card_id}/time-entries");
        let (_, admin_entry) = f.t.post(&uri, &f.admin, json!({ "minutes": 10 })).await;
        let (_, member_entry) = f.t.post(&uri, &f.member, json!({ "minutes": 20 })).await;
        let admin_entry = format!(
            "/api/time-entries/{}",
            admin_entry["time_entry"]["id"].as_str().unwrap()
        );
        let member_entry = format!(
            "/api/time-entries/{}",
            member_entry["time_entry"]["id"].as_str().unwrap()
        );

        assert_eq!(
            f.t.delete(&admin_entry, &f.member).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            f.t.delete(&member_entry, &f.viewer).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            f.t.delete(&member_entry, &f.outsider).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            f.t.delete(&member_entry, &f.member).await.0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            f.t.delete(&member_entry, &f.member).await.0,
            StatusCode::NOT_FOUND
        );

        let (_, member_entry) = f.t.post(&uri, &f.member, json!({ "minutes": 20 })).await;
        let member_entry = format!(
            "/api/time-entries/{}",
            member_entry["time_entry"]["id"].as_str().unwrap()
        );
        assert_eq!(
            f.t.delete(&member_entry, &f.admin).await.0,
            StatusCode::NO_CONTENT
        );

        let (_, list) = f.t.get(&uri, &f.member).await;
        assert_eq!(list["total_minutes"], 10);
        assert_eq!(
            f.t.delete("/api/time-entries/nope", &f.member).await.0,
            StatusCode::BAD_REQUEST
        );

        f.t.cleanup().await;
    }
}
