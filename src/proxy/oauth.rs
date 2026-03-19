//! Web OAuth trigger — start OAuth flows via HTTP endpoints, track sessions in memory.
//!
//! Unlike the CLI flow (which blocks on a local callback server), the web flow:
//! 1. Returns an auth URL immediately for the caller to open
//! 2. Receives the callback on the main server at `/antigravity/callback`
//! 3. Exchanges code → tokens → saves credentials → reloads accounts
//! 4. Caller polls `/v0/management/oauth/status?state=...` for completion

use std::collections::HashMap;
use std::sync::Arc;

use crate::auth::antigravity::*;
use crate::auth::kiro::{KiroTokenData, KiroTokenSource, BUILDER_ID_START_URL, DEFAULT_REGION};
use crate::auth::kiro_login::SSOOIDCClient;
use crate::auth::kiro_record::KiroRecordInput;
use crate::auth::store::{AuthRecord, AuthStatus};
use crate::proxy::ProxyState;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::RwLock;
use tracing::{info, warn};

// ── Session tracker ──────────────────────────────────────────────────────────

/// Status of an in-flight OAuth session.
#[derive(Debug, Clone)]
pub enum OAuthSessionStatus {
    Pending,
    Complete,
    Error(String),
}

/// Tracks in-flight OAuth sessions keyed by state parameter.
#[derive(Debug, Clone)]
struct OAuthSession {
    provider: String,
    status: OAuthSessionStatus,
    created_at: DateTime<Utc>,
    code_verifier: Option<String>,
    context: HashMap<String, Value>,
}

/// Thread-safe session store.
pub struct OAuthSessionStore {
    sessions: RwLock<HashMap<String, OAuthSession>>,
}

impl Default for OAuthSessionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl OAuthSessionStore {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    pub async fn register(&self, state: &str, provider: &str) {
        self.register_with_context(state, provider, None, HashMap::new())
            .await;
    }
    pub async fn register_with_code_verifier(
        &self,
        state: &str,
        provider: &str,
        code_verifier: Option<String>,
    ) {
        self.register_with_context(state, provider, code_verifier, HashMap::new())
            .await;
    }

    pub async fn register_with_context(
        &self,
        state: &str,
        provider: &str,
        code_verifier: Option<String>,
        context: HashMap<String, Value>,
    ) {
        self.sessions.write().await.insert(
            state.to_string(),
            OAuthSession {
                provider: provider.to_string(),
                status: OAuthSessionStatus::Pending,
                created_at: Utc::now(),
                code_verifier,
                context,
            },
        );
    }

    pub async fn complete(&self, state: &str) {
        if let Some(session) = self.sessions.write().await.get_mut(state) {
            session.status = OAuthSessionStatus::Complete;
        }
    }

    pub async fn set_error(&self, state: &str, msg: &str) {
        if let Some(session) = self.sessions.write().await.get_mut(state) {
            session.status = OAuthSessionStatus::Error(msg.to_string());
        }
    }

    pub async fn get_status(&self, state: &str) -> Option<(String, OAuthSessionStatus)> {
        self.sessions
            .read()
            .await
            .get(state)
            .map(|s| (s.provider.clone(), s.status.clone()))
    }

    pub async fn get_code_verifier(&self, state: &str) -> Option<String> {
        self.sessions
            .read()
            .await
            .get(state)
            .and_then(|s| s.code_verifier.clone())
    }

    pub async fn get_context(&self, state: &str) -> Option<HashMap<String, Value>> {
        self.sessions
            .read()
            .await
            .get(state)
            .map(|s| s.context.clone())
    }

