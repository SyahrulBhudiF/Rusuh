use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::Json;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{info, warn};

use crate::auth::kiro::{KiroTokenData, KiroTokenSource, BUILDER_ID_START_URL, DEFAULT_REGION};
use crate::auth::kiro_record::KiroRecordInput;
use crate::auth::kiro_login::SSOOIDCClient;
use crate::proxy::oauth::OAuthSessionStatus;
use crate::proxy::ProxyState;

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
