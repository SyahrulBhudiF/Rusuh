use crate::auth::codex_runtime::{
    parse_codex_retry_after_seconds, refresh_tokens_with_retry, token_needs_refresh,
};
use crate::auth::kiro::{KiroTokenData, KiroTokenSource, DEFAULT_REGION};
use crate::auth::kiro_login::SocialAuthClient;
use crate::auth::kiro_record::KiroRecordInput;
use crate::auth::store::{AuthRecord, AuthStatus};
use crate::auth::zed::{canonical_zed_login_filename, zed_user_ids_match};
use crate::auth::zed_callback::start_callback_server;
use crate::auth::zed_login::{build_login_url, decrypt_credential, generate_keypair};
use crate::auth::zed_session::{cleanup_expired_sessions, ZedLoginSession, ZedLoginSessionStatus};
use crate::error::{AppError, AppResult};
use crate::proxy::ProxyState;

async fn refresh_runtime_after_auth_change(state: &Arc<ProxyState>) -> AppResult<()> {
    state.accounts.reload().await?;
    state.refresh_provider_runtime().await?;
    Ok(())
}
use axum::{
    extract::{ConnectInfo, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, patch},
    Json, Router,
};
use reqwest::header::{ACCEPT, AUTHORIZATION, CONNECTION, CONTENT_TYPE, USER_AGENT};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::error::Error as StdError;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};
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
        .route(
            "/oauth-callback",
            axum::routing::post(crate::proxy::oauth::post_oauth_callback),
        )
        .route("/oauth/status", get(crate::proxy::oauth::get_auth_status))
        .route(
            "/kiro/builder-id/start",
            axum::routing::post(crate::proxy::oauth::start_builder_id_login),
        )
        .route("/kiro/import", axum::routing::post(import_kiro_auth_file))
        .route(
            "/kiro/social/import",
            axum::routing::post(import_kiro_social_refresh_token),
        )
        .route("/kiro/check-quota", axum::routing::post(check_kiro_quota))
        // ── Codex ────────────────────────────────────────────────────────────
        .route("/codex/check-quota", axum::routing::post(check_codex_quota))
        // ── Zed ──────────────────────────────────────────────────────────────
        .route(
            "/zed/login/initiate",
            axum::routing::post(initiate_zed_login),
        )
        .route(
            "/zed/login/status",
            axum::routing::get(get_zed_login_status),
        )
        .route("/zed/import", axum::routing::post(import_zed_credential))
        .route("/zed/check-quota", axum::routing::post(check_zed_quota))
        .route("/zed/models", axum::routing::post(list_zed_models))
        .route(
            "/github-copilot/models",
            axum::routing::post(list_github_copilot_models),
        )
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
            .unwrap_or(true);
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
    let provider_count = state.current_runtime_snapshot().await.provider_count();
    Json(json!({
        "status": "running",
        "port": cfg.port,
        "providers": provider_count,
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
                        "provider_key": r.provider_key,
                        "label": r.label,
                        "auth_method": r.metadata.get("auth_method").and_then(|v| v.as_str()),
                        "provider": r.metadata.get("provider").and_then(|v| v.as_str()),
                        "region": r.metadata.get("region").and_then(|v| v.as_str()),
                        "start_url": r.metadata.get("start_url").and_then(|v| v.as_str()),
                        "profile_arn": r.metadata.get("profile_arn").and_then(|v| v.as_str()),
                        "email": r.email().unwrap_or(""),
                        "project_id": r.project_id().unwrap_or(""),
                        "status": r.effective_status().to_string(),
                        "status_message": r.status_message,
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

    let refresh_warning = match refresh_runtime_after_auth_change(&state).await {
        Ok(()) => None,
        Err(error) => {
            tracing::warn!("auth file upload runtime refresh failed for {}: {}", name, error);
            Some(error.to_string())
        }
    };

    let mut response = json!({"status": "ok", "name": name});
    if let Some(warning) = refresh_warning {
        response["warning"] = json!(format!(
            "auth file saved but runtime refresh failed: {warning}"
        ));
    }

    (StatusCode::OK, Json(response))
}

#[derive(Deserialize)]
struct ImportKiroBody {
    access_token: Option<String>,
    refresh_token: Option<String>,
    profile_arn: Option<String>,
    expires_at: Option<String>,
    auth_method: Option<String>,
    provider: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    region: Option<String>,
    start_url: Option<String>,
    email: Option<String>,
    label: Option<String>,
}

#[derive(Deserialize)]
struct ImportKiroSocialBody {
    refresh_token: Option<String>,
    label: Option<String>,
}

async fn import_kiro_social_refresh_token(
    State(state): State<Arc<ProxyState>>,
    Json(body): Json<ImportKiroSocialBody>,
) -> impl IntoResponse {
    let refresh_token = body
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    let Some(refresh_token) = refresh_token else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "refresh_token is required"})),
        );
    };

    if !refresh_token.starts_with("aorAAAAAG") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid token format. token should start with aorAAAAAG..."})),
        );
    }

    let mut token_data = match SocialAuthClient::new()
        .refresh_social_token(&refresh_token)
        .await
    {
        Ok(data) => data,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("token validation failed: {error}")})),
            );
        }
    };

    if token_data.refresh_token.trim().is_empty() {
        token_data.refresh_token = refresh_token;
    }
    token_data.auth_method = "social".to_string();
    token_data.provider = "imported".to_string();

    let record = KiroRecordInput {
        token_data,
        label: body
            .label
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        source: KiroTokenSource::LegacySocial,
    }
    .into_auth_record();

    let name = record.id.clone();
    let label = record.label.clone();
    let auth_method = record
        .metadata
        .get("auth_method")
        .and_then(Value::as_str)
        .unwrap_or("social")
        .to_string();
    let provider = record
        .metadata
        .get("provider")
        .and_then(Value::as_str)
        .unwrap_or("imported")
        .to_string();

    if let Err(error) = state.accounts.store().save(&record).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("save credential: {error}")})),
        );
    }

    if let Err(error) = refresh_runtime_after_auth_change(&state).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string()})),
        );
    }

    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "name": name,
            "provider_key": "kiro",
            "label": label,
            "auth_method": auth_method,
            "provider": provider,
        })),
    )
}

