use axum::{
    Json, Router,
    extract::{ConnectInfo, Query, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;

use crate::proxy::ProxyState;

pub fn router(state: Arc<ProxyState>) -> Router<Arc<ProxyState>> {
    Router::new()
        .route("/status", get(status))
        .route("/config", get(get_config))
        // ── API key CRUD ─────────────────────────────────────────────────────
        .route(
            "/api-keys",
            get(get_api_keys)
                .put(put_api_keys)
                .patch(patch_api_keys)
                .delete(delete_api_keys),
        )
        // ── Management auth layer (applied to all routes above) ──────────────
        .layer(middleware::from_fn_with_state(state, management_auth))
}

// ── Management auth middleware ───────────────────────────────────────────────

/// Middleware that gates all management routes behind `remote-management.secret-key`.
///
/// Rules (matching Go CLIProxy):
/// - If `secret-key` is empty → 404 all management routes (disabled).
/// - Caller must provide the secret via `Authorization: Bearer <secret>`
///   or `X-Management-Key: <secret>`.
/// - If `allow-remote: false` (default), only localhost connections are allowed.
async fn management_auth(
    State(state): State<Arc<ProxyState>>,
    req: Request,
    next: Next,
) -> Response {
    let cfg = state.config.read().await;
    let secret = cfg.remote_management.secret_key.clone();
    let allow_remote = cfg.remote_management.allow_remote;
    drop(cfg);
    // No secret configured → management API disabled
    if secret.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "management API disabled — set remote-management.secret-key in config"})),
        )
            .into_response();
    }

    // Localhost check when allow-remote is false
    if !allow_remote {
        let is_local = req
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ci| ci.0.ip().is_loopback())
            .unwrap_or(false);

        if !is_local {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"error": "management API restricted to localhost — set remote-management.allow-remote: true to allow"})),
            )
                .into_response();
        }
    }

    // Validate secret key from headers
    let provided = extract_management_key(&req);
    match provided {
        Some(key) if key == secret => next.run(req).await,
        _ => (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "invalid management key — provide via Authorization: Bearer <key> or X-Management-Key header"})),
        )
            .into_response(),
    }
}

/// Extract management secret from request headers.
/// Checks `Authorization: Bearer <key>` first, then `X-Management-Key`.
fn extract_management_key(req: &Request) -> Option<String> {
    if let Some(auth) = req.headers().get("authorization") {
        if let Ok(val) = auth.to_str() {
            let val = val.trim();
            if let Some(token) = val
                .strip_prefix("Bearer ")
                .or_else(|| val.strip_prefix("bearer "))
            {
                let token = token.trim();
                if !token.is_empty() {
                    return Some(token.to_string());
                }
            }
        }
    }

    if let Some(key) = req.headers().get("x-management-key") {
        if let Ok(val) = key.to_str() {
            let val = val.trim();
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }

    None
}

// ── Existing handlers ────────────────────────────────────────────────────────

async fn status(State(state): State<Arc<ProxyState>>) -> Json<Value> {
    let cfg = state.config.read().await;
    Json(json!({
        "status": "running",
        "port": cfg.port,
        "providers": state.providers.len(),
    }))
}

async fn get_config(State(state): State<Arc<ProxyState>>) -> Json<Value> {
    let cfg = state.config.read().await;
    Json(json!({
        "port": cfg.port,
        "host": cfg.host,
        "debug": cfg.debug,
    }))
}

// ── API key management ───────────────────────────────────────────────────────

/// Generate a random API key in `rsk-<uuid>` format.
fn generate_api_key() -> String {
    format!("rsk-{}", uuid::Uuid::new_v4())
}

/// `GET /v0/management/api-keys` — list all configured API keys.
async fn get_api_keys(State(state): State<Arc<ProxyState>>) -> Json<Value> {
    let cfg = state.config.read().await;
    Json(json!({ "api-keys": cfg.api_keys }))
}

/// `PUT /v0/management/api-keys` — replace the entire API key list.
///
/// Body: `["key1", "key2"]` or `{"items": ["key1", "key2"]}`
async fn put_api_keys(
    State(state): State<Arc<ProxyState>>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let keys = parse_string_list(&body);
    let Some(keys) = keys else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid body — expected [\"key\", ...] or {\"items\": [...]}"})),
        );
    };

    let mut cfg = state.config.write().await;
    cfg.api_keys = keys.clone();
    (StatusCode::OK, Json(json!({ "api-keys": keys })))
}

