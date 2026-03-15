use crate::auth::store::AuthStatus;
use crate::proxy::ProxyState;
use axum::{
    extract::{ConnectInfo, Query, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, patch},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tower_http::limit::RequestBodyLimitLayer;

/// Max upload body size for auth files (1 MiB).
const MAX_UPLOAD_BYTES: usize = 1024 * 1024;
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
        // ── Auth file CRUD ───────────────────────────────────────────────────
        .route(
            "/auth-files",
            get(list_auth_files)
                .post(upload_auth_file)
                .delete(delete_auth_file),
        )
        .route("/auth-files/download", get(download_auth_file))
        .route("/auth-files/status", patch(patch_auth_file_status))
        .route("/auth-files/fields", patch(patch_auth_file_fields))
        // ── OAuth trigger ────────────────────────────────────────────────────
        .route("/oauth/start", get(crate::proxy::oauth::start_oauth))
        .route("/oauth/status", get(crate::proxy::oauth::get_auth_status))
        // ── Layers ───────────────────────────────────────────────────────────
        .layer(RequestBodyLimitLayer::new(MAX_UPLOAD_BYTES))
        .layer(middleware::from_fn(rate_limit))
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

// ── Rate limiting middleware ───────────────────────────────────────────────────

/// Per-IP rate limit: 60 requests per 60 seconds (sliding window).
const RATE_LIMIT_MAX: u32 = 60;
const RATE_LIMIT_WINDOW_SECS: u64 = 60;

static RATE_LIMITER: std::sync::LazyLock<Mutex<HashMap<IpAddr, (Instant, u32)>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Simple per-IP rate limiter for management endpoints.
///
/// Tracks request counts per IP in a sliding window. Returns 429 when exceeded.
/// Falls back to global rate limiting when `ConnectInfo` is unavailable.
async fn rate_limit(req: Request, next: Next) -> Response {
    let ip = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip())
        .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));

    {
        let mut map = RATE_LIMITER.lock().await;
        let now = Instant::now();
        let entry = map.entry(ip).or_insert((now, 0));

        // Reset window if expired
        if now.duration_since(entry.0).as_secs() >= RATE_LIMIT_WINDOW_SECS {
            *entry = (now, 0);
        }

        entry.1 += 1;

        if entry.1 > RATE_LIMIT_MAX {
            let retry_after =
                RATE_LIMIT_WINDOW_SECS.saturating_sub(now.duration_since(entry.0).as_secs());
            return (
                StatusCode::TOO_MANY_REQUESTS,
                [(
                    axum::http::header::RETRY_AFTER,
                    retry_after
                        .to_string()
                        .parse::<axum::http::HeaderValue>()
                        .unwrap_or_else(|_| axum::http::HeaderValue::from_static("60")),
                )],
                Json(json!({
                    "error": "rate limit exceeded — max 60 requests per minute",
                    "retry_after": retry_after,
                })),
            )
                .into_response();
        }
    }

    next.run(req).await
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

/// Validate an auth-file name against path-traversal and injection attacks.
///
/// Rejects:
/// - empty / whitespace-only names
/// - path separators (`/`, `\`)
/// - parent-directory traversal (`..`)
/// - null bytes
/// - non-ASCII characters
/// - names not ending with `.json`
///
/// Returns the trimmed name on success, or a 400 error response.
fn sanitize_filename(raw: &str) -> Result<String, (StatusCode, Json<Value>)> {
    let name = raw.trim().to_string();

    if name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "name must not be empty"})),
        ));
    }

    // Reject path separators, parent traversal, null bytes, non-ASCII
    if name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.contains('\0')
        || !name.is_ascii()
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({"error": "invalid filename — must be ASCII, no path separators, no '..' or null bytes"}),
            ),
        ));
    }

    if !name.to_lowercase().ends_with(".json") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "name must end with .json"})),
        ));
    }

    Ok(name)
}

// ── Auth file CRUD ───────────────────────────────────────────────────────────