async fn import_kiro_auth_file(
    State(state): State<Arc<ProxyState>>,
    Json(body): Json<ImportKiroBody>,
) -> impl IntoResponse {
    let access_token = body
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let refresh_token = body
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let expires_at = body
        .expires_at
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let client_id = body
        .client_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let client_secret = body
        .client_secret
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    let mut missing = Vec::new();
    if access_token.is_none() {
        missing.push("access_token");
    }
    if refresh_token.is_none() {
        missing.push("refresh_token");
    }
    if expires_at.is_none() {
        missing.push("expires_at");
    }
    if client_id.is_none() {
        missing.push("client_id");
    }
    if client_secret.is_none() {
        missing.push("client_secret");
    }
    if !missing.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("missing required fields: {}", missing.join(", "))
            })),
        );
    }

    let expires_at = expires_at.unwrap();
    if crate::auth::kiro::parse_expiry_str(&expires_at).is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "expires_at must be RFC3339"})),
        );
    }

    let token_data = KiroTokenData {
        access_token: access_token.unwrap(),
        refresh_token: refresh_token.unwrap(),
        profile_arn: body.profile_arn.unwrap_or_default().trim().to_string(),
        expires_at,
        auth_method: body
            .auth_method
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("import")
            .to_string(),
        provider: body
            .provider
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("AWS")
            .to_string(),
        client_id,
        client_secret,
        region: body
            .region
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_REGION)
            .to_string(),
        start_url: body
            .start_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        email: body
            .email
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
    };

    let record = KiroRecordInput {
        token_data,
        label: body
            .label
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        source: KiroTokenSource::Import,
    }
    .into_auth_record();

    let name = record.id.clone();
    let label = record.label.clone();
    let auth_method = record
        .metadata
        .get("auth_method")
        .and_then(Value::as_str)
        .unwrap_or("import")
        .to_string();
    let provider = record
        .metadata
        .get("provider")
        .and_then(Value::as_str)
        .unwrap_or("AWS")
        .to_string();

    if let Err(e) = state.accounts.store().save(&record).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("save auth file: {e}")})),
        );
    }
    if let Err(error) = refresh_runtime_after_auth_change(&state).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string()})),
        );
    }

    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "name": name,
            "provider_key": "kiro",
            "label": label,
            "auth_method": auth_method,
            "provider": provider,
        })),
    )
}

// ── Kiro quota check ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CheckKiroQuotaBody {
    name: Option<String>,
}

/// `POST /v0/management/kiro/check-quota` — check quota for a Kiro auth file.
async fn check_kiro_quota(
    State(state): State<Arc<ProxyState>>,
    Json(body): Json<CheckKiroQuotaBody>,
) -> impl IntoResponse {
    let name = match sanitize_filename(body.name.as_deref().unwrap_or("")) {
        Ok(n) => n,
        Err(e) => return e,
    };

    let records = match state.accounts.store().list().await {
        Ok(records) => records,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("failed to list auth files: {error}")})),
            );
        }
    };

    let mut record = match records
        .into_iter()
        .find(|record| record.id == name && record.provider_key.eq_ignore_ascii_case("kiro"))
    {
        Some(record) => record,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Kiro auth file not found"})),
            );
        }
    };

    // Try quota check with current token
    let status = check_quota_with_refresh(&state, &mut record).await;

    let response = match status {
        crate::auth::kiro_runtime::QuotaStatus::Unknown => json!({
            "status": "unknown"
        }),
        crate::auth::kiro_runtime::QuotaStatus::Available {
            remaining,
            next_reset,
            breakdown,
        } => {
            let mut resp = json!({
                "status": "available",
                "remaining": remaining,
                "next_reset": next_reset,
            });
            if let Some(bd) = breakdown {
                resp["breakdown"] = json!({
                    "base_remaining": bd.base_remaining,
                    "free_trial_remaining": bd.free_trial_remaining,
                });
                if let Some(title) = bd.subscription_title {
                    resp["subscription_title"] = json!(title);
                }
            }
            resp
        }
        crate::auth::kiro_runtime::QuotaStatus::Exhausted { detail } => json!({
            "status": "exhausted",
            "detail": detail,
        }),
    };

    (StatusCode::OK, Json(response))
}

/// Check quota with automatic token refresh on 403 errors.
async fn check_quota_with_refresh(
    state: &Arc<ProxyState>,
    record: &mut crate::auth::store::AuthRecord,
) -> crate::auth::kiro_runtime::QuotaStatus {
    use crate::auth::kiro_runtime::UsageCheckRequest;

    let access_token = record
        .metadata
        .get("access_token")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let profile_arn = record
        .metadata
        .get("profile_arn")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if access_token.is_empty() {
        return crate::auth::kiro_runtime::QuotaStatus::Unknown;
    }

    let client_id = record
        .metadata
        .get("client_id")
        .and_then(|v| v.as_str())
        .map(String::from);
    let refresh_token = record
        .metadata
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .map(String::from);

    let request = UsageCheckRequest {
        access_token: access_token.clone(),
        profile_arn: profile_arn.clone(),
        client_id: client_id.clone(),
        refresh_token: refresh_token.clone(),
    };

    // First attempt
    let status = state.kiro_runtime.quota_checker.check_quota(&request).await;

    // If Unknown (likely 403), attempt token refresh and retry once
    if matches!(status, crate::auth::kiro_runtime::QuotaStatus::Unknown) {
        if let Some(refreshed) = attempt_token_refresh(record).await {
            // Update record with refreshed token
            record.metadata.insert(
                "access_token".to_string(),
                serde_json::json!(refreshed.access_token),
            );
            if !refreshed.refresh_token.is_empty() {
                record.metadata.insert(
                    "refresh_token".to_string(),
                    serde_json::json!(refreshed.refresh_token),
                );
            }
            record.metadata.insert(
                "expires_at".to_string(),
                serde_json::json!(refreshed.expires_at),
            );

            // Save to disk
            if let Err(e) = state.accounts.store().save(record).await {
                tracing::warn!("failed to save refreshed token: {}", e);
            } else {
                tracing::info!("refreshed Kiro token for {}", record.id);
            }

            // Retry quota check with new token
            let retry_request = UsageCheckRequest {
                access_token: refreshed.access_token,
                profile_arn,
                client_id,
                refresh_token: Some(refreshed.refresh_token),
            };
            return state
                .kiro_runtime
                .quota_checker
                .check_quota(&retry_request)
                .await;
        }
    }

    status
}

