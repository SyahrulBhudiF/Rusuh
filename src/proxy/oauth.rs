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
