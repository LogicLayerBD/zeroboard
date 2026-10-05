use std::fmt::Write;

use anyhow::Context;
use axum::body::Body;
use axum::extract::multipart::MultipartRejection;
use axum::extract::{DefaultBodyLimit, Multipart, Path, State};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{middleware, Extension, Json, Router};
use tokio_util::io::ReaderStream;

use super::parse_id;
use crate::auth::{require_auth, AuthUser};
use crate::errors::AppError;
use crate::models::WorkspaceRole;
use crate::services::access::require_card_role;
use crate::services::attachments::{self, sanitize_filename, upload_error};
use crate::AppState;

const CARD_ID_FIELD: &str = "card id";
const ATTACHMENT_ID_FIELD: &str = "attachment id";
/// Multipart form field that carries the file.
const FILE_FIELD: &str = "file";
/// Headroom over the file size limit for multipart boundaries and part headers.
const MULTIPART_OVERHEAD_BYTES: usize = 64 * 1024;
/// RFC 5987 `attr-char` punctuation that may appear unescaped in `filename*`.
const ATTR_CHAR_PUNCTUATION: &[u8] = b"!#$&+-.^_`|~";

pub fn router(state: &AppState) -> Router<AppState> {
    let max_file_bytes = attachments::max_upload_bytes(&state.config);
    let body_limit = usize::try_from(max_file_bytes)
        .unwrap_or(usize::MAX)
        .saturating_add(MULTIPART_OVERHEAD_BYTES);
    Router::new()
        .route(
            "/api/cards/:id/attachments",
            post(upload_attachment).layer(DefaultBodyLimit::max(body_limit)),
        )
        .route("/api/attachments/:id/download", get(download_attachment))
        .route("/api/attachments/:id", delete(delete_attachment))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth))
}