/// Attempt to refresh a Kiro token based on its auth method.
async fn attempt_token_refresh(record: &crate::auth::store::AuthRecord) -> Option<RefreshedToken> {
    use crate::auth::kiro_login::{SSOOIDCClient, SocialAuthClient};

    let auth_method = record
        .metadata
        .get("auth_method")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let refresh_token = record
        .metadata
        .get("refresh_token")
        .and_then(|v| v.as_str())?;

    let client_id = record.metadata.get("client_id").and_then(|v| v.as_str());

    let client_secret = record
        .metadata
        .get("client_secret")
        .and_then(|v| v.as_str());

    let region = record
        .metadata
        .get("region")
        .and_then(|v| v.as_str())
        .unwrap_or("us-east-1");

    match auth_method.to_lowercase().as_str() {
        "builder-id" => {
            // Builder ID uses SSO OIDC with default region and Builder ID start URL
            if let (Some(cid), Some(secret)) = (client_id, client_secret) {
                let client = SSOOIDCClient::new();
                let start_url = "https://view.awsapps.com/start";
                match client
                    .refresh_token_with_region(cid, secret, refresh_token, region, start_url)
                    .await
                {
                    Ok(response) => {
                        return Some(RefreshedToken {
                            access_token: response.access_token,
                            refresh_token: response.refresh_token,
                            expires_at: response.expires_at,
                        });
                    }
                    Err(e) => {
                        tracing::warn!("Builder ID token refresh failed: {}", e);
                    }
                }
            }
        }
        "idc" => {
            // IDC uses SSO OIDC with region and start_url
            if let (Some(cid), Some(secret)) = (client_id, client_secret) {
                let start_url = record
                    .metadata
                    .get("start_url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let client = SSOOIDCClient::new();
                match client
                    .refresh_token_with_region(cid, secret, refresh_token, region, start_url)
                    .await
                {
                    Ok(response) => {
                        return Some(RefreshedToken {
                            access_token: response.access_token,
                            refresh_token: response.refresh_token,
                            expires_at: response.expires_at,
                        });
                    }
                    Err(e) => {
                        tracing::warn!("IDC token refresh failed: {}", e);
                    }
                }
            }
        }
        _ => {
            // Social auth (Google/GitHub) or legacy
            let client = SocialAuthClient::new();
            match client.refresh_social_token(refresh_token).await {
                Ok(token_data) => {
                    return Some(RefreshedToken {
                        access_token: token_data.access_token,
                        refresh_token: token_data.refresh_token,
                        expires_at: token_data.expires_at,
                    });
                }
                Err(e) => {
                    tracing::warn!("Social token refresh failed: {}", e);
                }
            }
        }
    }

    None
}

struct RefreshedToken {
    access_token: String,
    refresh_token: String,
    expires_at: String,
}

const CODEX_DEFAULT_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";
const CODEX_VERSION: &str = "0.101.0";
const CODEX_ORIGINATOR: &str = "codex_cli_rs";
const CODEX_USER_AGENT: &str = "codex_cli_rs/0.101.0";

fn codex_base_url_from_record(record: &AuthRecord) -> String {
    record
        .metadata
        .get("base_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(CODEX_DEFAULT_BASE_URL)
        .trim_end_matches('/')
        .to_string()
}

fn codex_account_id_from_record(record: &AuthRecord) -> String {
    record
        .metadata
        .get("account_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(record.id.as_str())
        .to_string()
}

fn build_codex_headers(
    access_token: &str,
    account_id: &str,
    accept: &str,
) -> Result<HeaderMap, AppError> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {access_token}"))
            .map_err(|error| AppError::Auth(format!("invalid authorization header: {error}")))?,
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_str(accept)
            .map_err(|error| AppError::Auth(format!("invalid accept header: {error}")))?,
    );
    headers.insert(CONNECTION, HeaderValue::from_static("Keep-Alive"));
    headers.insert("originator", HeaderValue::from_static(CODEX_ORIGINATOR));
    headers.insert(
        "chatgpt-account-id",
        HeaderValue::from_str(account_id)
            .map_err(|error| AppError::Auth(format!("invalid account id header: {error}")))?,
    );
    headers.insert(
        "session_id",
        HeaderValue::from_str(&uuid::Uuid::new_v4().to_string())
            .map_err(|error| AppError::Auth(format!("invalid session header: {error}")))?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static(CODEX_USER_AGENT));
    headers.insert("version", HeaderValue::from_static(CODEX_VERSION));
    Ok(headers)
}

// ── Codex quota check ────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CheckCodexQuotaBody {
    name: Option<String>,
}

/// `POST /v0/management/codex/check-quota` — check quota for a Codex auth file.
async fn check_codex_quota(
    State(_state): State<Arc<ProxyState>>,
    Json(body): Json<CheckCodexQuotaBody>,
) -> impl IntoResponse {
    let name = match sanitize_filename(body.name.as_deref().unwrap_or("")) {
        Ok(n) => n,
        Err(e) => return e,
    };

    let records = match _state.accounts.store().list().await {
        Ok(records) => records,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("failed to list auth files: {error}")})),
            );
        }
    };

    let record = match records
        .into_iter()
        .find(|record| record.id == name && record.provider_key.eq_ignore_ascii_case("codex"))
    {
        Some(record) => record,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Codex auth file not found"})),
            );
        }
    };

    let response = probe_codex_quota(&_state, record).await;
    (StatusCode::OK, Json(response))
}