/// `GET /v0/management/auth-files` — list all auth files with metadata.
async fn list_auth_files(State(state): State<Arc<ProxyState>>) -> impl IntoResponse {
    let store = state.accounts.store();
    match store.list().await {
        Ok(records) => {
            let files: Vec<Value> = records
                .iter()
                .map(|r| {
                    let size = std::fs::metadata(&r.path).map(|m| m.len()).unwrap_or(0);
                    json!({
                        "id": r.id,
                        "type": r.provider,
                        "email": r.email().unwrap_or(""),
                        "project_id": r.project_id().unwrap_or(""),
                        "status": r.effective_status().to_string(),
                        "disabled": r.disabled,
                        "size": size,
                        "updated_at": r.updated_at.to_rfc3339(),
                        "last_refreshed_at": r.last_refreshed_at.map(|t| t.to_rfc3339()),
                    })
                })
                .collect();
            (StatusCode::OK, Json(json!({ "auth-files": files }))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("failed to list auth files: {e}") })),
        )
            .into_response(),
    }
}

/// `POST /v0/management/auth-files?name=x.json` — upload auth file.
///
/// Body: raw JSON. Saves to auth-dir and reloads accounts.
#[derive(Deserialize)]
struct UploadQuery {
    name: String,
}

async fn upload_auth_file(
    State(state): State<Arc<ProxyState>>,
    Query(q): Query<UploadQuery>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let name = match sanitize_filename(&q.name) {
        Ok(n) => n,
        Err(e) => return e,
    };

    // Validate JSON
    if serde_json::from_slice::<Value>(&body).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "body is not valid JSON"})),
        );
    }

    let dir = state.accounts.store().base_dir().await;
    let path = dir.join(&name);

    // Ensure dir exists
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("create auth dir: {e}")})),
        );
    }

    if let Err(e) = tokio::fs::write(&path, &body).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("write file: {e}")})),
        );
    }

    // Set permissions
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).await;
    }

    // Reload accounts
    if let Err(e) = state.accounts.reload().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("reload accounts: {e}")})),
        );
    }

    (StatusCode::OK, Json(json!({"status": "ok", "name": name})))
}

/// `DELETE /v0/management/auth-files` — delete auth file(s).
///
/// Query: `?name=x.json` or `?all=true`
#[derive(Deserialize)]
struct DeleteAuthQuery {
    name: Option<String>,
    all: Option<bool>,
}

async fn delete_auth_file(
    State(state): State<Arc<ProxyState>>,
    Query(q): Query<DeleteAuthQuery>,
) -> impl IntoResponse {
    let store = state.accounts.store();
    let dir = store.base_dir().await;

    if q.all.unwrap_or(false) {
        // Delete all .json files
        let mut deleted = 0u32;
        if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "json")
                    && tokio::fs::remove_file(&path).await.is_ok()
                {
                    deleted += 1;
                }
            }
        }
        if let Err(e) = state.accounts.reload().await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("reload accounts: {e}")})),
            );
        }
        return (
            StatusCode::OK,
            Json(json!({"status": "ok", "deleted": deleted})),
        );
    }

    let name = match sanitize_filename(q.name.as_deref().unwrap_or("")) {
        Ok(n) => n,
        Err(e) => return e,
    };

    let path = dir.join(&name);
    if !path.exists() {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "file not found"})),
        );
    }

    if let Err(e) = tokio::fs::remove_file(&path).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("delete file: {e}")})),
        );
    }

    if let Err(e) = state.accounts.reload().await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("reload accounts: {e}")})),
        );
    }

    (StatusCode::OK, Json(json!({"status": "ok", "name": name})))
}

/// `GET /v0/management/auth-files/download?name=x.json` — download raw JSON.
#[derive(Deserialize)]
struct DownloadQuery {
    name: String,
}

