use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, patch};
use axum::{middleware, Extension, Json, Router};
use serde::Deserialize;

use super::{parse_id, validate_name};
use crate::auth::{require_auth, AuthUser};
use crate::errors::AppError;
use crate::models::WorkspaceRole;
use crate::services::access::{require_board_role, require_list_role};
use crate::services::lists;
use crate::AppState;

const BOARD_ID_FIELD: &str = "board id";
const LIST_ID_FIELD: &str = "list id";
const AFTER_ID_FIELD: &str = "after_id";

pub fn router(state: &AppState) -> Router<AppState> {
    // `:id` (not `:boardId`) because the router rejects differently named
    // parameters at the same position as `/api/boards/:id`.
    Router::new()
        .route("/api/boards/:id/lists", get(list_lists).post(create_list))
        .route("/api/lists/:id", patch(rename_list).delete(delete_list))
        .route("/api/lists/:id/position", patch(reorder_list))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth))
}

#[derive(Deserialize)]
pub struct ListNameRequest {
    name: String,
}

#[derive(Deserialize)]
pub struct ReorderListRequest {
    /// The list to place this one directly after; absent or null moves it first.
    #[serde(default)]
    after_id: Option<String>,
}

pub async fn list_lists(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let board_id = parse_id(&id, BOARD_ID_FIELD)?;
    require_board_role(&state, &board_id, &auth_user.id, WorkspaceRole::Viewer).await?;
    Ok(Json(lists::list_for_board(&state, &board_id).await?))
}

pub async fn create_list(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
    body: Result<Json<ListNameRequest>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let board_id = parse_id(&id, BOARD_ID_FIELD)?;
    let Json(body) = body?;
    let name = validate_name(&body.name)?;
    require_board_role(&state, &board_id, &auth_user.id, WorkspaceRole::Member).await?;
    let list = lists::create(&state, &board_id, &name).await?;
    Ok((StatusCode::CREATED, Json(list)))
}

pub async fn rename_list(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
    body: Result<Json<ListNameRequest>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let list_id = parse_id(&id, LIST_ID_FIELD)?;
    let Json(body) = body?;
    let name = validate_name(&body.name)?;
    require_list_role(&state, &list_id, &auth_user.id, WorkspaceRole::Member).await?;
    Ok(Json(lists::rename(&state, &list_id, &name).await?))
}