/// Probe Codex upstream API to check quota status.
async fn probe_codex_quota(state: &Arc<ProxyState>, mut record: AuthRecord) -> Value {
    let mut account = codex_account_id_from_record(&record);
    let base_url = codex_base_url_from_record(&record);
    let plan_type = record
        .metadata
        .get("plan_type")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    let expired = record
        .metadata
        .get("expired")
        .and_then(Value::as_str)
        .unwrap_or("");
    if token_needs_refresh(expired, chrono::Utc::now()) {
        let refresh_token = match record
            .metadata
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(value) => value,
            None => {
                return json!({
                    "account": account,
                    "status": "error",
                    "upstream_status": 0,
                    "detail": "codex auth file missing refresh_token",
                    "plan_type": plan_type,
                });
            }
        };

        match refresh_tokens_with_retry(&reqwest::Client::new(), refresh_token, 3).await {
            Ok(refreshed) => {
                if let Err(error) =
                    apply_refreshed_codex_record(state, &record.id, &refreshed).await
                {
                    return json!({
                        "account": account,
                        "status": "error",
                        "upstream_status": 0,
                        "detail": format!("failed to persist refreshed codex token: {error}"),
                        "plan_type": plan_type,
                    });
                }

                match state.accounts.get_by_id(&record.id).await {
                    Some(updated_record) => {
                        record = updated_record;
                        account = codex_account_id_from_record(&record);
                    }
                    None => {
                        return json!({
                            "account": account,
                            "status": "error",
                            "upstream_status": 0,
                            "detail": "codex auth record disappeared after refresh",
                            "plan_type": plan_type,
                        });
                    }
                }
            }
            Err(error) => {
                return json!({
                    "account": account,
                    "status": "error",
                    "upstream_status": 0,
                    "detail": format!("refresh failed: {error}"),
                    "plan_type": plan_type,
                });
            }
        }
    }

    let access_token = record.access_token().unwrap_or("");
    if access_token.is_empty() {
        return json!({
            "account": account,
            "status": "error",
            "upstream_status": 0,
            "detail": "codex auth file missing access_token",
            "plan_type": plan_type,
        });
    }

    let usage_url = codex_usage_url(&base_url);

    let headers = match build_codex_headers(access_token, &account, "application/json") {
        Ok(headers) => headers,
        Err(error) => {
            return json!({
                "account": account,
                "status": "error",
                "upstream_status": 0,
                "detail": format!("request setup failed: {error}"),
                "plan_type": plan_type,
            });
        }
    };

    let client = match reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return json!({
                "account": account,
                "status": "error",
                "upstream_status": 0,
                "detail": format!("request setup failed: {error}"),
                "plan_type": plan_type,
            });
        }
    };

    let result = client.get(&usage_url).headers(headers).send().await;

    match result {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body_text = resp.text().await.unwrap_or_default();
            let body_json: Value = serde_json::from_str(&body_text).unwrap_or_else(|_| json!({}));

            if let Some(retry_after_secs) =
                parse_codex_retry_after_seconds(status, &body_json, chrono::Utc::now())
            {
                let mut response = json!({
                    "account": account,
                    "status": "exhausted",
                    "upstream_status": status,
                    "retry_after_seconds": retry_after_secs,
                });
                if let Some(error_msg) = body_json
                    .get("error")
                    .and_then(|error| error.get("message"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|message| !message.is_empty())
                {
                    response["detail"] = json!(error_msg);
                }
                if let Some(plan_type) = plan_type.as_deref() {
                    response["plan_type"] = json!(plan_type);
                }
                return response;
            }

            if status == StatusCode::OK.as_u16() {
                let mut response = json!({
                    "account": account,
                    "status": codex_usage_status(&body_json),
                    "upstream_status": status,
                });
                merge_codex_usage_fields(&mut response, &body_json);
                if response.get("plan_type").is_none() {
                    if let Some(plan_type) = plan_type.as_deref() {
                        response["plan_type"] = json!(plan_type);
                    }
                }
                response
            } else {
                let detail = body_json
                    .get("error")
                    .and_then(|error| error.get("message"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|message| !message.is_empty())
                    .or_else(|| {
                        let trimmed = body_text.trim();
                        (!trimmed.is_empty()).then_some(trimmed)
                    })
                    .unwrap_or("upstream error");

                let mut response = json!({
                    "account": account,
                    "status": "error",
                    "upstream_status": status,
                    "detail": detail,
                });
                if let Some(plan_type) = plan_type.as_deref() {
                    response["plan_type"] = json!(plan_type);
                }
                response
            }
        }
        Err(error) => {
            let mut response = json!({
                "account": account,
                "status": "error",
                "detail": summarize_codex_transport_error(&error),
                "upstream_status": 0,
            });
            if let Some(plan_type) = plan_type.as_deref() {
                response["plan_type"] = json!(plan_type);
            }
            response
        }
    }
}

async fn apply_refreshed_codex_record(
    state: &Arc<ProxyState>,
    record_id: &str,
    refreshed: &crate::auth::codex::CodexTokenData,
) -> AppResult<()> {
    let refreshed_at = chrono::Utc::now();
    let refreshed_at_rfc3339 = refreshed_at.to_rfc3339();
    let updated = state
        .accounts
        .update(record_id, |record| {
            record.provider = "codex".to_string();
            record.provider_key = "codex".to_string();
            record.last_refreshed_at = Some(refreshed_at);
            record.metadata.insert("type".to_string(), json!("codex"));
            record
                .metadata
                .insert("provider_key".to_string(), json!("codex"));
            record
                .metadata
                .insert("id_token".to_string(), json!(refreshed.id_token));
            record
                .metadata
                .insert("access_token".to_string(), json!(refreshed.access_token));
            record
                .metadata
                .insert("refresh_token".to_string(), json!(refreshed.refresh_token));
            record
                .metadata
                .insert("account_id".to_string(), json!(refreshed.account_id));
            record
                .metadata
                .insert("email".to_string(), json!(refreshed.email));
            record
                .metadata
                .insert("expired".to_string(), json!(refreshed.expired));
            record.metadata.insert(
                "last_refresh".to_string(),
                json!(refreshed_at_rfc3339.clone()),
            );
            record.metadata.insert(
                "last_refreshed_at".to_string(),
                json!(refreshed_at_rfc3339.clone()),
            );
        })
        .await?;

    if !updated {
        return Err(AppError::NotFound(format!(
            "codex auth record not found: {record_id}"
        )));
    }

    refresh_runtime_after_auth_change(state).await
}

fn codex_usage_url(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if let Some(prefix) = trimmed.strip_suffix("/codex") {
        return format!("{prefix}/wham/usage");
    }
    format!("{trimmed}/wham/usage")
}

fn codex_usage_status(body_json: &Value) -> &'static str {
    let rate_limit_reached = body_json
        .get("rate_limit")
        .and_then(|value| value.get("limit_reached"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let review_limit_reached = body_json
        .get("code_review_rate_limit")
        .and_then(|value| value.get("limit_reached"))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    if rate_limit_reached || review_limit_reached {
        "exhausted"
    } else {
        "available"
    }
}

fn merge_codex_usage_fields(response: &mut Value, body_json: &Value) {
    for key in [
        "user_id",
        "account_id",
        "email",
        "plan_type",
        "rate_limit",
        "code_review_rate_limit",
        "additional_rate_limits",
        "credits",
        "spend_control",
        "promo",
    ] {
        if let Some(value) = body_json.get(key) {
            response[key] = value.clone();
        }
    }

    if body_json.is_object() && !body_json.as_object().is_some_and(|obj| obj.is_empty()) {
        response["raw_response"] = body_json.clone();
    }
}

fn summarize_codex_transport_error(error: &reqwest::Error) -> String {
    let mut parts = vec![format!("request failed: {error}")];
    let mut current = error.source();
    while let Some(source) = current {
        let rendered = source.to_string();
        if !rendered.is_empty() && !parts.iter().any(|part| part.contains(&rendered)) {
            parts.push(rendered);
        }
        current = source.source();
    }
    parts.join(" | caused by: ")
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
        if let Err(error) = refresh_runtime_after_auth_change(&state).await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": error.to_string()})),
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

    if let Err(error) = refresh_runtime_after_auth_change(&state).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string()})),
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

    if let Err(error) = refresh_runtime_after_auth_change(&state).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string()})),
        );
    }

    (
        StatusCode::OK,
        Json(json!({"status": "ok", "name": name, "disabled": disabled})),
    )
}

