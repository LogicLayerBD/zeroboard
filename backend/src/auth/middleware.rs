use axum::extract::{Request, State};
use axum::http::header::AUTHORIZATION;
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::Response;

use crate::auth::jwt;
use crate::errors::AppError;
use crate::AppState;

const BEARER_SCHEME: &str = "Bearer";

/// Authenticated caller, injected by [`require_auth`].
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: String,
    pub email: String,
}

pub async fn require_auth(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let token = bearer_token(req.headers()).ok_or(AppError::Unauthorized)?;
    let claims = jwt::verify_access_token(token, &state.config.jwt_secret).map_err(|err| {
        tracing::debug!(error = %err, "access token rejected");
        AppError::Unauthorized
    })?;
    req.extensions_mut().insert(AuthUser {
        id: claims.sub,
        email: claims.email,
    });
    Ok(next.run(req).await)
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case(BEARER_SCHEME) {
        return None;
    }
    let token = token.trim();
    (!token.is_empty()).then_some(token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers_with(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_str(value).unwrap());
        headers
    }

    #[test]
    fn extracts_bearer_token() {
        assert_eq!(bearer_token(&headers_with("Bearer abc.def")), Some("abc.def"));
        assert_eq!(bearer_token(&headers_with("bearer abc")), Some("abc"));
    }

    #[test]
    fn rejects_missing_or_non_bearer_headers() {
        assert_eq!(bearer_token(&HeaderMap::new()), None);
        assert_eq!(bearer_token(&headers_with("Basic abc")), None);
        assert_eq!(bearer_token(&headers_with("Bearer ")), None);
        assert_eq!(bearer_token(&headers_with("Bearer")), None);
    }
}
