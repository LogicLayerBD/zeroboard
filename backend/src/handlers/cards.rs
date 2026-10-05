use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, patch, post};
use axum::{middleware, Extension, Json, Router};
use serde::{Deserialize, Deserializer};

use super::parse_id;
use crate::auth::{require_auth, AuthUser};
use crate::errors::AppError;
use crate::models::WorkspaceRole;
use crate::services::access::{require_card_role, require_list_role};
use crate::services::cards::{self, CardChanges, NewCard};
use crate::AppState;

const LIST_ID_FIELD: &str = "list id";
const CARD_ID_FIELD: &str = "card id";
const USER_ID_FIELD: &str = "user id";
const LABEL_ID_FIELD: &str = "label id";
const AFTER_ID_FIELD: &str = "after_id";

const MAX_TITLE_CHARS: usize = 500;
const MAX_DESCRIPTION_CHARS: usize = 50_000;

pub fn router(state: &AppState) -> Router<AppState> {
    // `:id` (not `:listId`) because the router rejects differently named
    // parameters at the same position as `/api/lists/:id`.
    Router::new()
        .route("/api/lists/:id/cards", get(list_cards).post(create_card))
        .route(
            "/api/cards/:id",
            get(get_card).patch(update_card).delete(delete_card),
        )
        .route("/api/cards/:id/move", patch(move_card))
        .route("/api/cards/:id/assignees", post(add_assignee))
        .route("/api/cards/:id/assignees/:user_id", delete(remove_assignee))
        .route("/api/cards/:id/labels", post(add_label))
        .route("/api/cards/:id/labels/:label_id", delete(remove_label))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth))
}

#[derive(Deserialize)]
pub struct CreateCardRequest {
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    due_date: Option<i64>,
}

/// Absent fields are left unchanged; `null` clears `description` / `due_date`.
#[derive(Deserialize)]
pub struct UpdateCardRequest {
    #[serde(default)]
    title: Option<String>,
    #[serde(default, deserialize_with = "present")]
    description: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    due_date: Option<Option<i64>>,
}

#[derive(Deserialize)]
pub struct MoveCardRequest {
    list_id: String,
    /// The card to place this one directly after; absent or null moves it first.
    #[serde(default)]
    after_id: Option<String>,
}

#[derive(Deserialize)]
pub struct AssigneeRequest {
    user_id: String,
}

#[derive(Deserialize)]
pub struct LabelRequest {
    label_id: String,
}

/// Distinguishes an explicit `null` (`Some(None)`) from an absent field (`None`).
fn present<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

fn validate_title(raw: &str) -> Result<String, AppError> {
    let title = raw.trim();
    if title.is_empty() {
        return Err(AppError::BadRequest("title is required".into()));
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(AppError::BadRequest(format!(
            "title must be at most {MAX_TITLE_CHARS} characters"
        )));
    }
    Ok(title.to_string())
}

/// Blank descriptions are stored as NULL.
fn validate_description(raw: Option<String>) -> Result<Option<String>, AppError> {
    let Some(description) = raw.filter(|d| !d.trim().is_empty()) else {
        return Ok(None);
    };
    if description.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(AppError::BadRequest(format!(
            "description must be at most {MAX_DESCRIPTION_CHARS} characters"
        )));
    }
    Ok(Some(description))
}

/// Due dates are Unix milliseconds.
fn validate_due_date(raw: Option<i64>) -> Result<Option<i64>, AppError> {
    match raw {
        Some(ms) if ms < 0 => Err(AppError::BadRequest("due_date is invalid".into())),
        other => Ok(other),
    }
}

pub async fn list_cards(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let list_id = parse_id(&id, LIST_ID_FIELD)?;
    require_list_role(&state, &list_id, &auth_user.id, WorkspaceRole::Viewer).await?;
    Ok(Json(cards::list_for_list(&state, &list_id).await?))
}

pub async fn create_card(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
    body: Result<Json<CreateCardRequest>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let list_id = parse_id(&id, LIST_ID_FIELD)?;
    let Json(body) = body?;
    let new_card = NewCard {
        title: validate_title(&body.title)?,
        description: validate_description(body.description)?,
        due_date: validate_due_date(body.due_date)?,
    };
    let list = require_list_role(&state, &list_id, &auth_user.id, WorkspaceRole::Member).await?;
    let card = cards::create(&state, &list.id, &list.board_id, &auth_user.id, new_card).await?;
    Ok((StatusCode::CREATED, Json(card)))
}