/// `PATCH /v0/management/auth-files/fields` — update editable fields.
///
/// Body: `{"name": "x.json", "label": "...", "prefix": "...", "proxy_url": "...", "priority": 1}`
#[derive(Deserialize)]
struct PatchFieldsBody {
    name: String,
    label: Option<String>,
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
    if let Some(ref label) = body.label {
        metadata.insert("label".into(), json!(label));
        changed = true;
    }
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
            Json(
                json!({"error": "no fields to update — use label, prefix, proxy_url, or priority"}),
            ),
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

    let refresh_warning = match refresh_runtime_after_auth_change(&state).await {
        Ok(()) => None,
        Err(error) => {
            tracing::warn!(
                "auth file field patch runtime refresh failed for {}: {}",
                name,
                error
            );
            Some(error.to_string())
        }
    };

    let mut response = json!({"status": "ok", "name": name});
    if let Some(warning) = refresh_warning {
        response["warning"] = json!(format!(
            "auth file updated but runtime refresh failed: {warning}"
        ));
    }

    (StatusCode::OK, Json(response))
}

// ── Zed credential import ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ImportZedCredentialBody {
    name: Option<String>,
    user_id: Option<String>,
    credential_json: Option<String>,
}

/// `POST /v0/management/zed/import` — import Zed credential.
async fn import_zed_credential(
    State(state): State<Arc<ProxyState>>,
    Json(body): Json<ImportZedCredentialBody>,
) -> impl IntoResponse {
    use crate::proxy::zed_import::{import_zed_credential, validated_zed_credential};

    let name = body.name.as_deref().unwrap_or("").trim();
    if name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "name is required"})),
        );
    }

    let (user_id, credential_json) =
        match validated_zed_credential(body.user_id.as_deref(), body.credential_json.as_deref()) {
            Ok(values) => values,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": e.to_string()})),
                );
            }
        };

    let cfg = state.config.read().await;
    let auth_dir = std::path::PathBuf::from(&cfg.auth_dir);
    drop(cfg);

    match import_zed_credential(&auth_dir, name, user_id, credential_json) {
        Ok(filename) => {
            let refresh_warning = match refresh_runtime_after_auth_change(&state).await {
                Ok(()) => None,
                Err(error) => {
                    tracing::warn!(
                        "zed import runtime refresh failed for {}: {}",
                        filename,
                        error
                    );
                    Some(error.to_string())
                }
            };

            let mut response = json!({
                "status": "ok",
                "filename": filename,
            });
            if let Some(warning) = refresh_warning {
                response["warning"] = json!(format!(
                    "credential imported but runtime refresh failed: {warning}"
                ));
            }

            (StatusCode::OK, Json(response))
        }
        Err(e) => {
            let msg = e.to_string();
            let status = if msg.contains("already exists") {
                StatusCode::CONFLICT
            } else if msg.contains("invalid filename") || msg.contains("name is required") {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, Json(json!({"error": msg})))
        }
    }
}

// ── Zed quota check ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CheckZedQuotaBody {
    name: Option<String>,
}

#[derive(Deserialize)]
struct InitiateZedLoginBody {
    name: Option<String>,
}

#[derive(Deserialize)]
struct ZedLoginStatusQuery {
    session_id: Option<String>,
}

/// `POST /v0/management/zed/login/initiate` — start native Zed login.
async fn initiate_zed_login(
    State(state): State<Arc<ProxyState>>,
    Json(body): Json<InitiateZedLoginBody>,
) -> impl IntoResponse {
    let session_id = uuid::Uuid::new_v4().to_string();
    let name = body
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("")
        .to_string();

    let (public_key, private_key) = match generate_keypair() {
        Ok(pair) => pair,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("generate keypair: {error}")})),
            )
        }
    };

    let (callback_state, port, server_handle) = match start_callback_server(0).await {
        Ok(result) => result,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("start callback server: {error}")})),
            )
        }
    };

    let login_url = build_login_url(&public_key, port);
    let session = ZedLoginSession::new(
        name,
        private_key,
        port,
        Arc::new(callback_state),
        server_handle,
    );

    let mut sessions = state.zed_login_sessions.lock().await;
    cleanup_expired_sessions(&mut sessions);
    sessions.insert(session_id.clone(), session);
    drop(sessions);

    (
        StatusCode::OK,
        Json(json!({
            "status": "waiting",
            "session_id": session_id,
            "login_url": login_url,
            "port": port,
        })),
    )
}

