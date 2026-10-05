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
use crate::services::access::{require_board_role, require_label_role};
use crate::services::labels;
use crate::AppState;

const BOARD_ID_FIELD: &str = "board id";
const LABEL_ID_FIELD: &str = "label id";

const MAX_LABEL_NAME_CHARS: usize = 100;
/// `#rgb` or `#rrggbb`.
const HEX_COLOR_DIGITS: [usize; 2] = [3, 6];

pub fn router(state: &AppState) -> Router<AppState> {
    // `:id` (not `:boardId`) because the router rejects differently named
    // parameters at the same position as `/api/boards/:id`.
    Router::new()
        .route(
            "/api/boards/:id/labels",
            get(list_labels).post(create_label),
        )
        .route("/api/labels/:id", patch(update_label).delete(delete_label))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth))
}

#[derive(Deserialize)]
pub struct CreateLabelRequest {
    name: String,
    color: String,
}

#[derive(Deserialize)]
pub struct UpdateLabelRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    color: Option<String>,
}

fn validate_label_name(raw: &str) -> Result<String, AppError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest("name is required".into()));
    }
    if name.chars().count() > MAX_LABEL_NAME_CHARS {
        return Err(AppError::BadRequest(format!(
            "name must be at most {MAX_LABEL_NAME_CHARS} characters"
        )));
    }
    Ok(name.to_string())
}

/// Returns the color lowercased.
fn validate_color(raw: &str) -> Result<String, AppError> {
    let valid = raw.strip_prefix('#').is_some_and(|digits| {
        HEX_COLOR_DIGITS.contains(&digits.len()) && digits.chars().all(|c| c.is_ascii_hexdigit())
    });
    if !valid {
        return Err(AppError::BadRequest(
            "color must be a hex color like #ff0000".into(),
        ));
    }
    Ok(raw.to_ascii_lowercase())
}

pub async fn list_labels(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let board_id = parse_id(&id, BOARD_ID_FIELD)?;
    require_board_role(&state, &board_id, &auth_user.id, WorkspaceRole::Viewer).await?;
    Ok(Json(labels::list_for_board(&state, &board_id).await?))
}

pub async fn create_label(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
    body: Result<Json<CreateLabelRequest>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let board_id = parse_id(&id, BOARD_ID_FIELD)?;
    let Json(body) = body?;
    let name = validate_label_name(&body.name)?;
    let color = validate_color(&body.color)?;
    require_board_role(&state, &board_id, &auth_user.id, WorkspaceRole::Member).await?;
    let label = labels::create(&state, &board_id, &name, &color).await?;
    Ok((StatusCode::CREATED, Json(label)))
}

pub async fn update_label(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
    body: Result<Json<UpdateLabelRequest>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let label_id = parse_id(&id, LABEL_ID_FIELD)?;
    let Json(body) = body?;
    if body.name.is_none() && body.color.is_none() {
        return Err(AppError::BadRequest("no fields to update".into()));
    }
    let name = body.name.as_deref().map(validate_label_name).transpose()?;
    let color = body.color.as_deref().map(validate_color).transpose()?;
    require_label_role(&state, &label_id, &auth_user.id, WorkspaceRole::Member).await?;
    let label = labels::update(&state, &label_id, name.as_deref(), color.as_deref()).await?;
    Ok(Json(label))
}