/// `PATCH /v0/management/api-keys` — add, update, or generate a key.
///
/// Body variants:
/// - `{"generate": true}` — auto-generate a new key and append it
/// - `{"generate": true, "count": 3}` — generate N keys
/// - `{"value": "new-key"}` — append a key
/// - `{"old": "x", "new": "y"}` — replace key x with y
/// - `{"index": 0, "value": "y"}` — replace key at index
#[derive(Deserialize)]
struct PatchBody {
    generate: Option<bool>,
    count: Option<usize>,
    value: Option<String>,
    old: Option<String>,
    new: Option<String>,
    index: Option<usize>,
}

async fn patch_api_keys(
    State(state): State<Arc<ProxyState>>,
    Json(body): Json<PatchBody>,
) -> impl IntoResponse {
    let mut cfg = state.config.write().await;

    // Generate mode
    if body.generate.unwrap_or(false) {
        let count = body.count.unwrap_or(1).clamp(1, 50);
        let generated: Vec<String> = (0..count).map(|_| generate_api_key()).collect();
        cfg.api_keys.extend(generated.clone());
        return (
            StatusCode::OK,
            Json(json!({
                "generated": generated,
                "api-keys": cfg.api_keys,
            })),
        );
    }

    // Replace by index
    if let (Some(idx), Some(val)) = (body.index, &body.value) {
        if idx < cfg.api_keys.len() {
            cfg.api_keys[idx] = val.clone();
            return (StatusCode::OK, Json(json!({ "api-keys": cfg.api_keys })));
        }
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "index out of range"})),
        );
    }

    // Replace by old → new
    if let (Some(old), Some(new)) = (&body.old, &body.new) {
        if let Some(pos) = cfg.api_keys.iter().position(|k| k == old) {
            cfg.api_keys[pos] = new.clone();
        } else {
            // Key not found — append as new
            cfg.api_keys.push(new.clone());
        }
        return (StatusCode::OK, Json(json!({ "api-keys": cfg.api_keys })));
    }

    // Append
    if let Some(val) = &body.value {
        let val = val.trim().to_string();
        if val.is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "value must not be empty"})),
            );
        }
        cfg.api_keys.push(val);
        return (StatusCode::OK, Json(json!({ "api-keys": cfg.api_keys })));
    }

    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error": "missing fields — use generate, value, old/new, or index/value"})),
    )
}

/// `DELETE /v0/management/api-keys` — remove key(s).
///
/// Query params:
/// - `?value=rsk-xxx` — remove by value
/// - `?index=0` — remove by index
/// - `?all=true` — remove all keys (⚠️ disables auth)
#[derive(Deserialize)]
struct DeleteQuery {
    value: Option<String>,
    index: Option<usize>,
    all: Option<bool>,
}

async fn delete_api_keys(
    State(state): State<Arc<ProxyState>>,
    Query(q): Query<DeleteQuery>,
) -> impl IntoResponse {
    let mut cfg = state.config.write().await;

    if q.all.unwrap_or(false) {
        cfg.api_keys.clear();
        return (StatusCode::OK, Json(json!({ "api-keys": cfg.api_keys })));
    }

    if let Some(idx) = q.index {
        if idx < cfg.api_keys.len() {
            cfg.api_keys.remove(idx);
            return (StatusCode::OK, Json(json!({ "api-keys": cfg.api_keys })));
        }
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "index out of range"})),
        );
    }

    if let Some(val) = &q.value {
        let before = cfg.api_keys.len();
        cfg.api_keys.retain(|k| k != val);
        if cfg.api_keys.len() == before {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "key not found"})),
            );
        }
        return (StatusCode::OK, Json(json!({ "api-keys": cfg.api_keys })));
    }

    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error": "missing query param — use value, index, or all"})),
    )
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Parse body as `["a","b"]` or `{"items": ["a","b"]}`.
fn parse_string_list(body: &Value) -> Option<Vec<String>> {
    // Direct array
    if let Some(arr) = body.as_array() {
        let strings: Vec<String> = arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect();
        return Some(strings);
    }
    // Wrapped: {"items": [...]}
    if let Some(items) = body.get("items").and_then(|v| v.as_array()) {
        let strings: Vec<String> = items
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect();
        return Some(strings);
    }
    None
}