/// `GET /v0/management/zed/login/status` — poll native Zed login status.
async fn get_zed_login_status(
    State(state): State<Arc<ProxyState>>,
    Query(query): Query<ZedLoginStatusQuery>,
) -> impl IntoResponse {
    let Some(session_id) = query
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "session_id is required"})),
        );
    };

    let (private_key, callback_state, session_name) = {
        let mut sessions = state.zed_login_sessions.lock().await;
        cleanup_expired_sessions(&mut sessions);

        let session = match sessions.get_mut(session_id) {
            Some(session) => session,
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({"error": "Zed login session not found"})),
                )
            }
        };

        if !session.callback_state.is_completed() {
            return (
                StatusCode::OK,
                Json(json!({
                    "status": "waiting",
                    "session_id": session_id,
                })),
            );
        }

        (
            session.private_key.clone(),
            Arc::clone(&session.callback_state),
            session.name.clone(),
        )
    };

    let user_id = match callback_state.user_id.lock().await.clone() {
        Some(user_id) => user_id,
        None => {
            let error_msg = "callback completed without user_id".to_string();
            let mut sessions = state.zed_login_sessions.lock().await;
            if let Some(session) = sessions.get_mut(session_id) {
                session.status = ZedLoginSessionStatus::Failed(error_msg.clone());
                session.server_handle.abort();
            }
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": error_msg})),
            );
        }
    };
    let encrypted_access_token = match callback_state.access_token.lock().await.clone() {
        Some(access_token) => access_token,
        None => {
            let error_msg = "callback completed without access_token".to_string();
            let mut sessions = state.zed_login_sessions.lock().await;
            if let Some(session) = sessions.get_mut(session_id) {
                session.status = ZedLoginSessionStatus::Failed(error_msg.clone());
                session.server_handle.abort();
            }
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": error_msg})),
            );
        }
    };

    let credential_json = match decrypt_credential(&private_key, &encrypted_access_token) {
        Ok(value) => value,
        Err(error) => {
            let error_msg = format!("decrypt credential: {error}");
            let mut sessions = state.zed_login_sessions.lock().await;
            if let Some(session) = sessions.get_mut(session_id) {
                session.status = ZedLoginSessionStatus::Failed(error_msg.clone());
                session.server_handle.abort();
            }
            return (StatusCode::BAD_REQUEST, Json(json!({"error": error_msg})));
        }
    };

    let existing_accounts = state.accounts.accounts_for("zed").await;
    let existing_filename = find_existing_zed_filename(&existing_accounts, &user_id);
    let filename = existing_filename
        .as_ref()
        .cloned()
        .unwrap_or_else(|| canonical_zed_login_filename(&user_id));

    let existing_record = if let Some(existing_filename) = existing_filename {
        let stored_records = match state.accounts.store().list().await {
            Ok(records) => records,
            Err(error) => {
                let error_msg = format!("load auth files: {error}");
                let mut sessions = state.zed_login_sessions.lock().await;
                if let Some(session) = sessions.get_mut(session_id) {
                    session.status = ZedLoginSessionStatus::Failed(error_msg.clone());
                    session.server_handle.abort();
                }
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": error_msg})),
                );
            }
        };

        match stored_records.into_iter().find(|record| {
            record.id == existing_filename
                || record.path.file_name().and_then(|f| f.to_str())
                    == Some(existing_filename.as_str())
        }) {
            Some(record) => Some(record),
            None => {
                let error_msg = format!("existing zed auth file not found: {existing_filename}");
                let mut sessions = state.zed_login_sessions.lock().await;
                if let Some(session) = sessions.get_mut(session_id) {
                    session.status = ZedLoginSessionStatus::Failed(error_msg.clone());
                    session.server_handle.abort();
                }
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": error_msg})),
                );
            }
        }
    } else {
        None
    };

    let record = build_zed_login_record(existing_record, &filename, &user_id, &credential_json, &session_name);
    if let Err(error) = state.accounts.store().save(&record).await {
        let error_msg = format!("save auth file: {error}");
        let mut sessions = state.zed_login_sessions.lock().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.status = ZedLoginSessionStatus::Failed(error_msg.clone());
            session.server_handle.abort();
        }
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error_msg})),
        );
    }
    if let Err(error) = refresh_runtime_after_auth_change(&state).await {
        let mut sessions = state.zed_login_sessions.lock().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.status = ZedLoginSessionStatus::Failed(error.to_string());
            session.server_handle.abort();
        }
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string()})),
        );
    }

    {
        let mut sessions = state.zed_login_sessions.lock().await;
        if let Some(session) = sessions.remove(session_id) {
            session.server_handle.abort();
        }
    }

    (
        StatusCode::OK,
        Json(json!({
            "status": "completed",
            "filename": filename,
            "user_id": user_id,
        })),
    )
}

fn find_existing_zed_filename(accounts: &[AuthRecord], user_id: &str) -> Option<String> {
    accounts.iter().find_map(|record| {
        let metadata = serde_json::to_value(&record.metadata).ok()?;
        let (existing_user_id, _) = crate::auth::zed::parse_zed_credential(&metadata).ok()?;
        if zed_user_ids_match(&existing_user_id, user_id) {
            Some(record.id.clone())
        } else {
            None
        }
    })
}

fn build_zed_login_record(
    existing_record: Option<AuthRecord>,
    filename: &str,
    user_id: &str,
    credential_json: &str,
    session_name: &str,
) -> AuthRecord {
    let now = chrono::Utc::now();

    if let Some(mut record) = existing_record {
        record.metadata.insert("type".to_string(), json!("zed"));
        record
            .metadata
            .insert("user_id".to_string(), json!(user_id));
        record
            .metadata
            .insert("credential_json".to_string(), json!(credential_json));
        record
            .metadata
            .insert("last_refreshed_at".to_string(), json!(now.to_rfc3339()));

        if !session_name.is_empty() {
            record.label = session_name.to_string();
            record.metadata.insert("label".to_string(), json!(session_name));
        }

        return record;
    }

    let label = if session_name.is_empty() {
        user_id.to_string()
    } else {
        session_name.to_string()
    };

    let mut metadata = HashMap::from([
        ("type".to_string(), json!("zed")),
        ("user_id".to_string(), json!(user_id)),
        ("credential_json".to_string(), json!(credential_json)),
        ("last_refreshed_at".to_string(), json!(now.to_rfc3339())),
    ]);

    if !session_name.is_empty() {
        metadata.insert("label".to_string(), json!(session_name));
    }

    AuthRecord {
        id: filename.to_string(),
        provider: "zed".to_string(),
        provider_key: "zed".to_string(),
        label,
        disabled: false,
        status: AuthStatus::Active,
        status_message: None,
        last_refreshed_at: Some(now),
        path: std::path::PathBuf::from(filename),
        metadata,
        updated_at: now,
    }
}

/// `POST /v0/management/zed/check-quota` — check quota for a Zed auth file.
async fn check_zed_quota(
    State(state): State<Arc<ProxyState>>,
    Json(body): Json<CheckZedQuotaBody>,
) -> impl IntoResponse {
    let name = match sanitize_filename(body.name.as_deref().unwrap_or("")) {
        Ok(n) => n,
        Err(e) => return e,
    };

    let records = match state.accounts.store().list().await {
        Ok(records) => records,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("failed to list auth files: {error}")})),
            );
        }
    };

    let record = match records
        .into_iter()
        .find(|record| record.id == name && record.provider_key.eq_ignore_ascii_case("zed"))
    {
        Some(record) => record,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Zed auth file not found"})),
            );
        }
    };

    // Parse Zed credential from metadata
    let metadata_value = serde_json::to_value(&record.metadata).unwrap_or(json!({}));
    let (user_id, credential_json) = match crate::auth::zed::parse_zed_credential(&metadata_value) {
        Ok(creds) => creds,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("failed to parse Zed credential: {}", e)})),
            );
        }
    };

    // Call /client/users/me to get quota info
    let response = fetch_zed_user_info(&name, &user_id, &credential_json).await;

    (StatusCode::OK, Json(response))
}

async fn list_zed_models(
    State(state): State<Arc<ProxyState>>,
    Json(body): Json<CheckZedQuotaBody>,
) -> impl IntoResponse {
    let name = match sanitize_filename(body.name.as_deref().unwrap_or("")) {
        Ok(n) => n,
        Err(e) => return e,
    };

    let records = match state.accounts.store().list().await {
        Ok(records) => records,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("failed to list auth files: {error}")})),
            );
        }
    };

    let record = match records
        .into_iter()
        .find(|record| record.id == name && record.provider_key.eq_ignore_ascii_case("zed"))
    {
        Some(record) => record,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Zed auth file not found"})),
            );
        }
    };

    let models = crate::providers::static_models::static_models_by_channel("zed")
        .into_iter()
        .map(|model| model.id)
        .collect::<Vec<_>>();

    (
        StatusCode::OK,
        Json(json!({
            "account": record.id,
            "provider_key": "zed",
            "models": models,
        })),
    )
}