    /// Clean up sessions older than `max_age` seconds.
    pub async fn cleanup(&self, max_age_secs: i64) {
        let cutoff = Utc::now() - chrono::Duration::seconds(max_age_secs);
        self.sessions
            .write()
            .await
            .retain(|_, s| s.created_at > cutoff);
    }
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// `GET /v0/management/oauth/start?provider=antigravity` — generate OAuth URL,
/// return `{status, provider, url, state}`.
///
/// The caller should open the URL in a browser. After auth, Google redirects to
pub async fn antigravity_auth_url(State(state): State<Arc<ProxyState>>) -> impl IntoResponse {
    let cfg = state.config.read().await;
    let port = cfg.port;
    drop(cfg);

    let oauth_state = uuid::Uuid::new_v4().to_string();
    let redirect_uri = format!("http://localhost:{port}/antigravity/callback");

    let scopes = SCOPES.join(" ");
    let params = [
        ("access_type", "offline"),
        ("client_id", CLIENT_ID),
        ("prompt", "consent"),
        ("redirect_uri", redirect_uri.as_str()),
        ("response_type", "code"),
        ("scope", &scopes),
        ("state", &oauth_state),
    ];
    let query = serde_urlencoded::to_string(params).expect("encode params");
    let auth_url = format!("{AUTH_ENDPOINT}?{query}");

    // Register session
    state
        .oauth_sessions
        .register(&oauth_state, "antigravity")
        .await;

    // Cleanup old sessions (>10 min)
    state.oauth_sessions.cleanup(600).await;

    info!(
        provider = "antigravity",
        state = %oauth_state,
        "OAuth flow initiated via management API"
    );

    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "url": auth_url,
            "state": oauth_state,
        })),
    )
}

/// `GET /antigravity/callback?code=...&state=...` — OAuth callback handler.
///
/// This is a top-level route (not under /v0/management/) because Google
/// redirects the browser here directly.
#[derive(Deserialize)]
pub struct CallbackQuery {
    code: String,
    state: String,
}

pub async fn antigravity_callback(
    State(state): State<Arc<ProxyState>>,
    Query(q): Query<CallbackQuery>,
) -> impl IntoResponse {
    let oauth_state = q.state.trim().to_string();
    let code = q.code.trim().to_string();

    // Verify session exists and is pending
    let session_status = state.oauth_sessions.get_status(&oauth_state).await;
    match &session_status {
        Some((_, OAuthSessionStatus::Pending)) => {}
        Some((_, OAuthSessionStatus::Complete)) => {
            return Html(
                "<h1>Already completed</h1><p>This OAuth session has already been processed.</p>"
                    .to_string(),
            );
        }
        Some((_, OAuthSessionStatus::Error(msg))) => {
            return Html(format!("<h1>Error</h1><p>Session error: {msg}</p>"));
        }
        None => {
            return Html("<h1>Error</h1><p>Unknown or expired OAuth session.</p>".to_string());
        }
    }

    // Exchange code for tokens in background
    let state_clone = state.clone();
    let oauth_state_clone = oauth_state.clone();
    tokio::spawn(async move {
        if let Err(e) = process_antigravity_callback(&state_clone, &code, &oauth_state_clone).await
        {
            warn!(
                provider = "antigravity",
                state = %oauth_state_clone,
                "OAuth callback processing failed: {e}"
            );
            state_clone
                .oauth_sessions
                .set_error(&oauth_state_clone, &e.to_string())
                .await;
        }
    });

    Html(
        "<h1>✓ Authenticating...</h1>\
         <p>Processing your credentials. You can close this tab.</p>\
         <p>Check status via <code>GET /v0/management/oauth/status?state=...</code></p>"
            .to_string(),
    )
}