/// `Content-Disposition: attachment` with an ASCII fallback name and the exact
/// UTF-8 name percent-encoded in `filename*`.
fn content_disposition(filename: &str) -> String {
    let fallback: String = filename
        .chars()
        .map(|c| {
            if c == ' ' || (c.is_ascii_graphic() && c != '"' && c != '\\') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut encoded = String::with_capacity(filename.len());
    for byte in filename.bytes() {
        if byte.is_ascii_alphanumeric() || ATTR_CHAR_PUNCTUATION.contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    format!("attachment; filename=\"{fallback}\"; filename*=UTF-8''{encoded}")
}

/// Expects `multipart/form-data` with the file in a part named `file`.
pub async fn upload_attachment(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<impl IntoResponse, AppError> {
    let card_id = parse_id(&id, CARD_ID_FIELD)?;
    let mut multipart = multipart
        .map_err(|_| AppError::BadRequest("expected a multipart/form-data body".into()))?;
    let card = require_card_role(&state, &card_id, &auth_user.id, WorkspaceRole::Member).await?;

    let field = loop {
        match multipart
            .next_field()
            .await
            .map_err(|err| upload_error(err, &state.config))?
        {
            Some(field) if field.name() == Some(FILE_FIELD) => break field,
            Some(_) => continue,
            None => return Err(AppError::BadRequest(format!("{FILE_FIELD} is required"))),
        }
    };
    let filename = sanitize_filename(field.file_name().unwrap_or_default());
    let attachment = attachments::upload(&state, &card, &auth_user.id, &filename, field).await?;
    Ok((StatusCode::CREATED, Json(attachment)))
}

pub async fn download_attachment(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let attachment_id = parse_id(&id, ATTACHMENT_ID_FIELD)?;
    let attachment = attachments::get(&state, &attachment_id).await?;
    require_card_role(
        &state,
        &attachment.card_id,
        &auth_user.id,
        WorkspaceRole::Viewer,
    )
    .await?;

    let download = attachments::open_for_download(&state, &attachment).await?;
    let disposition = HeaderValue::from_str(&content_disposition(&download.filename))
        .context("invalid Content-Disposition header")?;
    let headers = [
        (
            CONTENT_TYPE,
            HeaderValue::from_static(download.content_type),
        ),
        (CONTENT_LENGTH, HeaderValue::from(download.size_bytes)),
        (CONTENT_DISPOSITION, disposition),
        (X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")),
        (CACHE_CONTROL, HeaderValue::from_static("private, no-store")),
    ];
    let body = Body::from_stream(ReaderStream::new(download.file));
    Ok((headers, body))
}

pub async fn delete_attachment(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let attachment_id = parse_id(&id, ATTACHMENT_ID_FIELD)?;
    let attachment = attachments::get(&state, &attachment_id).await?;
    let card = require_card_role(
        &state,
        &attachment.card_id,
        &auth_user.id,
        WorkspaceRole::Member,
    )
    .await?;
    attachments::delete(&state, &auth_user.id, &card, &attachment).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use axum::http::Method;
    use serde_json::{json, Value};

    use super::super::test_support::{BoardFixture, TestApp, TestUser};
    use super::*;

    const MISSING_ID: &str = "00000000-0000-4000-8000-000000000000";
    const BOUNDARY: &str = "zeroboard-test-boundary";
    const PNG_BYTES: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01";
    const BYTES_PER_MB: usize = 1024 * 1024;

    fn multipart_body(field: &str, filename: &str, claimed_type: &str, content: &[u8]) -> Vec<u8> {
        let mut body = format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{filename}\"\r\nContent-Type: {claimed_type}\r\n\r\n"
        )
        .into_bytes();
        body.extend_from_slice(content);
        body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
        body
    }

    async fn upload(
        t: &TestApp,
        user: &TestUser,
        card_id: &str,
        filename: &str,
        claimed_type: &str,
        content: &[u8],
    ) -> (StatusCode, Value) {
        let (status, _, bytes) = t
            .send_raw(
                Method::POST,
                &format!("/api/cards/{card_id}/attachments"),
                user,
                Some(&format!("multipart/form-data; boundary={BOUNDARY}")),
                multipart_body(FILE_FIELD, filename, claimed_type, content),
            )
            .await;
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn files_in(t: &TestApp, card_id: &str) -> usize {
        let dir = t.state.config.attachments_dir.join(card_id);
        let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
            return 0;
        };
        let mut count = 0;
        while entries.next_entry().await.unwrap().is_some() {
            count += 1;
        }
        count
    }

    async fn actions(t: &TestApp, card_id: &str) -> Vec<(String, Value)> {
        sqlx::query!(
            r#"SELECT action, payload AS "payload!" FROM activity_log
               WHERE card_id = $1 ORDER BY created_at, rowid"#,
            card_id
        )
        .fetch_all(&t.state.db)
        .await
        .unwrap()
        .into_iter()
        .map(|row| (row.action, serde_json::from_str(&row.payload).unwrap()))
        .collect()
    }

    #[tokio::test]
    async fn upload_detects_type_from_content_and_download_streams_it() {
        let f = BoardFixture::new().await;
        let card_id = f.card("T").await;

        let (status, attachment) = upload(
            &f.t,
            &f.member,
            &card_id,
            "../../evil.png",
            "text/plain",
            PNG_BYTES,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{attachment}");
        assert_eq!(attachment["filename"], "evil.png");
        assert_eq!(attachment["card_id"], card_id.as_str());
        assert_eq!(attachment["size_bytes"], PNG_BYTES.len());
        assert_eq!(attachment["uploaded_by"], f.member.id.as_str());
        assert!(attachment.get("stored_path").is_none());
        let id = attachment["id"].as_str().unwrap();

        let stored =
            f.t.state
                .config
                .attachments_dir
                .join(&card_id)
                .join(format!("{id}_evil.png"));
        assert_eq!(tokio::fs::read(&stored).await.unwrap(), PNG_BYTES);
        assert_eq!(
            actions(&f.t, &card_id).await[1],
            (
                "added_attachment".to_string(),
                json!({ "attachment_id": id, "filename": "evil.png" })
            )
        );

        let uri = format!("/api/attachments/{id}/download");
        let (status, headers, body) =
            f.t.send_raw(Method::GET, &uri, &f.viewer, None, Vec::new())
                .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[CONTENT_TYPE], "image/png");
        assert_eq!(
            headers[CONTENT_DISPOSITION],
            "attachment; filename=\"evil.png\"; filename*=UTF-8''evil.png"
        );
        assert_eq!(headers[X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert_eq!(
            headers[CONTENT_LENGTH],
            PNG_BYTES.len().to_string().as_str()
        );
        assert_eq!(&body[..], PNG_BYTES);

        assert_eq!(f.t.get(&uri, &f.outsider).await.0, StatusCode::FORBIDDEN);
        assert_eq!(
            f.t.get(
                &format!("/api/attachments/{MISSING_ID}/download"),
                &f.viewer
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn text_uploads_are_typed_by_content_and_extension() {
        let f = BoardFixture::new().await;
        let card_id = f.card("T").await;

        let (status, csv) = upload(
            &f.t,
            &f.member,
            &card_id,
            "data.csv",
            "image/png",
            b"a,b\n1,2\n",
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{csv}");
        let uri = format!("/api/attachments/{}/download", csv["id"].as_str().unwrap());
        let (_, headers, _) =
            f.t.send_raw(Method::GET, &uri, &f.member, None, Vec::new())
                .await;
        assert_eq!(headers[CONTENT_TYPE], "text/csv");

        let (status, txt) = upload(
            &f.t,
            &f.member,
            &card_id,
            "résumé notes.txt",
            "text/plain",
            b"hi",
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{txt}");
        let uri = format!("/api/attachments/{}/download", txt["id"].as_str().unwrap());
        let (_, headers, _) =
            f.t.send_raw(Method::GET, &uri, &f.member, None, Vec::new())
                .await;
        assert_eq!(headers[CONTENT_TYPE], "text/plain");
        assert_eq!(
            headers[CONTENT_DISPOSITION],
            "attachment; filename=\"r_sum_ notes.txt\"; filename*=UTF-8''r%C3%A9sum%C3%A9%20notes.txt"
        );

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn upload_rejects_bad_types_roles_and_requests_without_leaving_files() {
        let f = BoardFixture::new().await;
        let card_id = f.card("T").await;

        for (name, content) in [
            ("setup.png", b"MZ\x90\0\x03\0\0\0\x04\0".as_slice()),
            (
                "page.txt",
                b"<html><script>alert(1)</script></html>".as_slice(),
            ),
            ("blob.txt", b"abc\0\x01\x02".as_slice()),
            ("empty.txt", b"".as_slice()),
        ] {
            let (status, body) =
                upload(&f.t, &f.member, &card_id, name, "image/png", content).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{name}: {body}");
        }
        assert_eq!(
            upload(&f.t, &f.viewer, &card_id, "a.png", "image/png", PNG_BYTES)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            upload(&f.t, &f.outsider, &card_id, "a.png", "image/png", PNG_BYTES)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            upload(&f.t, &f.member, MISSING_ID, "a.png", "image/png", PNG_BYTES)
                .await
                .0,
            StatusCode::NOT_FOUND
        );

        let wrong_field =
            f.t.send_raw(
                Method::POST,
                &format!("/api/cards/{card_id}/attachments"),
                &f.member,
                Some(&format!("multipart/form-data; boundary={BOUNDARY}")),
                multipart_body("other", "a.png", "image/png", PNG_BYTES),
            )
            .await;
        assert_eq!(wrong_field.0, StatusCode::BAD_REQUEST);
        let not_multipart =
            f.t.post(
                &format!("/api/cards/{card_id}/attachments"),
                &f.member,
                json!({}),
            )
            .await;
        assert_eq!(not_multipart.0, StatusCode::BAD_REQUEST);

        assert_eq!(files_in(&f.t, &card_id).await, 0);
        let rows = sqlx::query!(
            r#"SELECT COUNT(*) AS "n!: i64" FROM attachments WHERE card_id = $1"#,
            card_id
        )
        .fetch_one(&f.t.state.db)
        .await
        .unwrap();
        assert_eq!(rows.n, 0);

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn upload_enforces_configured_max_size() {
        let f = BoardFixture::with_app(TestApp::with_max_attachment_size_mb(1).await).await;
        let card_id = f.card("T").await;

        let mut at_limit = PNG_BYTES.to_vec();
        at_limit.resize(BYTES_PER_MB, 0);
        let (status, body) =
            upload(&f.t, &f.member, &card_id, "max.png", "image/png", &at_limit).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");

        let mut over_limit = at_limit.clone();
        over_limit.push(0);
        let (status, body) = upload(
            &f.t,
            &f.member,
            &card_id,
            "big.png",
            "image/png",
            &over_limit,
        )
        .await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(body["error"], "payload too large");

        let mut far_over = at_limit.clone();
        far_over.resize(2 * BYTES_PER_MB, 0);
        let (status, body) = upload(
            &f.t,
            &f.member,
            &card_id,
            "huge.png",
            "image/png",
            &far_over,
        )
        .await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(body["error"], "payload too large");

        assert_eq!(files_in(&f.t, &card_id).await, 1);

        f.t.cleanup().await;
    }

    #[tokio::test]
    async fn delete_removes_row_and_file_and_logs() {
        let f = BoardFixture::new().await;
        let card_id = f.card("T").await;
        let (_, attachment) =
            upload(&f.t, &f.admin, &card_id, "a.png", "image/png", PNG_BYTES).await;
        let id = attachment["id"].as_str().unwrap();
        let uri = format!("/api/attachments/{id}");

        assert_eq!(f.t.delete(&uri, &f.viewer).await.0, StatusCode::FORBIDDEN);
        assert_eq!(f.t.delete(&uri, &f.outsider).await.0, StatusCode::FORBIDDEN);
        assert_eq!(files_in(&f.t, &card_id).await, 1);
        assert_eq!(f.t.delete(&uri, &f.member).await.0, StatusCode::NO_CONTENT);
        assert_eq!(files_in(&f.t, &card_id).await, 0);
        assert_eq!(f.t.delete(&uri, &f.member).await.0, StatusCode::NOT_FOUND);
        assert_eq!(
            f.t.get(&format!("{uri}/download"), &f.member).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            actions(&f.t, &card_id).await.last().unwrap(),
            &(
                "removed_attachment".to_string(),
                json!({ "attachment_id": id, "filename": "a.png" })
            )
        );
        assert_eq!(
            f.t.delete("/api/attachments/nope", &f.member).await.0,
            StatusCode::BAD_REQUEST
        );

        f.t.cleanup().await;
    }

    #[test]
    fn content_disposition_escapes_quotes_and_non_ascii() {
        assert_eq!(
            content_disposition("a\"b\\c.txt"),
            "attachment; filename=\"a_b_c.txt\"; filename*=UTF-8''a%22b%5Cc.txt"
        );
    }
}