async fn list_github_copilot_models(
    State(state): State<Arc<ProxyState>>,
    Json(body): Json<CheckZedQuotaBody>,
) -> impl IntoResponse {
    let name = match sanitize_filename(body.name.as_deref().unwrap_or("")) {
        Ok(n) => n,
        Err(e) => return e,
    };

    let records = match state.accounts.store().list().await {
        Ok(records) => records,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("failed to list auth files: {error}")})),
            );
        }
    };

    let record = match records
        .into_iter()
        .find(|record| record.id == name && record.provider_key.eq_ignore_ascii_case("github-copilot"))
    {
        Some(record) => record,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "GitHub Copilot auth file not found"})),
            );
        }
    };

    let provider = match crate::providers::github_copilot::GithubCopilotProvider::new(record.clone()) {
        Ok(provider) => provider,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("failed to initialize GitHub Copilot provider: {error}")})),
            );
        }
    };

    let models = match provider
        .live_models()
        .await
        .map(|models| models.into_iter().map(|model| model.id).collect::<Vec<_>>())
    {
        Ok(models) => models,
        Err(error) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": format!("failed to fetch GitHub Copilot models: {error}")})),
            );
        }
    };

    (
        StatusCode::OK,
        Json(json!({
            "account": record.id,
            "provider_key": "github-copilot",
            "models": models,
        })),
    )
}

/// Fetch user info from Zed /client/users/me endpoint and return flattened response.
async fn fetch_zed_user_info(account: &str, user_id: &str, credential_json: &str) -> Value {
    use crate::providers::zed::ZedClient;

    let client = ZedClient;
    let url = client.users_me_endpoint();

    let http_client = reqwest::Client::new();
    let result = http_client
        .get(url)
        .header("authorization", format!("{} {}", user_id, credential_json))
        .header("content-type", "application/json")
        .send()
        .await;

    match result {
        Ok(resp) => {
            let status = resp.status().as_u16();
            match resp.json::<Value>().await {
                Ok(data) => build_zed_quota_response(account, status, Some(data)),
                Err(e) => build_zed_quota_response_error(
                    account,
                    status,
                    format!("failed to parse response: {}", e),
                ),
            }
        }
        Err(e) => build_zed_quota_response_error(account, 0, format!("request failed: {}", e)),
    }
}

/// Build successful quota response with flattened fields.
fn build_zed_quota_response(account: &str, upstream_status: u16, data: Option<Value>) -> Value {
    let data = data.unwrap_or(json!({}));

    // Normalize limit fields
    let model_requests_limit = normalize_zed_limit(data.get("model_requests_limit"));
    let edit_predictions_limit = normalize_zed_limit(data.get("edit_predictions_limit"));

    json!({
        "account": account,
        "status": if upstream_status == 200 { "available" } else { "error" },
        "plan": data.get("plan").and_then(|v| v.as_str()),
        "plan_v2": data.get("plan_v2").and_then(|v| v.as_str()),
        "plan_v3": data.get("plan_v3").and_then(|v| v.as_str()),
        "subscription_started_at": data.get("subscription_started_at").and_then(|v| v.as_str()),
        "subscription_ended_at": data.get("subscription_ended_at").and_then(|v| v.as_str()),
        "model_requests_used": data.get("model_requests_used").and_then(|v| v.as_i64()).unwrap_or(0),
        "model_requests_limit": model_requests_limit,
        "edit_predictions_used": data.get("edit_predictions_used").and_then(|v| v.as_i64()).unwrap_or(0),
        "edit_predictions_limit": edit_predictions_limit,
        "is_account_too_young": data.get("is_account_too_young").and_then(|v| v.as_bool()).unwrap_or(false),
        "has_overdue_invoices": data.get("has_overdue_invoices").and_then(|v| v.as_bool()).unwrap_or(false),
        "is_usage_based_billing_enabled": data.get("is_usage_based_billing_enabled").and_then(|v| v.as_bool()).unwrap_or(false),
        "feature_flags": data.get("feature_flags").and_then(|v| v.as_array()).cloned().unwrap_or_default(),
        "error": null,
        "upstream_status": upstream_status,
    })
}

/// Build error quota response with flattened fields.
fn build_zed_quota_response_error(account: &str, upstream_status: u16, error: String) -> Value {
    json!({
        "account": account,
        "status": "error",
        "plan": null,
        "plan_v2": null,
        "plan_v3": null,
        "subscription_started_at": null,
        "subscription_ended_at": null,
        "model_requests_used": 0,
        "model_requests_limit": 0,
        "edit_predictions_used": 0,
        "edit_predictions_limit": "unlimited",
        "is_account_too_young": false,
        "has_overdue_invoices": false,
        "is_usage_based_billing_enabled": false,
        "feature_flags": [],
        "error": error,
        "upstream_status": upstream_status,
    })
}