/// Process the OAuth callback — exchange code, fetch user info, save credentials.
async fn process_antigravity_callback(
    state: &Arc<ProxyState>,
    code: &str,
    oauth_state: &str,
) -> anyhow::Result<()> {
    let cfg = state.config.read().await;
    let port = cfg.port;
    drop(cfg);

    let redirect_uri = format!("http://localhost:{port}/antigravity/callback");
    let client = reqwest::Client::new();

    // Exchange code for tokens
    let params = [
        ("code", code),
        ("client_id", CLIENT_ID),
        ("client_secret", CLIENT_SECRET),
        ("redirect_uri", redirect_uri.as_str()),
        ("grant_type", "authorization_code"),
    ];

    let resp = client.post(TOKEN_ENDPOINT).form(&params).send().await?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("token exchange failed ({status}): {body}");
    }

    let token_resp: crate::auth::antigravity_login::TokenResponse = resp.json().await?;
    let access_token = token_resp.access_token.trim().to_string();
    if access_token.is_empty() {
        anyhow::bail!("token exchange returned empty access_token");
    }

    // Fetch user email
    let email_resp = client
        .get(USERINFO_ENDPOINT)
        .bearer_auth(&access_token)
        .send()
        .await?;

    let email_data: Value = email_resp.json().await?;
    let email = email_data["email"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();

    if email.is_empty() {
        anyhow::bail!("could not fetch user email");
    }

    info!(provider = "antigravity", email = %email, "OAuth user authenticated");

    // Fetch project_id (best-effort)
    let project_id =
        match crate::auth::antigravity_login::fetch_project_id(&client, &access_token).await {
            Ok(pid) => {
                info!(provider = "antigravity", project_id = %pid, "project_id fetched");
                Some(pid)
            }
            Err(e) => {
                warn!(provider = "antigravity", "could not fetch project_id: {e}");
                None
            }
        };

    // Build metadata
    let now = Utc::now();
    let expires_in = token_resp.expires_in.unwrap_or(3599);
    let mut metadata: HashMap<String, Value> = HashMap::new();
    metadata.insert("type".into(), json!("antigravity"));
    metadata.insert("email".into(), json!(email));
    metadata.insert("access_token".into(), json!(access_token));
    if let Some(ref rt) = token_resp.refresh_token {
        metadata.insert("refresh_token".into(), json!(rt));
    }
    metadata.insert("expires_in".into(), json!(expires_in));
    metadata.insert("timestamp".into(), json!(now.timestamp_millis()));
    metadata.insert(
        "expired".into(),
        json!((now + chrono::Duration::seconds(expires_in)).to_rfc3339()),
    );
    if let Some(ref pid) = project_id {
        metadata.insert("project_id".into(), json!(pid));
    }
    metadata.insert("disabled".into(), json!(false));

    // Save credential
    let filename = if email.is_empty() {
        "antigravity.json".to_string()
    } else {
        format!("antigravity-{email}.json")
    };

    let record = AuthRecord {
        id: filename.clone(),
        provider: "antigravity".into(),
        provider_key: "antigravity".into(),
        label: email.clone(),
        disabled: false,
        status: AuthStatus::Active,
        status_message: None,
        last_refreshed_at: Some(now),
        path: std::path::PathBuf::from(&filename),
        metadata,
        updated_at: now,
    };

    state
        .accounts
        .store()
        .save(&record)
        .await
        .map_err(|e| anyhow::anyhow!("save credential: {e}"))?;

    info!(
        provider = "antigravity",
        email = %email,
        file = %filename,
        "credentials saved via web OAuth"
    );

    // Reload accounts
    if let Err(e) = state.accounts.reload().await {
        warn!("failed to reload accounts after OAuth: {e}");
    }

    // Mark session complete
    state.oauth_sessions.complete(oauth_state).await;

    Ok(())
}

/// `GET /v0/management/oauth/status?state=...` — poll OAuth session status.
#[derive(Deserialize)]
pub struct AuthStatusQuery {
    state: Option<String>,
}

pub async fn get_auth_status(
    State(state): State<Arc<ProxyState>>,
    Query(q): Query<AuthStatusQuery>,
) -> Json<Value> {
    let Some(oauth_state) = q.state.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        return Json(json!({"status": "ok"}));
    };

    match state.oauth_sessions.get_status(oauth_state).await {
        Some((provider, OAuthSessionStatus::Pending)) => {
            Json(json!({"status": "wait", "provider": provider}))
        }
        Some((provider, OAuthSessionStatus::Complete)) => {
            Json(json!({"status": "ok", "provider": provider}))
        }
        Some((provider, OAuthSessionStatus::Error(msg))) => {
            Json(json!({"status": "error", "provider": provider, "error": msg}))
        }
        None => Json(json!({"status": "ok"})),
    }
}

// ── Universal OAuth Handler ─────────────────────────────────────────────────

