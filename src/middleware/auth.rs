//! API key authentication middleware.

use std::sync::Arc;

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use crate::proxy::ProxyState;

/// Axum middleware that validates API key from Bearer token or x-api-key header.
///
/// Skips auth when:
/// - `api-keys` config is empty (auth disabled)
/// - Path is `/health`
/// - Path starts with `/v0/management/`
pub async fn api_key_auth(
    state: axum::extract::State<Arc<ProxyState>>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path();

    // Skip auth for health, management, dashboard read API, and OAuth callbacks
    if path == "/health"
        || path.starts_with("/v0/management")
        || path.starts_with("/dashboard")
        || path.ends_with("/callback")
    {
        return next.run(req).await;
    }

    // If no API keys configured, auth is disabled
    let api_keys = {
        let config = state.config.read().await;
        config.api_keys.clone()
    };

    if api_keys.is_empty() {
        return next.run(req).await;
    }

    // Extract key from Authorization: Bearer <key> or x-api-key header
    let provided_key = extract_api_key(&req);

    match provided_key {
        Some(key) if api_keys.iter().any(|k| k == &key) => next.run(req).await,
        _ => unauthorized_response(),
    }
}

/// Extract API key from request headers.
/// Checks `Authorization: Bearer <key>` first, then `x-api-key`.
fn extract_api_key(req: &Request) -> Option<String> {
    // Try Authorization: Bearer <key>
    if let Some(auth) = req.headers().get("authorization") {
        if let Ok(val) = auth.to_str() {
            let val = val.trim();
            if let Some(token) = val.strip_prefix("Bearer ") {
                let token = token.trim();
                if !token.is_empty() {
                    return Some(token.to_string());
                }
            }
            // Also support just the raw key without "Bearer " prefix
            if let Some(token) = val.strip_prefix("bearer ") {
                let token = token.trim();
                if !token.is_empty() {
                    return Some(token.to_string());
                }
            }
        }
    }

    // Try x-api-key header
    if let Some(key) = req.headers().get("x-api-key") {
        if let Ok(val) = key.to_str() {
            let val = val.trim();
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }

    None
}

/// Return 401 Unauthorized in OpenAI-compatible error format.
fn unauthorized_response() -> Response {
    let body = Json(json!({
        "error": {
            "message": "Invalid API key. Provide a valid key via Authorization: Bearer <key> or x-api-key header.",
            "type": "invalid_request_error",
            "param": null,
            "code": "invalid_api_key"
        }
    }));

    (StatusCode::UNAUTHORIZED, body).into_response()
}