pub async fn get_card(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let card_id = parse_id(&id, CARD_ID_FIELD)?;
    let card = require_card_role(&state, &card_id, &auth_user.id, WorkspaceRole::Viewer).await?;
    Ok(Json(cards::details(&state, card).await?))
}

pub async fn update_card(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
    body: Result<Json<UpdateCardRequest>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let card_id = parse_id(&id, CARD_ID_FIELD)?;
    let Json(body) = body?;
    if body.title.is_none() && body.description.is_none() && body.due_date.is_none() {
        return Err(AppError::BadRequest("no fields to update".into()));
    }
    let changes = CardChanges {
        title: body.title.as_deref().map(validate_title).transpose()?,
        description: body.description.map(validate_description).transpose()?,
        due_date: body.due_date.map(validate_due_date).transpose()?,
    };
    require_card_role(&state, &card_id, &auth_user.id, WorkspaceRole::Member).await?;
    Ok(Json(cards::update(&state, &card_id, changes).await?))
}

pub async fn delete_card(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let card_id = parse_id(&id, CARD_ID_FIELD)?;
    require_card_role(&state, &card_id, &auth_user.id, WorkspaceRole::Member).await?;
    cards::delete(&state, &card_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn move_card(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
    body: Result<Json<MoveCardRequest>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let card_id = parse_id(&id, CARD_ID_FIELD)?;
    let Json(body) = body?;
    let to_list_id = parse_id(&body.list_id, LIST_ID_FIELD)?;
    let after_id = body
        .after_id
        .as_deref()
        .map(|raw| parse_id(raw, AFTER_ID_FIELD))
        .transpose()?;
    let card = require_card_role(&state, &card_id, &auth_user.id, WorkspaceRole::Member).await?;
    let moved = cards::move_card(
        &state,
        &auth_user.id,
        &card,
        &to_list_id,
        after_id.as_deref(),
    )
    .await?;
    Ok(Json(moved))
}

pub async fn add_assignee(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
    body: Result<Json<AssigneeRequest>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let card_id = parse_id(&id, CARD_ID_FIELD)?;
    let Json(body) = body?;
    let user_id = parse_id(&body.user_id, USER_ID_FIELD)?;
    let card = require_card_role(&state, &card_id, &auth_user.id, WorkspaceRole::Member).await?;
    let assignee = cards::add_assignee(&state, &auth_user.id, &card, &user_id).await?;
    Ok((StatusCode::CREATED, Json(assignee)))
}

pub async fn remove_assignee(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((id, user_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, AppError> {
    let card_id = parse_id(&id, CARD_ID_FIELD)?;
    let user_id = parse_id(&user_id, USER_ID_FIELD)?;
    let card = require_card_role(&state, &card_id, &auth_user.id, WorkspaceRole::Member).await?;
    cards::remove_assignee(&state, &auth_user.id, &card, &user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn add_label(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
    body: Result<Json<LabelRequest>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let card_id = parse_id(&id, CARD_ID_FIELD)?;
    let Json(body) = body?;
    let label_id = parse_id(&body.label_id, LABEL_ID_FIELD)?;
    let card = require_card_role(&state, &card_id, &auth_user.id, WorkspaceRole::Member).await?;
    let card_label = cards::add_label(&state, &auth_user.id, &card, &label_id).await?;
    Ok((StatusCode::CREATED, Json(card_label)))
}

pub async fn remove_label(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((id, label_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, AppError> {
    let card_id = parse_id(&id, CARD_ID_FIELD)?;
    let label_id = parse_id(&label_id, LABEL_ID_FIELD)?;
    let card = require_card_role(&state, &card_id, &auth_user.id, WorkspaceRole::Member).await?;
    cards::remove_label(&state, &auth_user.id, &card, &label_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::super::test_support::BoardFixture;
    use super::*;

    const MISSING_ID: &str = "00000000-0000-4000-8000-000000000000";
    const DUE_MS: i64 = 1_800_000_000_000;

    async fn list(f: &BoardFixture, board_id: &str, name: &str) -> String {
        let (status, list) =
            f.t.post(
                &format!("/api/boards/{board_id}/lists"),
                &f.admin,
                json!({ "name": name }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{list}");
        list["id"].as_str().unwrap().to_string()
    }

    async fn card(f: &BoardFixture, list_id: &str, title: &str) -> String {
        let (status, card) =
            f.t.post(
                &format!("/api/lists/{list_id}/cards"),
                &f.member,
                json!({ "title": title }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{card}");
        card["id"].as_str().unwrap().to_string()
    }

    async fn label(f: &BoardFixture, board_id: &str, name: &str) -> String {
        let (status, label) =
            f.t.post(
                &format!("/api/boards/{board_id}/labels"),
                &f.member,
                json!({ "name": name, "color": "#ff0000" }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{label}");
        label["id"].as_str().unwrap().to_string()
    }

    async fn titles_and_positions(f: &BoardFixture, list_id: &str) -> Vec<(String, f64)> {
        let (status, cards) =
            f.t.get(&format!("/api/lists/{list_id}/cards"), &f.viewer)
                .await;
        assert_eq!(status, StatusCode::OK);
        cards
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                (
                    c["title"].as_str().unwrap().to_string(),
                    c["position"].as_f64().unwrap(),
                )
            })
            .collect()
    }

    async fn activity(f: &BoardFixture, card_id: &str) -> Vec<(String, Value)> {
        sqlx::query!(
            r#"SELECT action, payload AS "payload!" FROM activity_log
               WHERE card_id = $1 ORDER BY created_at, rowid"#,
            card_id
        )
        .fetch_all(&f.t.state.db)
        .await
        .unwrap()
        .into_iter()
        .map(|row| (row.action, serde_json::from_str(&row.payload).unwrap()))
        .collect()
    }

    async fn move_to(
        f: &BoardFixture,
        card_id: &str,
        list_id: &str,
        after_id: Value,
    ) -> (StatusCode, Value) {
        f.t.patch(
            &format!("/api/cards/{card_id}/move"),
            &f.member,
            json!({ "list_id": list_id, "after_id": after_id }),
        )
        .await
    }

    #[tokio::test]
    async fn create_appends_validates_and_logs_activity() {
        let f = BoardFixture::new().await;
        let todo = list(&f, &f.board_id, "Todo").await;
        let uri = format!("/api/lists/{todo}/cards");

        let (status, first) =
            f.t.post(
                &uri,
                &f.member,
                json!({ "title": " First ", "description": "d", "due_date": DUE_MS }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(first["title"], "First");
        assert_eq!(first["description"], "d");
        assert_eq!(first["due_date"], DUE_MS);
        assert_eq!(first["list_id"], todo.as_str());
        assert_eq!(first["board_id"], f.board_id.as_str());
        assert_eq!(first["created_by"], f.member.id.as_str());
        card(&f, &todo, "Second").await;
        assert_eq!(
            titles_and_positions(&f, &todo).await,
            [
                ("First".to_string(), 1000.0),
                ("Second".to_string(), 2000.0)
            ]
        );

        let first_id = first["id"].as_str().unwrap();
        assert_eq!(
            activity(&f, first_id).await,
            [(
                "created_card".to_string(),
                json!({ "list_id": todo, "title": "First" })
            )]
        );

        let title = json!({ "title": "x" });
        assert_eq!(
            f.t.post(&uri, &f.viewer, title.clone()).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            f.t.post(&uri, &f.outsider, title.clone()).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(f.t.get(&uri, &f.outsider).await.0, StatusCode::FORBIDDEN);
        for bad in [
            json!({ "title": " " }),
            json!({ "title": "a".repeat(MAX_TITLE_CHARS + 1) }),
            json!({ "title": "t", "description": "a".repeat(MAX_DESCRIPTION_CHARS + 1) }),
            json!({ "title": "t", "due_date": -1 }),
        ] {
            assert_eq!(
                f.t.post(&uri, &f.member, bad).await.0,
                StatusCode::BAD_REQUEST
            );
        }
        assert_eq!(
            f.t.post(&format!("/api/lists/{MISSING_ID}/cards"), &f.member, title)
                .await
                .0,
            StatusCode::NOT_FOUND
        );

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn update_is_partial_and_null_clears() {
        let f = BoardFixture::new().await;
        let todo = list(&f, &f.board_id, "Todo").await;
        let id = card(&f, &todo, "T").await;
        let uri = format!("/api/cards/{id}");

        let (status, updated) =
            f.t.patch(
                &uri,
                &f.member,
                json!({ "description": "notes", "due_date": DUE_MS }),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(updated["title"], "T");
        assert_eq!(updated["description"], "notes");
        assert_eq!(updated["due_date"], DUE_MS);

        let (status, updated) = f.t.patch(&uri, &f.member, json!({ "title": "New" })).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(updated["title"], "New");
        assert_eq!(updated["description"], "notes");

        let (status, updated) =
            f.t.patch(
                &uri,
                &f.member,
                json!({ "description": null, "due_date": null }),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(updated["title"], "New");
        assert_eq!(updated["description"], Value::Null);
        assert_eq!(updated["due_date"], Value::Null);

        assert_eq!(
            f.t.patch(&uri, &f.member, json!({})).await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            f.t.patch(&uri, &f.member, json!({ "title": "" })).await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            f.t.patch(&uri, &f.viewer, json!({ "title": "V" })).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            f.t.patch(
                &format!("/api/cards/{MISSING_ID}"),
                &f.member,
                json!({ "title": "x" })
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn move_across_lists_uses_fractional_positions_and_logs() {
        let f = BoardFixture::new().await;
        let todo = list(&f, &f.board_id, "Todo").await;
        let done = list(&f, &f.board_id, "Done").await;
        let a = card(&f, &todo, "A").await;
        let b = card(&f, &todo, "B").await;
        let x = card(&f, &done, "X").await;
        let y = card(&f, &done, "Y").await;

        let (status, moved) = move_to(&f, &a, &done, json!(x)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(moved["list_id"], done.as_str());
        assert_eq!(moved["position"], 1500.0);

        let (status, moved) = move_to(&f, &b, &done, Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(moved["position"], 500.0);

        let (status, moved) = move_to(&f, &x, &done, json!(y)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(moved["position"], 3000.0);

        assert!(titles_and_positions(&f, &todo).await.is_empty());
        assert_eq!(
            titles_and_positions(&f, &done).await,
            [
                ("B".to_string(), 500.0),
                ("A".to_string(), 1500.0),
                ("Y".to_string(), 2000.0),
                ("X".to_string(), 3000.0)
            ]
        );
        assert_eq!(
            activity(&f, &a).await[1],
            (
                "moved_card".to_string(),
                json!({ "from_list_id": todo, "to_list_id": done, "position": 1500.0 })
            )
        );

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn move_rebalances_target_list_when_gap_is_too_small() {
        let f = BoardFixture::new().await;
        let todo = list(&f, &f.board_id, "Todo").await;
        let a = card(&f, &todo, "A").await;
        let b = card(&f, &todo, "B").await;
        let c = card(&f, &todo, "C").await;
        sqlx::query!("UPDATE cards SET position = 1000.0005 WHERE id = $1", b)
            .execute(&f.t.state.db)
            .await
            .unwrap();

        let (status, moved) = move_to(&f, &c, &todo, json!(a)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(moved["position"], 2000.0);
        assert_eq!(
            titles_and_positions(&f, &todo).await,
            [
                ("A".to_string(), 1000.0),
                ("C".to_string(), 2000.0),
                ("B".to_string(), 3000.0)
            ]
        );

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn move_rejects_other_board_bad_anchor_and_viewers() {
        let f = BoardFixture::new().await;
        let todo = list(&f, &f.board_id, "Todo").await;
        let done = list(&f, &f.board_id, "Done").await;
        let other_board = f.t.board(&f.admin, &f.workspace_id, "Other").await;
        let foreign_list = list(&f, &other_board, "Foreign").await;
        let a = card(&f, &todo, "A").await;
        let b = card(&f, &todo, "B").await;

        assert_eq!(
            move_to(&f, &a, &foreign_list, Value::Null).await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            move_to(&f, &a, MISSING_ID, Value::Null).await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            move_to(&f, &a, &done, json!(b)).await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            move_to(&f, &a, &todo, json!(a)).await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            f.t.patch(
                &format!("/api/cards/{a}/move"),
                &f.viewer,
                json!({ "list_id": done })
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn assignees_must_be_workspace_members_and_are_logged() {
        let f = BoardFixture::new().await;
        let todo = list(&f, &f.board_id, "Todo").await;
        let id = card(&f, &todo, "T").await;
        let uri = format!("/api/cards/{id}/assignees");

        let (status, assignee) =
            f.t.post(&uri, &f.member, json!({ "user_id": f.viewer.id }))
                .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(assignee, json!({ "card_id": id, "user_id": f.viewer.id }));

        let duplicate = json!({ "user_id": f.viewer.id });
        assert_eq!(
            f.t.post(&uri, &f.member, duplicate).await.0,
            StatusCode::BAD_REQUEST
        );
        let outsider = json!({ "user_id": f.outsider.id });
        assert_eq!(
            f.t.post(&uri, &f.member, outsider).await.0,
            StatusCode::BAD_REQUEST
        );
        let viewer_attempt = json!({ "user_id": f.member.id });
        assert_eq!(
            f.t.post(&uri, &f.viewer, viewer_attempt).await.0,
            StatusCode::FORBIDDEN
        );

        let (_, details) = f.t.get(&format!("/api/cards/{id}"), &f.viewer).await;
        assert_eq!(details["assignees"][0]["user_id"], f.viewer.id.as_str());
        assert_eq!(details["assignees"][0]["email"], f.viewer.email.as_str());

        let remove_uri = format!("{uri}/{}", f.viewer.id);
        assert_eq!(
            f.t.delete(&remove_uri, &f.viewer).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            f.t.delete(&remove_uri, &f.member).await.0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            f.t.delete(&remove_uri, &f.member).await.0,
            StatusCode::NOT_FOUND
        );

        assert_eq!(
            activity(&f, &id).await[1..],
            [
                (
                    "added_assignee".to_string(),
                    json!({ "user_id": f.viewer.id })
                ),
                (
                    "removed_assignee".to_string(),
                    json!({ "user_id": f.viewer.id })
                )
            ]
        );

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn labels_must_belong_to_the_board_and_are_logged() {
        let f = BoardFixture::new().await;
        let todo = list(&f, &f.board_id, "Todo").await;
        let id = card(&f, &todo, "T").await;
        let bug = label(&f, &f.board_id, "Bug").await;
        let other_board = f.t.board(&f.admin, &f.workspace_id, "Other").await;
        let foreign = label(&f, &other_board, "Foreign").await;
        let uri = format!("/api/cards/{id}/labels");

        let (status, card_label) = f.t.post(&uri, &f.member, json!({ "label_id": bug })).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(card_label, json!({ "card_id": id, "label_id": bug }));
        assert_eq!(
            f.t.post(&uri, &f.member, json!({ "label_id": bug }))
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            f.t.post(&uri, &f.member, json!({ "label_id": foreign }))
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            f.t.post(&uri, &f.viewer, json!({ "label_id": bug }))
                .await
                .0,
            StatusCode::FORBIDDEN
        );

        let (_, details) = f.t.get(&format!("/api/cards/{id}"), &f.viewer).await;
        assert_eq!(details["labels"][0]["id"], bug.as_str());
        assert_eq!(details["labels"][0]["name"], "Bug");

        let remove_uri = format!("{uri}/{bug}");
        assert_eq!(
            f.t.delete(&remove_uri, &f.member).await.0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            f.t.delete(&remove_uri, &f.member).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            activity(&f, &id).await[1..],
            [
                ("added_label".to_string(), json!({ "label_id": bug })),
                ("removed_label".to_string(), json!({ "label_id": bug }))
            ]
        );

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn get_returns_full_detail_to_viewers_only() {
        let f = BoardFixture::new().await;
        let todo = list(&f, &f.board_id, "Todo").await;
        let id = card(&f, &todo, "T").await;
        seed_children(&f, &id).await;

        let (status, details) = f.t.get(&format!("/api/cards/{id}"), &f.viewer).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(details["id"], id.as_str());
        assert_eq!(details["title"], "T");
        assert_eq!(details["attachments"][0]["filename"], "f.txt");
        assert!(details["attachments"][0].get("stored_path").is_none());
        assert_eq!(details["time_entries"][0]["minutes"], 30);
        assert_eq!(details["comments"][0]["body"], "hi");
        assert_eq!(details["assignees"], json!([]));
        assert_eq!(details["labels"], json!([]));

        assert_eq!(
            f.t.get(&format!("/api/cards/{id}"), &f.outsider).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            f.t.get(&format!("/api/cards/{MISSING_ID}"), &f.viewer)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            f.t.get("/api/cards/nope", &f.viewer).await.0,
            StatusCode::BAD_REQUEST
        );

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn delete_removes_related_rows_and_files_but_keeps_history() {
        let f = BoardFixture::new().await;
        let todo = list(&f, &f.board_id, "Todo").await;
        let id = card(&f, &todo, "T").await;
        let bug = label(&f, &f.board_id, "Bug").await;
        f.t.post(
            &format!("/api/cards/{id}/assignees"),
            &f.member,
            json!({ "user_id": f.member.id }),
        )
        .await;
        f.t.post(
            &format!("/api/cards/{id}/labels"),
            &f.member,
            json!({ "label_id": bug }),
        )
        .await;
        seed_children(&f, &id).await;
        let card_dir = f.t.state.config.attachments_dir.join(&id);
        tokio::fs::create_dir_all(&card_dir).await.unwrap();
        tokio::fs::write(card_dir.join("f.txt"), b"x")
            .await
            .unwrap();
        let uri = format!("/api/cards/{id}");

        assert_eq!(f.t.delete(&uri, &f.viewer).await.0, StatusCode::FORBIDDEN);
        assert_eq!(f.t.delete(&uri, &f.member).await.0, StatusCode::NO_CONTENT);

        let counts = sqlx::query!(
            r#"SELECT
                 (SELECT COUNT(*) FROM cards WHERE id = $1) AS "cards!: i64",
                 (SELECT COUNT(*) FROM card_assignees WHERE card_id = $1) AS "assignees!: i64",
                 (SELECT COUNT(*) FROM card_labels WHERE card_id = $1) AS "labels!: i64",
                 (SELECT COUNT(*) FROM attachments WHERE card_id = $1) AS "attachments!: i64",
                 (SELECT COUNT(*) FROM time_entries WHERE card_id = $1) AS "time_entries!: i64",
                 (SELECT COUNT(*) FROM comments WHERE card_id = $1) AS "comments!: i64",
                 (SELECT COUNT(*) FROM activity_log WHERE board_id = $2) AS "activity!: i64",
                 (SELECT COUNT(*) FROM labels WHERE id = $3) AS "board_labels!: i64""#,
            id,
            f.board_id,
            bug
        )
        .fetch_one(&f.t.state.db)
        .await
        .unwrap();
        assert_eq!(
            [
                counts.cards,
                counts.assignees,
                counts.labels,
                counts.attachments,
                counts.time_entries,
                counts.comments
            ],
            [0; 6]
        );
        assert_eq!(counts.activity, 3, "history survives card deletion");
        assert_eq!(counts.board_labels, 1, "board label is not deleted");
        assert!(!card_dir.exists());
        assert_eq!(f.t.delete(&uri, &f.member).await.0, StatusCode::NOT_FOUND);

        f.t.cleanup().await;
    }

    /// Inserts one attachment, time entry and comment directly; their handlers don't exist yet.
    async fn seed_children(f: &BoardFixture, card_id: &str) {
        let user = &f.member.id;
        let ts = DUE_MS;
        let attachment_id = uuid::Uuid::new_v4().to_string();
        let stored_path = format!("{card_id}/f.txt");
        sqlx::query!(
            "INSERT INTO attachments (id, card_id, filename, stored_path, size_bytes, uploaded_by, uploaded_at)
             VALUES ($1, $2, 'f.txt', $3, 1, $4, $5)",
            attachment_id,
            card_id,
            stored_path,
            user,
            ts
        )
        .execute(&f.t.state.db)
        .await
        .unwrap();
        let entry_id = uuid::Uuid::new_v4().to_string();
        sqlx::query!(
            "INSERT INTO time_entries (id, card_id, user_id, minutes, logged_at)
             VALUES ($1, $2, $3, 30, $4)",
            entry_id,
            card_id,
            user,
            ts
        )
        .execute(&f.t.state.db)
        .await
        .unwrap();
        let comment_id = uuid::Uuid::new_v4().to_string();
        sqlx::query!(
            "INSERT INTO comments (id, card_id, user_id, body, created_at, updated_at)
             VALUES ($1, $2, $3, 'hi', $4, $4)",
            comment_id,
            card_id,
            user,
            ts
        )
        .execute(&f.t.state.db)
        .await
        .unwrap();
    }
}