/// `GET /v0/management/oauth/start?provider=antigravity|kiro-google|kiro-github`.
///
/// Kiro social providers are intentionally unsupported. Supported Kiro web
/// management flows live under provider-specific `/v0/management/kiro/*` routes.
#[derive(Deserialize)]
pub struct StartOAuthQuery {
    provider: String,
    label: Option<String>,
}

pub async fn start_oauth(
    State(state): State<Arc<ProxyState>>,
    Query(q): Query<StartOAuthQuery>,
) -> impl IntoResponse {
    let provider = q.provider.trim().to_lowercase();
    match provider.as_str() {
        "antigravity" => start_antigravity_oauth(state, q.label).await,
        "kiro-google" | "kiro-github" => reject_legacy_kiro_social(),
        _ => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "status": "error",
                "error": format!(
                    "unsupported provider '{}' — supported: antigravity; Kiro uses provider-specific /v0/management/kiro/* endpoints",
                    provider
                )
            })),
        ).into_response(),
    }
}

// ── Antigravity OAuth (existing logic) ──────────────────────────────────────

async fn start_antigravity_oauth(state: Arc<ProxyState>, _label: Option<String>) -> Response {
    let cfg = state.config.read().await;
    let port = cfg.port;
    drop(cfg);

    let oauth_state = uuid::Uuid::new_v4().to_string();
    let redirect_uri = format!("http://localhost:{port}/antigravity/callback");

    let scopes = SCOPES.join(" ");
    let params = [
        ("access_type", "offline"),
        ("client_id", CLIENT_ID),
        ("prompt", "consent"),
        ("redirect_uri", redirect_uri.as_str()),
        ("response_type", "code"),
        ("scope", &scopes),
        ("state", &oauth_state),
    ];
    let query = serde_urlencoded::to_string(params).expect("encode params");
    let auth_url = format!("{AUTH_ENDPOINT}?{query}");

    state
        .oauth_sessions
        .register(&oauth_state, "antigravity")
        .await;

    state.oauth_sessions.cleanup(600).await;

    info!(
        provider = "antigravity",
        state = %oauth_state,
        "OAuth flow initiated via management API"
    );

    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "url": auth_url,
            "state": oauth_state,
            "provider": "antigravity"
        })),
    )
        .into_response()
}

// ── Legacy KIRO social rejection ────────────────────────────────────────────

fn reject_legacy_kiro_social() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "status": "error",
            "error": "Kiro social login is unsupported. Use /v0/management/kiro/builder-id/start, the IDC flow, or token import instead."
        })),
    )
        .into_response()
}

#[derive(Deserialize)]
pub struct KiroCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}
pub async fn kiro_callback(
    State(_state): State<Arc<ProxyState>>,
    Query(q): Query<KiroCallbackQuery>,
) -> impl IntoResponse {
    let _ = (&q.code, &q.state, &q.error, &q.error_description);
    Html(
        "<h1>Unsupported</h1><p>Legacy Kiro social OAuth callback is disabled. Use the provider-specific Kiro management flows instead.</p>"
            .to_string(),
    )
}

// ── Kiro Builder ID OAuth (provider-specific web flow) ──────────────────────

const KIRO_SESSION_TTL_SECS: i64 = 600;
pub const BUILDER_ID_CALLBACK_PATH: &str = "/kiro/builder-id/callback";

#[derive(Deserialize)]
pub struct BuilderIdStartBody {
    pub label: Option<String>,
}

#[derive(Clone)]
pub struct BuilderIdStartResponse {
    pub session_id: String,
    pub auth_url: String,
    pub expires_at: String,
}

impl BuilderIdStartResponse {
    fn into_json(self) -> Value {
        json!({
            "session_id": self.session_id,
            "auth_url": self.auth_url,
            "expires_at": self.expires_at,
            "auth_method": "builder-id",
            "provider_key": "kiro",
        })
    }
}