/// Normalize Zed limit field: { limited: 42, remaining: 0 } -> 42, or return as-is.
fn normalize_zed_limit(value: Option<&Value>) -> Value {
    match value {
        Some(Value::Object(obj)) if obj.contains_key("limited") => {
            obj.get("limited").cloned().unwrap_or(json!(0))
        }
        Some(v) => v.clone(),
        None => json!(0),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_refreshed_codex_record, build_codex_headers, codex_account_id_from_record,
        refresh_runtime_after_auth_change,
    };
    use crate::auth::manager::AccountManager;
    use crate::auth::store::{AuthRecord, AuthStatus};
    use crate::config::Config;
    use crate::error::AppError;
    use crate::proxy::ProxyState;
    use reqwest::header::HeaderMap;
    use serde_json::json;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn codex_record(dir: &std::path::Path, id: &str) -> AuthRecord {
        let now = chrono::Utc::now();
        let mut metadata = HashMap::new();
        metadata.insert("type".to_string(), json!("codex"));
        metadata.insert("provider_key".to_string(), json!("codex"));
        metadata.insert("access_token".to_string(), json!("old-access-token"));
        metadata.insert("refresh_token".to_string(), json!("old-refresh-token"));
        metadata.insert("id_token".to_string(), json!("old-id-token"));
        metadata.insert("account_id".to_string(), json!("acct_old"));
        metadata.insert("email".to_string(), json!("old@example.com"));
        metadata.insert("expired".to_string(), json!("2020-01-01T00:00:00Z"));
        metadata.insert("last_refresh".to_string(), json!("2020-01-01T00:00:00Z"));
        metadata.insert("base_url".to_string(), json!("http://127.0.0.1:9/codex"));

        AuthRecord {
            id: id.to_string(),
            provider: "codex".to_string(),
            provider_key: "codex".to_string(),
            label: "old@example.com".to_string(),
            disabled: false,
            status: AuthStatus::Active,
            status_message: None,
            last_refreshed_at: Some(now),
            path: dir.join(id),
            metadata,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn apply_refreshed_codex_record_update_failure_stays_typed() {
        let dir = TempDir::new().expect("create temp dir");
        let accounts = Arc::new(AccountManager::with_dir(dir.path()));
        let record = codex_record(dir.path(), "codex-refresh.json");
        accounts
            .store()
            .save(&record)
            .await
            .expect("save initial codex record");
        accounts.reload().await.expect("reload accounts");

        let deleted_path = PathBuf::from(dir.path()).join("codex-refresh.json");
        std::fs::remove_file(&deleted_path).expect("remove saved auth file");
        std::fs::create_dir(&deleted_path).expect("replace auth file with directory");

        let state = Arc::new(ProxyState::new(
            Config {
                auth_dir: dir.path().to_string_lossy().to_string(),
                ..Default::default()
            },
            accounts,
            Arc::new(crate::providers::model_registry::ModelRegistry::new()),
            0,
        ));

        let refreshed = crate::auth::codex::CodexTokenData {
            id_token: "new-id-token".to_string(),
            access_token: "new-access-token".to_string(),
            refresh_token: "new-refresh-token".to_string(),
            account_id: "acct_new".to_string(),
            email: "new@example.com".to_string(),
            expired: "2035-01-01T00:00:00Z".to_string(),
        };

        let error = apply_refreshed_codex_record(&state, "codex-refresh.json", &refreshed)
            .await
            .expect_err("directory-backed auth path should fail to persist");

        match error {
            AppError::Config(message) => {
                assert!(
                    message.contains("write auth file"),
                    "unexpected config error: {message}"
                );
            }
            other => panic!("expected AppError::Config, got {other}"),
        }
    }

    #[tokio::test]
    async fn apply_refreshed_codex_record_updates_memory_and_disk() {
        let dir = TempDir::new().expect("create temp dir");
        let accounts = Arc::new(AccountManager::with_dir(dir.path()));
        let record = codex_record(dir.path(), "codex-refresh.json");
        accounts
            .store()
            .save(&record)
            .await
            .expect("save initial codex record");
        accounts.reload().await.expect("reload accounts");

        let state = Arc::new(ProxyState::new(
            Config {
                auth_dir: dir.path().to_string_lossy().to_string(),
                ..Default::default()
            },
            accounts.clone(),
            Arc::new(crate::providers::model_registry::ModelRegistry::new()),
            0,
        ));

        let refreshed = crate::auth::codex::CodexTokenData {
            id_token: "new-id-token".to_string(),
            access_token: "new-access-token".to_string(),
            refresh_token: "new-refresh-token".to_string(),
            account_id: "acct_new".to_string(),
            email: "new@example.com".to_string(),
            expired: "2035-01-01T00:00:00Z".to_string(),
        };

        apply_refreshed_codex_record(&state, "codex-refresh.json", &refreshed)
            .await
            .expect("apply refreshed codex record");

        let updated = state
            .accounts
            .get_by_id("codex-refresh.json")
            .await
            .expect("updated record in memory");
        assert_eq!(
            updated.metadata.get("access_token"),
            Some(&json!("new-access-token"))
        );
        assert_eq!(
            updated.metadata.get("refresh_token"),
            Some(&json!("new-refresh-token"))
        );
        assert_eq!(
            updated.metadata.get("id_token"),
            Some(&json!("new-id-token"))
        );
        assert_eq!(updated.metadata.get("account_id"), Some(&json!("acct_new")));
        assert_eq!(
            updated.metadata.get("email"),
            Some(&json!("new@example.com"))
        );
        assert_eq!(
            updated.metadata.get("expired"),
            Some(&json!("2035-01-01T00:00:00Z"))
        );
        assert_eq!(updated.provider, "codex");
        assert_eq!(updated.provider_key, "codex");
        assert!(updated.metadata.contains_key("last_refresh"));
        assert!(updated.metadata.contains_key("last_refreshed_at"));

        let saved: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(PathBuf::from(dir.path()).join("codex-refresh.json"))
                .expect("read saved auth file"),
        )
        .expect("parse saved auth file");
        assert_eq!(saved["access_token"], "new-access-token");
        assert_eq!(saved["refresh_token"], "new-refresh-token");
        assert_eq!(saved["id_token"], "new-id-token");
        assert_eq!(saved["account_id"], "acct_new");
        assert_eq!(saved["email"], "new@example.com");
        assert_eq!(saved["expired"], "2035-01-01T00:00:00Z");
        assert!(saved.get("last_refresh").is_some());
        assert!(saved.get("last_refreshed_at").is_some());
    }

    #[tokio::test]
    async fn refresh_runtime_after_auth_change_preserves_typed_reload_errors() {
        let dir = TempDir::new().expect("create temp dir");
        let auth_path = dir.path().join("not-a-directory.json");
        std::fs::write(&auth_path, "{}").expect("write auth file placeholder");

        let accounts = Arc::new(AccountManager::with_dir(&auth_path));
        let state = Arc::new(ProxyState::new(
            Config {
                auth_dir: auth_path.to_string_lossy().to_string(),
                ..Default::default()
            },
            accounts,
            Arc::new(crate::providers::model_registry::ModelRegistry::new()),
            0,
        ));

        let error = refresh_runtime_after_auth_change(&state)
            .await
            .expect_err("file auth path should fail reload");

        assert!(matches!(error, AppError::Config(message) if message.contains("read auth dir")));
    }

    #[test]
    fn codex_account_id_is_recomputed_after_record_refresh_for_headers() {
        let dir = TempDir::new().expect("create temp dir");
        let mut record = codex_record(dir.path(), "codex-refresh.json");
        let stale_account = codex_account_id_from_record(&record);
        assert_eq!(stale_account, "acct_old");

        record
            .metadata
            .insert("account_id".to_string(), json!("acct_new"));

        let headers_with_stale_account =
            build_codex_headers("access-token", &stale_account, "application/json")
                .expect("stale headers should build");
        assert_eq!(
            header_account_id(&headers_with_stale_account),
            Some("acct_old")
        );

        let account = codex_account_id_from_record(&record);
        let headers = build_codex_headers("access-token", &account, "application/json")
            .expect("headers should build");

        assert_eq!(header_account_id(&headers), Some("acct_new"));
    }

    fn header_account_id(headers: &HeaderMap) -> Option<&str> {
        headers
            .get("chatgpt-account-id")
            .and_then(|value| value.to_str().ok())
    }
}
