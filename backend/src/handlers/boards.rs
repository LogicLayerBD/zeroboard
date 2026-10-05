use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{middleware, Extension, Json, Router};
use serde::Deserialize;

use super::{parse_id, validate_name};
use crate::auth::{require_auth, AuthUser};
use crate::errors::AppError;
use crate::models::WorkspaceRole;
use crate::services::access::{require_board_role, require_workspace_role};
use crate::services::boards;
use crate::AppState;

const WORKSPACE_ID_FIELD: &str = "workspace id";
const BOARD_ID_FIELD: &str = "board id";

pub fn router(state: &AppState) -> Router<AppState> {
    // `:id` (not `:workspaceId`) because the router rejects differently named
    // parameters at the same position as `/api/workspaces/:id`.
    Router::new()
        .route(
            "/api/workspaces/:id/boards",
            get(list_boards).post(create_board),
        )
        .route(
            "/api/boards/:id",
            get(get_board).patch(rename_board).delete(archive_board),
        )
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth))
}

#[derive(Deserialize)]
pub struct BoardNameRequest {
    name: String,
}

pub async fn list_boards(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let workspace_id = parse_id(&id, WORKSPACE_ID_FIELD)?;
    require_workspace_role(&state, &workspace_id, &auth_user.id, WorkspaceRole::Viewer).await?;
    Ok(Json(
        boards::list_for_workspace(&state, &workspace_id).await?,
    ))
}

pub async fn create_board(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
    body: Result<Json<BoardNameRequest>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let workspace_id = parse_id(&id, WORKSPACE_ID_FIELD)?;
    let Json(body) = body?;
    let name = validate_name(&body.name)?;
    require_workspace_role(&state, &workspace_id, &auth_user.id, WorkspaceRole::Member).await?;
    let board = boards::create(&state, &workspace_id, &auth_user.id, &name).await?;
    Ok((StatusCode::CREATED, Json(board)))
}

pub async fn get_board(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let board_id = parse_id(&id, BOARD_ID_FIELD)?;
    let board = require_board_role(&state, &board_id, &auth_user.id, WorkspaceRole::Viewer).await?;
    Ok(Json(boards::details(&state, board).await?))
}

pub async fn rename_board(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
    body: Result<Json<BoardNameRequest>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let board_id = parse_id(&id, BOARD_ID_FIELD)?;
    let Json(body) = body?;
    let name = validate_name(&body.name)?;
    require_board_role(&state, &board_id, &auth_user.id, WorkspaceRole::Admin).await?;
    Ok(Json(boards::rename(&state, &board_id, &name).await?))
}