pub async fn start_builder_id_login(
    State(state): State<Arc<ProxyState>>,
    Json(body): Json<BuilderIdStartBody>,
) -> impl IntoResponse {
    match build_builder_id_start_response(state, body.label).await {
        Ok(response) => (StatusCode::OK, Json(response.into_json())).into_response(),
        Err(message) => (StatusCode::BAD_GATEWAY, Json(json!({"error": message}))).into_response(),
    }
}

pub async fn builder_id_callback(
    State(state): State<Arc<ProxyState>>,
    Query(query): Query<BuilderIdCallbackQuery>,
) -> impl IntoResponse {
    let Some(session_id) = query
        .state
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Html("<h1>Error</h1><p>Missing state parameter.</p>".to_string());
    };

    if let Some(error) = query
        .error
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        state.oauth_sessions.set_error(session_id, error).await;
        return Html(format!("<h1>Authentication failed</h1><p>{error}</p>"));
    }

    let Some(code) = query
        .code
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        state
            .oauth_sessions
            .set_error(session_id, "missing authorization code in callback")
            .await;
        return Html(
            "<h1>Authentication failed</h1><p>Missing authorization code.</p>".to_string(),
        );
    };

    match state.oauth_sessions.get_status(session_id).await {
        Some((provider, OAuthSessionStatus::Pending)) if provider == "kiro" => {}
        Some((_, OAuthSessionStatus::Complete)) => {
            return Html(
                "<h1>Already completed</h1><p>This OAuth session has already been processed.</p>"
                    .to_string(),
            )
        }
        Some((_, OAuthSessionStatus::Error(message))) => {
            return Html(format!("<h1>Error</h1><p>Session error: {message}</p>"))
        }
        _ => return Html("<h1>Error</h1><p>Unknown or expired OAuth session.</p>".to_string()),
    }

    let state_clone = state.clone();
    let session_id = session_id.to_string();
    let code = code.to_string();
    tokio::spawn(async move {
        if let Err(error) = process_builder_id_callback(&state_clone, &session_id, &code).await {
            log_builder_id_callback_error(&error);
            state_clone
                .oauth_sessions
                .set_error(&session_id, &error.to_string())
                .await;
        }
    });

    Html(
        "<h1>✓ Authenticating...</h1><p>Processing your Kiro credentials. You can close this tab.</p><p>Check status via <code>GET /v0/management/oauth/status?state=...</code></p>"
            .to_string(),
    )
}

#[derive(Deserialize)]
pub struct BuilderIdCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

pub async fn build_builder_id_start_response(
    state: Arc<ProxyState>,
    label: Option<String>,
) -> Result<BuilderIdStartResponse, String> {
    let cfg = state.config.read().await;
    let redirect_uri = builder_id_redirect_uri(cfg.port);
    drop(cfg);

    let sso_client = SSOOIDCClient::new();
    let code_verifier = crate::auth::kiro_login::generate_code_verifier();
    let code_challenge = crate::auth::kiro_login::generate_code_challenge(&code_verifier);
    let session_id = uuid::Uuid::new_v4().to_string();

    let registration = sso_client
        .register_client_for_auth_code(&redirect_uri, BUILDER_ID_START_URL, DEFAULT_REGION)
        .await
        .map_err(|error| format!("register client failed: {error}"))?;

    let auth_url = sso_client.build_builder_id_authorization_url(
        &registration.client_id,
        &redirect_uri,
        &session_id,
        &code_challenge,
    );

    let context = build_builder_id_session_context(&registration, &redirect_uri, label);

    state
        .oauth_sessions
        .register_with_context(&session_id, "kiro", Some(code_verifier), context)
        .await;
    state.oauth_sessions.cleanup(KIRO_SESSION_TTL_SECS).await;

    Ok(BuilderIdStartResponse {
        session_id,
        auth_url,
        expires_at: (Utc::now() + chrono::Duration::seconds(KIRO_SESSION_TTL_SECS)).to_rfc3339(),
    })
}

pub fn builder_id_redirect_uri(port: u16) -> String {
    format!("http://localhost:{port}{BUILDER_ID_CALLBACK_PATH}")
}

