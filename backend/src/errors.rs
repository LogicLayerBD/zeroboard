use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

const INTERNAL_ERROR_MESSAGE: &str = "internal server error";
const INVALID_BODY_MESSAGE: &str = "invalid request body";

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found")]
    NotFound,
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("{0}")]
    BadRequest(String),
    #[error("too many requests")]
    TooManyRequests,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

/// Serde details may echo raw input, so clients only get a generic message.
impl From<JsonRejection> for AppError {
    fn from(_: JsonRejection) -> Self {
        AppError::BadRequest(INVALID_BODY_MESSAGE.to_string())
    }
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        match err {
            sqlx::Error::RowNotFound => AppError::NotFound,
            other => AppError::Internal(other.into()),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::NotFound => (StatusCode::NOT_FOUND, self.to_string()),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, self.to_string()),
            AppError::Forbidden => (StatusCode::FORBIDDEN, self.to_string()),
            AppError::BadRequest(reason) => (StatusCode::BAD_REQUEST, reason.clone()),
            AppError::TooManyRequests => (StatusCode::TOO_MANY_REQUESTS, self.to_string()),
            AppError::Internal(err) => {
                tracing::error!(error = ?err, "internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    INTERNAL_ERROR_MESSAGE.to_string(),
                )
            }
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    const MAX_TEST_BODY_BYTES: usize = 64 * 1024;

    async fn render(err: AppError) -> (StatusCode, serde_json::Value) {
        let response = err.into_response();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), MAX_TEST_BODY_BYTES)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn maps_client_errors_to_status_and_message() {
        assert_eq!(
            render(AppError::NotFound).await,
            (StatusCode::NOT_FOUND, json!({ "error": "not found" }))
        );
        assert_eq!(
            render(AppError::Unauthorized).await,
            (StatusCode::UNAUTHORIZED, json!({ "error": "unauthorized" }))
        );
        assert_eq!(
            render(AppError::Forbidden).await,
            (StatusCode::FORBIDDEN, json!({ "error": "forbidden" }))
        );
        assert_eq!(
            render(AppError::BadRequest("title is required".into())).await,
            (StatusCode::BAD_REQUEST, json!({ "error": "title is required" }))
        );
        assert_eq!(
            render(AppError::TooManyRequests).await,
            (StatusCode::TOO_MANY_REQUESTS, json!({ "error": "too many requests" }))
        );
    }

    #[tokio::test]
    async fn internal_errors_do_not_leak_details() {
        let err = AppError::Internal(anyhow::anyhow!("disk /var/secret is full"));
        let (status, body) = render(err).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body, json!({ "error": INTERNAL_ERROR_MESSAGE }));
    }

    #[tokio::test]
    async fn sqlx_row_not_found_maps_to_404_and_others_to_500() {
        let (status, _) = render(sqlx::Error::RowNotFound.into()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, body) = render(sqlx::Error::PoolTimedOut.into()).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body, json!({ "error": INTERNAL_ERROR_MESSAGE }));
    }
}