pub async fn delete_list(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let list_id = parse_id(&id, LIST_ID_FIELD)?;
    require_list_role(&state, &list_id, &auth_user.id, WorkspaceRole::Member).await?;
    lists::delete(&state, &list_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn reorder_list(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
    body: Result<Json<ReorderListRequest>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let list_id = parse_id(&id, LIST_ID_FIELD)?;
    let Json(body) = body?;
    let after_id = body
        .after_id
        .as_deref()
        .map(|raw| parse_id(raw, AFTER_ID_FIELD))
        .transpose()?;
    let list = require_list_role(&state, &list_id, &auth_user.id, WorkspaceRole::Member).await?;
    Ok(Json(
        lists::reorder(&state, &list, after_id.as_deref()).await?,
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::super::test_support::BoardFixture;
    use super::*;

    const MISSING_ID: &str = "00000000-0000-4000-8000-000000000000";

    async fn create(f: &BoardFixture, board_id: &str, name: &str) -> String {
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

    async fn lists(f: &BoardFixture) -> Value {
        let (status, lists) =
            f.t.get(&format!("/api/boards/{}/lists", f.board_id), &f.viewer)
                .await;
        assert_eq!(status, StatusCode::OK);
        lists
    }

    fn names_and_positions(lists: &Value) -> Vec<(String, f64)> {
        lists
            .as_array()
            .unwrap()
            .iter()
            .map(|l| {
                (
                    l["name"].as_str().unwrap().to_string(),
                    l["position"].as_f64().unwrap(),
                )
            })
            .collect()
    }

    async fn reorder(f: &BoardFixture, list_id: &str, after_id: Value) -> (StatusCode, Value) {
        f.t.patch(
            &format!("/api/lists/{list_id}/position"),
            &f.member,
            json!({ "after_id": after_id }),
        )
        .await
    }

    #[tokio::test]
    async fn create_appends_and_get_orders_by_position() {
        let f = BoardFixture::new().await;
        let uri = format!("/api/boards/{}/lists", f.board_id);

        let (status, todo) = f.t.post(&uri, &f.member, json!({ "name": " Todo " })).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(todo["name"], "Todo");
        assert_eq!(todo["board_id"], f.board_id.as_str());
        assert_eq!(todo["position"], 1000.0);
        create(&f, &f.board_id, "Doing").await;

        assert_eq!(
            names_and_positions(&lists(&f).await),
            [("Todo".to_string(), 1000.0), ("Doing".to_string(), 2000.0)]
        );

        let name = json!({ "name": "x" });
        assert_eq!(
            f.t.post(&uri, &f.viewer, name.clone()).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            f.t.post(&uri, &f.outsider, name.clone()).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(f.t.get(&uri, &f.outsider).await.0, StatusCode::FORBIDDEN);
        assert_eq!(
            f.t.post(&uri, &f.admin, json!({ "name": "  " })).await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            f.t.post(&format!("/api/boards/{MISSING_ID}/lists"), &f.admin, name)
                .await
                .0,
            StatusCode::NOT_FOUND
        );

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn rename_requires_member_role() {
        let f = BoardFixture::new().await;
        let list = create(&f, &f.board_id, "Old").await;
        let uri = format!("/api/lists/{list}");

        assert_eq!(
            f.t.patch(&uri, &f.viewer, json!({ "name": "New" })).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            f.t.patch(&uri, &f.member, json!({ "name": "" })).await.0,
            StatusCode::BAD_REQUEST
        );
        let (status, renamed) = f.t.patch(&uri, &f.member, json!({ "name": "New" })).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(renamed["name"], "New");
        assert_eq!(
            f.t.patch(
                &format!("/api/lists/{MISSING_ID}"),
                &f.admin,
                json!({ "name": "x" })
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn reorder_uses_fractional_positions() {
        let f = BoardFixture::new().await;
        let a = create(&f, &f.board_id, "A").await;
        let b = create(&f, &f.board_id, "B").await;
        let c = create(&f, &f.board_id, "C").await;

        let (status, moved) = reorder(&f, &c, Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(moved["position"], 500.0);

        let (status, moved) = reorder(&f, &b, json!(c)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(moved["position"], 750.0);

        let (status, moved) = reorder(&f, &c, json!(a)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(moved["position"], 2000.0);

        assert_eq!(
            names_and_positions(&lists(&f).await),
            [
                ("B".to_string(), 750.0),
                ("A".to_string(), 1000.0),
                ("C".to_string(), 2000.0)
            ]
        );

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn reorder_rebalances_when_gap_is_too_small() {
        let f = BoardFixture::new().await;
        let a = create(&f, &f.board_id, "A").await;
        let b = create(&f, &f.board_id, "B").await;
        let c = create(&f, &f.board_id, "C").await;
        sqlx::query!("UPDATE lists SET position = 1000.0005 WHERE id = $1", b)
            .execute(&f.t.state.db)
            .await
            .unwrap();

        let (status, moved) = reorder(&f, &c, json!(a)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(moved["position"], 2000.0);
        assert_eq!(
            names_and_positions(&lists(&f).await),
            [
                ("A".to_string(), 1000.0),
                ("C".to_string(), 2000.0),
                ("B".to_string(), 3000.0)
            ]
        );

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn reorder_rejects_bad_anchor_and_viewers() {
        let f = BoardFixture::new().await;
        let a = create(&f, &f.board_id, "A").await;
        let other_board = f.t.board(&f.admin, &f.workspace_id, "Other").await;
        let foreign = create(&f, &other_board, "Foreign").await;

        assert_eq!(reorder(&f, &a, json!(a)).await.0, StatusCode::BAD_REQUEST);
        assert_eq!(
            reorder(&f, &a, json!(foreign)).await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            reorder(&f, &a, json!("nope")).await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            f.t.patch(
                &format!("/api/lists/{a}/position"),
                &f.viewer,
                json!({ "after_id": null })
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn delete_removes_cards_and_attachment_files() {
        let f = BoardFixture::new().await;
        let list = create(&f, &f.board_id, "L").await;
        let (status, card) =
            f.t.post(
                &format!("/api/lists/{list}/cards"),
                &f.member,
                json!({ "title": "C" }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        let card_id = card["id"].as_str().unwrap();
        let card_dir = f.t.state.config.attachments_dir.join(card_id);
        tokio::fs::create_dir_all(&card_dir).await.unwrap();
        tokio::fs::write(card_dir.join("file.txt"), b"x")
            .await
            .unwrap();
        let uri = format!("/api/lists/{list}");

        assert_eq!(f.t.delete(&uri, &f.viewer).await.0, StatusCode::FORBIDDEN);
        assert_eq!(f.t.delete(&uri, &f.member).await.0, StatusCode::NO_CONTENT);

        let cards = sqlx::query!(
            r#"SELECT COUNT(*) AS "count!: i64" FROM cards WHERE list_id = $1"#,
            list
        )
        .fetch_one(&f.t.state.db)
        .await
        .unwrap();
        assert_eq!(cards.count, 0);
        assert!(!card_dir.exists());
        assert_eq!(f.t.delete(&uri, &f.member).await.0, StatusCode::NOT_FOUND);

        f.t.cleanup().await;
    }
}