pub fn build_builder_id_session_context(
    registration: &crate::auth::kiro_login::RegisterClientResponse,
    redirect_uri: &str,
    label: Option<String>,
) -> HashMap<String, Value> {
    let mut context = HashMap::from([
        ("client_id".to_string(), json!(registration.client_id)),
        (
            "client_secret".to_string(),
            json!(registration.client_secret),
        ),
        ("redirect_uri".to_string(), json!(redirect_uri)),
        ("auth_method".to_string(), json!("builder-id")),
        ("provider".to_string(), json!("AWS")),
        ("region".to_string(), json!(DEFAULT_REGION)),
        ("start_url".to_string(), json!(BUILDER_ID_START_URL)),
    ]);
    if let Some(label) = label.and_then(|value| {
        let trimmed = value.trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    }) {
        context.insert("label".to_string(), json!(label));
    }
    context
}

pub fn build_builder_id_auth_record(
    context: &HashMap<String, Value>,
    token_resp: crate::auth::kiro_login::CreateTokenResponse,
    email: Option<String>,
) -> anyhow::Result<crate::auth::store::AuthRecord> {
    let client_id = required_context_string(context, "client_id")?;
    let client_secret = required_context_string(context, "client_secret")?;
    let region = required_context_string(context, "region")?;
    let start_url = required_context_string(context, "start_url")?;
    let provider = required_context_string(context, "provider")?;
    let label = context
        .get("label")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    if token_resp.access_token.trim().is_empty() {
        anyhow::bail!("Builder ID token exchange returned empty access token");
    }

    let expires_at =
        (Utc::now() + chrono::Duration::seconds(i64::from(token_resp.expires_in))).to_rfc3339();

    Ok(KiroRecordInput {
        token_data: KiroTokenData {
            access_token: token_resp.access_token,
            refresh_token: token_resp.refresh_token.unwrap_or_default(),
            profile_arn: String::new(),
            expires_at,
            auth_method: "builder-id".to_string(),
            provider,
            client_id: Some(client_id),
            client_secret: Some(client_secret),
            region,
            start_url: Some(start_url),
            email,
        },
        label,
        source: KiroTokenSource::BuilderIdWeb,
    }
    .into_auth_record())
}

async fn process_builder_id_callback(
    state: &Arc<ProxyState>,
    session_id: &str,
    code: &str,
) -> anyhow::Result<()> {
    let code_verifier = state
        .oauth_sessions
        .get_code_verifier(session_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("missing PKCE verifier for Builder ID session"))?;
    let context = state
        .oauth_sessions
        .get_context(session_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("missing Builder ID session context"))?;

    let client_id = required_context_string(&context, "client_id")?;
    let client_secret = required_context_string(&context, "client_secret")?;
    let redirect_uri = required_context_string(&context, "redirect_uri")?;
    let sso_client = SSOOIDCClient::new();
    let token_resp = sso_client
        .create_token_with_auth_code(
            &client_id,
            &client_secret,
            code,
            &code_verifier,
            &redirect_uri,
        )
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let email = sso_client
        .fetch_builder_id_email(&token_resp.access_token)
        .await;
    let record = build_builder_id_auth_record(&context, token_resp, email)?;

    let saved_path = state
        .accounts
        .store()
        .save(&record)
        .await
        .map_err(|error| anyhow::anyhow!("save credential: {error}"))?;

    info!(
        provider = "kiro",
        auth_method = "builder-id",
        file = %saved_path.display(),
        session_id,
        "Builder ID credentials saved via management OAuth"
    );

    state
        .accounts
        .reload()
        .await
        .map_err(|error| anyhow::anyhow!("reload accounts: {error}"))?;

    state.oauth_sessions.complete(session_id).await;
    Ok(())
}

fn required_context_string(context: &HashMap<String, Value>, key: &str) -> anyhow::Result<String> {
    context
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow::anyhow!("missing session context field: {key}"))
}

pub fn log_builder_id_callback_error(error: &dyn std::fmt::Display) {
    warn!(
        provider = "kiro",
        "Builder ID callback processing failed: {error}"
    );
}
