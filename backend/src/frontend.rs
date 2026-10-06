//! Serves the compiled Svelte app that is embedded into the binary at build time.

use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{Method, Uri};
use axum::response::{IntoResponse, Response};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rust_embed::RustEmbed;
use sha2::{Digest, Sha256};

use crate::errors::AppError;

#[derive(RustEmbed)]
#[folder = "../frontend/dist/"]
struct Assets;

/// SPA shell; SvelteKit's client router handles every non-asset path.
const INDEX_HTML: &str = "index.html";
/// Vite content-hashes every file under this prefix, so it can be cached forever.
const IMMUTABLE_PREFIX: &str = "_app/immutable/";
const IMMUTABLE_CACHE_CONTROL: &str = "public, max-age=31536000, immutable";
/// Everything else (notably index.html) must be revalidated so clients pick up
/// new asset hashes after an upgrade.
const REVALIDATE_CACHE_CONTROL: &str = "no-cache";
/// Unknown paths under these first segments are real misses (unknown API routes,
/// stale asset hashes) and must 404 instead of returning the SPA shell.
const NO_SPA_FALLBACK_SEGMENTS: &[&str] = &["api", "_app"];
const INLINE_SCRIPT_OPEN: &str = "<script>";
const SCRIPT_CLOSE: &str = "</script>";

pub async fn serve(method: Method, uri: Uri) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return AppError::NotFound.into_response();
    }

    let path = uri.path().trim_start_matches('/');
    if let Some(response) = asset_response(path) {
        return response;
    }

    let first_segment = path.split('/').next().unwrap_or_default();
    if NO_SPA_FALLBACK_SEGMENTS.contains(&first_segment) {
        return AppError::NotFound.into_response();
    }

    asset_response(INDEX_HTML).unwrap_or_else(|| AppError::NotFound.into_response())
}

/// CSP source expressions (`sha256-...`) for the inline `<script>` blocks in the
/// SPA shell. SvelteKit bootstraps the app from an inline script, which
/// `script-src 'self'` would otherwise block.
pub fn inline_script_hashes() -> Vec<String> {
    let Some(index) = Assets::get(INDEX_HTML) else {
        return Vec::new();
    };
    let html = String::from_utf8_lossy(&index.data);
    inline_scripts(&html).into_iter().map(csp_hash).collect()
}

fn asset_response(path: &str) -> Option<Response> {
    if path.is_empty() {
        return None;
    }
    let file = Assets::get(path)?;
    let cache_control = if path.starts_with(IMMUTABLE_PREFIX) {
        IMMUTABLE_CACHE_CONTROL
    } else {
        REVALIDATE_CACHE_CONTROL
    };
    Some(
        (
            [
                (CONTENT_TYPE, file.metadata.mimetype().to_owned()),
                (CACHE_CONTROL, cache_control.to_owned()),
            ],
            file.data,
        )
            .into_response(),
    )
}

/// Bodies of attribute-less `<script>` tags; tags with attributes (e.g. `src`) are external.
fn inline_scripts(html: &str) -> Vec<&str> {
    let mut scripts = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find(INLINE_SCRIPT_OPEN) {
        let body = &rest[start + INLINE_SCRIPT_OPEN.len()..];
        let Some(end) = body.find(SCRIPT_CLOSE) else {
            break;
        };
        scripts.push(&body[..end]);
        rest = &body[end + SCRIPT_CLOSE.len()..];
    }
    scripts
}

fn csp_hash(script: &str) -> String {
    format!("sha256-{}", BASE64.encode(Sha256::digest(script.as_bytes())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::StatusCode;

    const MAX_BODY_BYTES: usize = 10 * 1024 * 1024;

    async fn get(path: &str) -> Response {
        serve(Method::GET, path.parse().unwrap()).await
    }

    async fn body_text(response: Response) -> String {
        let bytes = to_bytes(response.into_body(), MAX_BODY_BYTES).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn root_and_client_routes_serve_the_spa_shell() {
        for path in ["/", "/login", "/some-workspace-id/board/123"] {
            let response = get(path).await;
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            assert!(response.headers()[CONTENT_TYPE].to_str().unwrap().starts_with("text/html"));
            assert_eq!(response.headers()[CACHE_CONTROL], REVALIDATE_CACHE_CONTROL, "{path}");
            assert!(body_text(response).await.contains("<html"), "{path}");
        }
    }

    #[tokio::test]
    async fn hashed_assets_are_served_with_immutable_caching() {
        let asset = Assets::iter()
            .find(|name| name.starts_with(IMMUTABLE_PREFIX) && name.ends_with(".js"))
            .expect("frontend build contains hashed JS assets");
        let response = get(&format!("/{asset}")).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers()[CONTENT_TYPE].to_str().unwrap().contains("javascript"));
        assert_eq!(response.headers()[CACHE_CONTROL], IMMUTABLE_CACHE_CONTROL);
    }

    #[tokio::test]
    async fn unknown_api_and_asset_paths_404_instead_of_spa_shell() {
        for path in ["/api/nope", "/api", "/_app/immutable/missing.js"] {
            let response = get(path).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
            assert!(body_text(response).await.contains("not found"), "{path}");
        }
    }

    #[tokio::test]
    async fn non_get_requests_do_not_get_the_spa_shell() {
        let response = serve(Method::POST, "/login".parse().unwrap()).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn extracts_only_inline_scripts() {
        let html = r#"<script src="/a.js"></script><script>alert(1)</script><p>x</p><script>b()</script>"#;
        assert_eq!(inline_scripts(html), vec!["alert(1)", "b()"]);
    }

    #[test]
    fn csp_hash_matches_browser_format() {
        // echo -n "alert(1)" | openssl dgst -sha256 -binary | base64
        assert_eq!(csp_hash("alert(1)"), "sha256-bhHHL3z2vDgxUt0W3dWQOrprscmda2Y5pLsLg4GF+pI=");
    }

    #[test]
    fn built_shell_bootstrap_script_is_hashed() {
        assert!(!inline_script_hashes().is_empty());
    }
}