pub async fn archive_board(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let board_id = parse_id(&id, BOARD_ID_FIELD)?;
    require_board_role(&state, &board_id, &auth_user.id, WorkspaceRole::Admin).await?;
    boards::archive(&state, &board_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::super::test_support::{TestApp, TestUser};
    use super::*;

    const MISSING_ID: &str = "00000000-0000-4000-8000-000000000000";
    const MAX_NAME_CHARS: usize = 255;
    const SEED_TS: i64 = 1_700_000_000_000;

    struct Fixture {
        t: TestApp,
        admin: TestUser,
        member: TestUser,
        viewer: TestUser,
        outsider: TestUser,
        workspace_id: String,
    }

    async fn fixture() -> Fixture {
        let t = TestApp::new().await;
        let admin = t.user("admin@example.com").await;
        let member = t.user("member@example.com").await;
        let viewer = t.user("viewer@example.com").await;
        let outsider = t.user("outsider@example.com").await;
        let workspace_id = t.workspace(&admin, "W").await;
        t.add_member(&workspace_id, &admin, &member, "member").await;
        t.add_member(&workspace_id, &admin, &viewer, "viewer").await;
        Fixture {
            t,
            admin,
            member,
            viewer,
            outsider,
            workspace_id,
        }
    }

    async fn create_board(f: &Fixture, user: &TestUser, name: &str) -> (StatusCode, Value) {
        f.t.post(
            &format!("/api/workspaces/{}/boards", f.workspace_id),
            user,
            json!({ "name": name }),
        )
        .await
    }

    async fn board_id(f: &Fixture, name: &str) -> String {
        let (status, board) = create_board(f, &f.admin, name).await;
        assert_eq!(status, StatusCode::CREATED);
        board["id"].as_str().unwrap().to_string()
    }

    async fn seed_list(f: &Fixture, board_id: &str, id: &str, position: f64) {
        sqlx::query!(
            "INSERT INTO lists (id, board_id, name, position, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
            id,
            board_id,
            id,
            position,
            SEED_TS,
            SEED_TS
        )
        .execute(&f.t.state.db)
        .await
        .unwrap();
    }

    async fn seed_card(f: &Fixture, board_id: &str, list_id: &str, id: &str, position: f64) {
        sqlx::query!(
            "INSERT INTO cards (id, list_id, board_id, title, position, created_by, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            id,
            list_id,
            board_id,
            id,
            position,
            f.admin.id,
            SEED_TS,
            SEED_TS
        )
        .execute(&f.t.state.db)
        .await
        .unwrap();
    }

    fn ids(values: &Value) -> Vec<&str> {
        values
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["id"].as_str().unwrap())
            .collect()
    }

    #[tokio::test]
    async fn create_requires_member_role_and_valid_name() {
        let f = fixture().await;

        let (status, board) = create_board(&f, &f.member, "  Roadmap ").await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(board["name"], "Roadmap");
        assert_eq!(board["workspace_id"], f.workspace_id.as_str());
        assert_eq!(board["created_by"], f.member.id.as_str());
        assert_eq!(board["archived"], false);

        assert_eq!(create_board(&f, &f.admin, "A").await.0, StatusCode::CREATED);
        assert_eq!(
            create_board(&f, &f.viewer, "V").await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            create_board(&f, &f.outsider, "O").await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            create_board(&f, &f.admin, " ").await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            create_board(&f, &f.admin, &"a".repeat(MAX_NAME_CHARS + 1))
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            create_board(&f, &f.admin, &"a".repeat(MAX_NAME_CHARS))
                .await
                .0,
            StatusCode::CREATED
        );

        let (status, _) =
            f.t.post(
                &format!("/api/workspaces/{MISSING_ID}/boards"),
                &f.admin,
                json!({ "name": "x" }),
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn list_shows_live_boards_to_members_only() {
        let f = fixture().await;
        let first = board_id(&f, "First").await;
        let second = board_id(&f, "Second").await;
        let archived = board_id(&f, "Archived").await;
        assert_eq!(
            f.t.delete(&format!("/api/boards/{archived}"), &f.admin)
                .await
                .0,
            StatusCode::NO_CONTENT
        );
        let uri = format!("/api/workspaces/{}/boards", f.workspace_id);

        let (status, boards) = f.t.get(&uri, &f.viewer).await;
        assert_eq!(status, StatusCode::OK);
        // Boards created in the same millisecond tie-break on id, so compare as a set.
        let mut listed = ids(&boards);
        listed.sort_unstable();
        let mut expected = [first.as_str(), second.as_str()];
        expected.sort_unstable();
        assert_eq!(listed, expected);

        assert_eq!(f.t.get(&uri, &f.outsider).await.0, StatusCode::FORBIDDEN);
        assert_eq!(
            f.t.get(&format!("/api/workspaces/{MISSING_ID}/boards"), &f.admin)
                .await
                .0,
            StatusCode::NOT_FOUND
        );

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn get_returns_lists_and_cards_ordered_by_position() {
        let f = fixture().await;
        let board = board_id(&f, "B").await;
        let other_board = board_id(&f, "Other").await;
        seed_list(&f, &board, "list-b", 2000.0).await;
        seed_list(&f, &board, "list-a", 1000.0).await;
        seed_list(&f, &other_board, "list-x", 500.0).await;
        seed_card(&f, &board, "list-a", "card-2", 2000.0).await;
        seed_card(&f, &board, "list-a", "card-1", 1000.0).await;
        seed_card(&f, &board, "list-b", "card-3", 1000.0).await;
        seed_card(&f, &other_board, "list-x", "card-x", 1000.0).await;

        let (status, details) = f.t.get(&format!("/api/boards/{board}"), &f.viewer).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(details["id"], board.as_str());
        assert_eq!(details["name"], "B");
        assert_eq!(ids(&details["lists"]), ["list-a", "list-b"]);
        assert_eq!(ids(&details["lists"][0]["cards"]), ["card-1", "card-2"]);
        assert_eq!(ids(&details["lists"][1]["cards"]), ["card-3"]);
        assert_eq!(details["lists"][0]["cards"][0]["list_id"], "list-a");

        let (status, empty) =
            f.t.get(&format!("/api/boards/{other_board}"), &f.member)
                .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(ids(&empty["lists"][0]["cards"]), ["card-x"]);

        assert_eq!(
            f.t.get(&format!("/api/boards/{board}"), &f.outsider)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            f.t.get(&format!("/api/boards/{MISSING_ID}"), &f.admin)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            f.t.get("/api/boards/nope", &f.admin).await.0,
            StatusCode::BAD_REQUEST
        );

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn rename_is_admin_only() {
        let f = fixture().await;
        let board = board_id(&f, "Old").await;
        let uri = format!("/api/boards/{board}");
        let body = json!({ "name": "New" });

        assert_eq!(
            f.t.patch(&uri, &f.member, body.clone()).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            f.t.patch(&uri, &f.viewer, body.clone()).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            f.t.patch(&uri, &f.outsider, body.clone()).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            f.t.patch(&format!("/api/boards/{MISSING_ID}"), &f.admin, body.clone())
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            f.t.patch(&uri, &f.admin, json!({ "name": "" })).await.0,
            StatusCode::BAD_REQUEST
        );

        let (status, renamed) = f.t.patch(&uri, &f.admin, body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(renamed["name"], "New");

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn delete_archives_board_admin_only_and_keeps_data() {
        let f = fixture().await;
        let board = board_id(&f, "B").await;
        seed_list(&f, &board, "list-a", 1000.0).await;
        seed_card(&f, &board, "list-a", "card-1", 1000.0).await;
        let uri = format!("/api/boards/{board}");

        assert_eq!(f.t.delete(&uri, &f.member).await.0, StatusCode::FORBIDDEN);
        assert_eq!(f.t.delete(&uri, &f.outsider).await.0, StatusCode::FORBIDDEN);
        assert_eq!(f.t.delete(&uri, &f.admin).await.0, StatusCode::NO_CONTENT);

        let row = sqlx::query!(
            r#"SELECT archived AS "archived: bool",
                      (SELECT COUNT(*) FROM cards WHERE board_id = $1) AS "cards!: i64"
               FROM boards WHERE id = $1"#,
            board
        )
        .fetch_one(&f.t.state.db)
        .await
        .unwrap();
        assert!(row.archived);
        assert_eq!(row.cards, 1, "archiving must not delete cards");

        assert_eq!(f.t.get(&uri, &f.admin).await.0, StatusCode::NOT_FOUND);
        assert_eq!(
            f.t.patch(&uri, &f.admin, json!({ "name": "x" })).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(f.t.delete(&uri, &f.admin).await.0, StatusCode::NOT_FOUND);

        f.t.cleanup().await;
    }
}