async fn download_auth_file(
    State(state): State<Arc<ProxyState>>,
    Query(q): Query<DownloadQuery>,
) -> Response {
    let name = match sanitize_filename(&q.name) {
        Ok(n) => n,
        Err((status, json)) => return (status, json).into_response(),
    };

    let dir = state.accounts.store().base_dir().await;
    let path = dir.join(&name);

    match tokio::fs::read(&path).await {
        Ok(data) => {
            let disposition = format!("attachment; filename=\"{name}\"");
            (
                StatusCode::OK,
                [
                    (axum::http::header::CONTENT_TYPE, "application/json"),
                    (
                        axum::http::header::CONTENT_DISPOSITION,
                        disposition.as_str(),
                    ),
                ],
                data,
            )
                .into_response()
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "file not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("read file: {e}")})),
        )
            .into_response(),
    }
}

/// `PATCH /v0/management/auth-files/status` — toggle disabled state.
///
/// Body: `{"name": "x.json", "disabled": true}`
#[derive(Deserialize)]
struct PatchStatusBody {
    name: String,
    disabled: Option<bool>,
}

async fn patch_auth_file_status(
    State(state): State<Arc<ProxyState>>,
    Json(body): Json<PatchStatusBody>,
) -> impl IntoResponse {
    let name = match sanitize_filename(&body.name) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let Some(disabled) = body.disabled else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "disabled is required"})),
        );
    };

    // Read file, update disabled field, write back
    let dir = state.accounts.store().base_dir().await;
    let path = dir.join(&name);

    let data = match tokio::fs::read_to_string(&path).await {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "file not found"})),
            );
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("read file: {e}")})),
            );
        }
    };

    let mut metadata: serde_json::Map<String, Value> = match serde_json::from_str(&data) {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("parse file: {e}")})),
            );
        }
    };

    metadata.insert("disabled".into(), json!(disabled));
    let status = if disabled {
        AuthStatus::Disabled
    } else {
        AuthStatus::Active
    };
    metadata.insert("status".into(), json!(status.to_string()));

    let json_str = match serde_json::to_string_pretty(&metadata) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("serialize: {e}")})),
            );
        }
    };

    if let Err(e) = tokio::fs::write(&path, json_str).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("write file: {e}")})),
        );
    }

    // Reload
    let _ = state.accounts.reload().await;

    (
        StatusCode::OK,
        Json(json!({"status": "ok", "name": name, "disabled": disabled})),
    )
}

/// `PATCH /v0/management/auth-files/fields` — update editable fields.
///
/// Body: `{"name": "x.json", "prefix": "...", "proxy_url": "...", "priority": 1}`
#[derive(Deserialize)]
struct PatchFieldsBody {
    name: String,
    prefix: Option<String>,
    proxy_url: Option<String>,
    priority: Option<i32>,
}

async fn patch_auth_file_fields(
    State(state): State<Arc<ProxyState>>,
    Json(body): Json<PatchFieldsBody>,
) -> impl IntoResponse {
    let name = match sanitize_filename(&body.name) {
        Ok(n) => n,
        Err(e) => return e,
    };

    let dir = state.accounts.store().base_dir().await;
    let path = dir.join(&name);

    let data = match tokio::fs::read_to_string(&path).await {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "file not found"})),
            );
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("read file: {e}")})),
            );
        }
    };

    let mut metadata: serde_json::Map<String, Value> = match serde_json::from_str(&data) {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("parse file: {e}")})),
            );
        }
    };

    let mut changed = false;
    if let Some(ref prefix) = body.prefix {
        metadata.insert("prefix".into(), json!(prefix));
        changed = true;
    }
    if let Some(ref proxy_url) = body.proxy_url {
        metadata.insert("proxy_url".into(), json!(proxy_url));
        changed = true;
    }
    if let Some(priority) = body.priority {
        metadata.insert("priority".into(), json!(priority));
        changed = true;
    }

    if !changed {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "no fields to update — use prefix, proxy_url, or priority"})),
        );
    }

    let json_str = match serde_json::to_string_pretty(&metadata) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("serialize: {e}")})),
            );
        }
    };

    if let Err(e) = tokio::fs::write(&path, json_str).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("write file: {e}")})),
        );
    }

    // Reload
    let _ = state.accounts.reload().await;

    (StatusCode::OK, Json(json!({"status": "ok", "name": name})))
}