pub async fn delete_label(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let label_id = parse_id(&id, LABEL_ID_FIELD)?;
    require_label_role(&state, &label_id, &auth_user.id, WorkspaceRole::Member).await?;
    labels::delete(&state, &label_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::test_support::BoardFixture;
    use super::*;

    const MISSING_ID: &str = "00000000-0000-4000-8000-000000000000";

    #[test]
    fn color_accepts_short_and_long_hex_only() {
        assert_eq!(validate_color("#FF00aa").unwrap(), "#ff00aa");
        assert_eq!(validate_color("#abc").unwrap(), "#abc");
        for bad in ["ff0000", "#ff00", "#gg0000", "#ff00000", "", "#", "red"] {
            assert!(validate_color(bad).is_err(), "{bad}");
        }
    }

    #[tokio::test]
    async fn create_validates_and_list_orders_by_name() {
        let f = BoardFixture::new().await;
        let uri = format!("/api/boards/{}/labels", f.board_id);

        let (status, label) =
            f.t.post(
                &uri,
                &f.member,
                json!({ "name": " Urgent ", "color": "#FF0000" }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(label["name"], "Urgent");
        assert_eq!(label["color"], "#ff0000");
        assert_eq!(label["board_id"], f.board_id.as_str());
        let (status, _) =
            f.t.post(&uri, &f.admin, json!({ "name": "Bug", "color": "#00f" }))
                .await;
        assert_eq!(status, StatusCode::CREATED);

        let (status, labels) = f.t.get(&uri, &f.viewer).await;
        assert_eq!(status, StatusCode::OK);
        let names: Vec<_> = labels
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["Bug", "Urgent"]);

        for bad in [
            json!({ "name": "", "color": "#fff" }),
            json!({ "name": "a".repeat(MAX_LABEL_NAME_CHARS + 1), "color": "#fff" }),
            json!({ "name": "x", "color": "blue" }),
            json!({ "name": "x" }),
        ] {
            assert_eq!(
                f.t.post(&uri, &f.member, bad).await.0,
                StatusCode::BAD_REQUEST
            );
        }
        let valid = json!({ "name": "x", "color": "#fff" });
        assert_eq!(
            f.t.post(
                &uri,
                &f.member,
                json!({ "name": "a".repeat(MAX_LABEL_NAME_CHARS), "color": "#fff" })
            )
            .await
            .0,
            StatusCode::CREATED
        );
        assert_eq!(
            f.t.post(&uri, &f.viewer, valid.clone()).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            f.t.post(&uri, &f.outsider, valid.clone()).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(f.t.get(&uri, &f.outsider).await.0, StatusCode::FORBIDDEN);
        assert_eq!(
            f.t.post(
                &format!("/api/boards/{MISSING_ID}/labels"),
                &f.member,
                valid
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn update_changes_only_given_fields() {
        let f = BoardFixture::new().await;
        let (_, label) =
            f.t.post(
                &format!("/api/boards/{}/labels", f.board_id),
                &f.member,
                json!({ "name": "Bug", "color": "#ff0000" }),
            )
            .await;
        let uri = format!("/api/labels/{}", label["id"].as_str().unwrap());

        let (status, updated) =
            f.t.patch(&uri, &f.member, json!({ "color": "#00FF00" }))
                .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(updated["name"], "Bug");
        assert_eq!(updated["color"], "#00ff00");

        let (status, updated) =
            f.t.patch(&uri, &f.member, json!({ "name": "Defect" }))
                .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(updated["name"], "Defect");
        assert_eq!(updated["color"], "#00ff00");

        assert_eq!(
            f.t.patch(&uri, &f.member, json!({})).await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            f.t.patch(&uri, &f.member, json!({ "color": "nope" }))
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            f.t.patch(&uri, &f.viewer, json!({ "name": "V" })).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            f.t.patch(
                &format!("/api/labels/{MISSING_ID}"),
                &f.member,
                json!({ "name": "x" })
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn delete_removes_label_from_all_cards() {
        let f = BoardFixture::new().await;
        let (_, label) =
            f.t.post(
                &format!("/api/boards/{}/labels", f.board_id),
                &f.member,
                json!({ "name": "Bug", "color": "#ff0000" }),
            )
            .await;
        let label_id = label["id"].as_str().unwrap();
        let (_, list) =
            f.t.post(
                &format!("/api/boards/{}/lists", f.board_id),
                &f.member,
                json!({ "name": "L" }),
            )
            .await;
        let list_id = list["id"].as_str().unwrap();
        for title in ["A", "B"] {
            let (_, card) =
                f.t.post(
                    &format!("/api/lists/{list_id}/cards"),
                    &f.member,
                    json!({ "title": title }),
                )
                .await;
            let (status, _) =
                f.t.post(
                    &format!("/api/cards/{}/labels", card["id"].as_str().unwrap()),
                    &f.member,
                    json!({ "label_id": label_id }),
                )
                .await;
            assert_eq!(status, StatusCode::CREATED);
        }
        let uri = format!("/api/labels/{label_id}");

        assert_eq!(f.t.delete(&uri, &f.viewer).await.0, StatusCode::FORBIDDEN);
        assert_eq!(f.t.delete(&uri, &f.member).await.0, StatusCode::NO_CONTENT);

        let remaining = sqlx::query!(
            r#"SELECT COUNT(*) AS "count!: i64" FROM card_labels WHERE label_id = $1"#,
            label_id
        )
        .fetch_one(&f.t.state.db)
        .await
        .unwrap();
        assert_eq!(remaining.count, 0);
        assert_eq!(f.t.delete(&uri, &f.member).await.0, StatusCode::NOT_FOUND);

        f.t.cleanup().await;
    }
}
