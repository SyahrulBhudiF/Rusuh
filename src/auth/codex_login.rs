//! Codex login helpers: PKCE, manual callback parsing, OAuth exchange, and CLI login flows.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration as StdDuration;

use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use chrono::{Duration, Utc};
use rand::RngExt;
use reqwest::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::auth::codex::{
    save_auth_bundle, CodexAuthBundle, CodexTokenData, CLIENT_ID, REDIRECT_URI, TOKEN_URL,
};
use crate::auth::store::FileTokenStore;
use crate::error::{AppError, AppResult};

/// OAuth authorization endpoint.
pub const AUTH_URL: &str = "https://auth.openai.com/oauth/authorize";

/// PKCE verifier/challenge pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PKCECodes {
    /// High-entropy verifier sent to the token endpoint.
    pub code_verifier: String,
    /// SHA256-derived challenge sent in the authorization request.
    pub code_challenge: String,
}

/// Parsed callback values from a pasted redirect URL.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ManualCallbackResult {
    /// Authorization code.
    pub code: Option<String>,
    /// OAuth state.
    pub state: Option<String>,
    /// OAuth error code.
    pub error: Option<String>,
    /// OAuth error description.
    pub error_description: Option<String>,
    /// Optional setup_required flag from callback success flow.
    pub setup_required: Option<String>,
    /// Optional platform URL from callback success flow.
    pub platform_url: Option<String>,
}

/// Minimal OAuth server lifecycle holder.
#[derive(Debug, Clone)]
pub struct OAuthServer {
    running: Arc<AtomicBool>,
    /// Configured callback port.
    pub port: u16,
}

impl OAuthServer {
    /// Create a new OAuth server helper.
    pub fn new(port: u16) -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            port,
        }
    }

    /// Mark the helper as running.
    pub fn start(&self) -> AppResult<()> {
        self.running.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Mark the helper as stopped.
    pub fn stop(&self) -> AppResult<()> {
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// Whether the helper is currently marked running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Wait for callback until timeout.
    pub async fn wait_for_callback(&self, timeout: StdDuration) -> AppResult<ManualCallbackResult> {
        if !self.is_running() {
            return Err(AppError::Auth("oauth server is not running".into()));
        }

        tokio::time::sleep(timeout).await;
        Err(AppError::Auth("oauth callback timeout".into()))
    }
}

#[derive(Debug, Clone, Deserialize)]
struct CodexTokenEndpointResponse {
    access_token: String,
    refresh_token: String,
    id_token: String,
    expires_in: i64,
}

/// Claims under `https://api.openai.com/auth`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CodexAuthInfo {
    /// ChatGPT account ID for the authenticated user.
    #[serde(default)]
    pub chatgpt_account_id: String,
    /// ChatGPT plan type (e.g. plus/team).
    #[serde(default)]
    pub chatgpt_plan_type: String,
}

/// Parsed JWT payload claims used by Codex auth logic.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct JWTClaims {
    /// User email claim.
    #[serde(default)]
    pub email: String,
    /// Provider-specific auth info block.
    #[serde(default, rename = "https://api.openai.com/auth")]
    pub codex_auth_info: CodexAuthInfo,
}

/// Generate a PKCE verifier/challenge pair.
pub fn generate_pkce_codes() -> PKCECodes {
    let mut random_bytes = [0u8; 64];
    rand::rng().fill(&mut random_bytes);

    let code_verifier = URL_SAFE_NO_PAD.encode(random_bytes);
    let code_challenge = derive_code_challenge(&code_verifier);

    PKCECodes {
        code_verifier,
        code_challenge,
    }
}

/// Derive the PKCE `S256` code challenge from a verifier.
pub fn derive_code_challenge(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();

    URL_SAFE_NO_PAD.encode(hash)
}

/// Build OAuth authorization URL for Codex.
pub fn generate_auth_url(
    state: &str,
    pkce_codes: &PKCECodes,
    redirect_uri: &str,
) -> AppResult<String> {
    if state.trim().is_empty() {
        return Err(AppError::BadRequest("state must not be empty".into()));
    }
    if redirect_uri.trim().is_empty() {
        return Err(AppError::BadRequest(
            "redirect_uri must not be empty".into(),
        ));
    }

    let params = [
        ("client_id", CLIENT_ID),
        ("response_type", "code"),
        ("redirect_uri", redirect_uri),
        ("scope", "openid email profile offline_access"),
        ("state", state),
        ("code_challenge", pkce_codes.code_challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("prompt", "login"),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
    ];

    let query = serde_urlencoded::to_string(params)
        .map_err(|error| AppError::Config(format!("failed to build auth query: {error}")))?;

    Ok(format!("{AUTH_URL}?{query}"))
}

/// Parse manual callback URL query values.
pub fn parse_manual_callback_url(input: &str) -> AppResult<ManualCallbackResult> {
    let url = url::Url::parse(input.trim())
        .map_err(|error| AppError::BadRequest(format!("invalid callback url: {error}")))?;

    let mut result = ManualCallbackResult::default();
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => result.code = non_empty(value.as_ref()),
            "state" => result.state = non_empty(value.as_ref()),
            "error" => result.error = non_empty(value.as_ref()),
            "error_description" => result.error_description = non_empty(value.as_ref()),
            "setup_required" => result.setup_required = non_empty(value.as_ref()),
            "platform_url" => result.platform_url = non_empty(value.as_ref()),
            _ => {}
        }
    }

    Ok(result)
}

/// Validate callback state value.
pub fn validate_callback_state(expected: &str, actual: &str) -> AppResult<()> {
    if expected.trim().is_empty() || actual.trim().is_empty() {
        return Err(AppError::Auth("state must not be empty".into()));
    }
    if expected != actual {
        return Err(AppError::Auth("state mismatch".into()));
    }
    Ok(())
}

/// Whether a platform/setup URL is safe to render in a success page.
pub fn is_safe_platform_url(url: &str) -> bool {
    match url::Url::parse(url.trim()) {
        Ok(parsed) => matches!(parsed.scheme(), "http" | "https"),
        Err(_) => false,
    }
}

/// Parse JWT payload claims without signature verification.
pub fn parse_jwt_token(token: &str) -> AppResult<JWTClaims> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(AppError::Auth(format!(
            "invalid JWT token format: expected 3 parts, got {}",
            parts.len()
        )));
    }

    let payload = decode_base64url(parts[1])?;
    serde_json::from_slice::<JWTClaims>(&payload)
        .map_err(|error| AppError::Auth(format!("failed to parse JWT claims: {error}")))
}

/// Get normalized user email from parsed claims.
pub fn get_user_email(claims: &JWTClaims) -> Option<&str> {
    let email = claims.email.trim();
    if email.is_empty() {
        None
    } else {
        Some(email)
    }
}

/// Get normalized account ID from parsed claims.
pub fn get_account_id(claims: &JWTClaims) -> Option<&str> {
    let account_id = claims.codex_auth_info.chatgpt_account_id.trim();
    if account_id.is_empty() {
        None
    } else {
        Some(account_id)
    }
}

/// Get normalized plan type from parsed claims.
pub fn get_plan_type(claims: &JWTClaims) -> Option<&str> {
    let plan = claims.codex_auth_info.chatgpt_plan_type.trim();
    if plan.is_empty() {
        None
    } else {
        Some(plan)
    }
}

/// Parse callback URL and return authorization code after state validation.
pub fn resolve_callback_code_from_url(
    callback_url: &str,
    expected_state: &str,
) -> AppResult<String> {
    let parsed = parse_manual_callback_url(callback_url)?;

    if let Some(error) = parsed.error {
        return Err(AppError::Auth(format!("oauth callback error: {error}")));
    }

    let state = parsed
        .state
        .ok_or_else(|| AppError::Auth("oauth callback missing state".into()))?;
    validate_callback_state(expected_state, &state)?;

    parsed
        .code
        .ok_or_else(|| AppError::Auth("oauth callback missing code".into()))
}

/// Exchange authorization code for Codex tokens.
pub async fn exchange_code_for_tokens(
    client: &Client,
    code: &str,
    redirect_uri: &str,
    pkce_codes: &PKCECodes,
) -> AppResult<CodexAuthBundle> {
    exchange_code_for_tokens_with_redirect_url(client, code, redirect_uri, pkce_codes, TOKEN_URL)
        .await
}

/// Exchange authorization code for Codex tokens with a custom redirect URI.
pub async fn exchange_code_for_tokens_with_redirect(
    client: &Client,
    code: &str,
    redirect_uri: &str,
    pkce_codes: &PKCECodes,
) -> AppResult<CodexAuthBundle> {
    exchange_code_for_tokens_with_redirect_url(client, code, redirect_uri, pkce_codes, TOKEN_URL)
        .await
}

pub(crate) async fn exchange_code_for_tokens_with_redirect_url(
    client: &Client,
    code: &str,
    redirect_uri: &str,
    pkce_codes: &PKCECodes,
    token_url: &str,
) -> AppResult<CodexAuthBundle> {
    if code.trim().is_empty() {
        return Err(AppError::BadRequest("code must not be empty".into()));
    }
    if redirect_uri.trim().is_empty() {
        return Err(AppError::BadRequest(
            "redirect_uri must not be empty".into(),
        ));
    }
    if pkce_codes.code_verifier.trim().is_empty() {
        return Err(AppError::BadRequest(
            "code_verifier must not be empty".into(),
        ));
    }

    let token_url = token_url.trim();
    if token_url.is_empty() {
        return Err(AppError::BadRequest(
            "token exchange url must not be empty".into(),
        ));
    }

    let params = [
        ("grant_type", "authorization_code"),
        ("client_id", CLIENT_ID),
        ("code", code.trim()),
        ("redirect_uri", redirect_uri.trim()),
        ("code_verifier", pkce_codes.code_verifier.trim()),
    ];

    let response = client
        .post(token_url)
        .form(&params)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|error| AppError::Auth(format!("codex token exchange request failed: {error}")))?;

    let status = response.status();
    let body_text = response.text().await.map_err(|error| {
        AppError::Auth(format!("failed to read token exchange response: {error}"))
    })?;

    if !status.is_success() {
        return Err(AppError::Auth(format!(
            "codex token exchange failed ({status}): {body_text}"
        )));
    }

    let payload: CodexTokenEndpointResponse =
        serde_json::from_str(&body_text).map_err(|error| {
            AppError::Auth(format!("failed to parse token exchange response: {error}"))
        })?;

    build_auth_bundle_from_token_payload(payload)
}

/// Run interactive Codex OAuth login and persist resulting auth file.
pub async fn login(store: &FileTokenStore) -> AppResult<std::path::PathBuf> {
    let state = uuid::Uuid::new_v4().to_string();
    let pkce = generate_pkce_codes();

    let auth_url = generate_auth_url(&state, &pkce, REDIRECT_URI)?;

    println!("\nOpen this URL in your browser to authenticate Codex:\n");
    println!("  {auth_url}\n");

    if open::that(&auth_url).is_err() {
        println!("(Could not open browser automatically — please open the URL manually)");
    }

    print!("Paste callback URL: ");
    std::io::Write::flush(&mut std::io::stdout())
        .map_err(|error| AppError::Auth(format!("failed to flush stdout: {error}")))?;

    let mut callback_url = String::new();
    std::io::stdin()
        .read_line(&mut callback_url)
        .map_err(|error| AppError::Auth(format!("failed to read callback url: {error}")))?;

    let code = resolve_callback_code_from_url(&callback_url, &state)?;
    let client = Client::new();
    let bundle =
        exchange_code_for_tokens_with_redirect(&client, &code, REDIRECT_URI, &pkce).await?;

    save_auth_bundle(store, &bundle, true).await
}

fn build_auth_bundle_from_token_payload(
    payload: CodexTokenEndpointResponse,
) -> AppResult<CodexAuthBundle> {
    let id_token = payload.id_token.trim();
    let access_token = payload.access_token.trim();
    let refresh_token = payload.refresh_token.trim();

    if id_token.is_empty() || access_token.is_empty() || refresh_token.is_empty() {
        return Err(AppError::Auth(
            "codex token payload missing required token fields".into(),
        ));
    }

    let claims = parse_jwt_token(id_token)?;
    let email = get_user_email(&claims)
        .ok_or_else(|| AppError::Auth("codex id_token missing email claim".into()))?
        .to_string();
    let account_id = get_account_id(&claims)
        .ok_or_else(|| AppError::Auth("codex id_token missing account id claim".into()))?
        .to_string();

    let now = Utc::now();
    let expires_in = payload.expires_in.max(0);
    let expired = (now + Duration::seconds(expires_in)).to_rfc3339();

    Ok(CodexAuthBundle {
        token_data: CodexTokenData {
            id_token: id_token.to_string(),
            access_token: access_token.to_string(),
            refresh_token: refresh_token.to_string(),
            account_id,
            email,
            expired,
        },
        last_refresh: now.to_rfc3339(),
    })
}

fn decode_base64url(data: &str) -> AppResult<Vec<u8>> {
    let mut padded = data.trim().to_string();
    match padded.len() % 4 {
        2 => padded.push_str("=="),
        3 => padded.push('='),
        _ => {}
    }

    URL_SAFE
        .decode(padded)
        .map_err(|error| AppError::Auth(format!("failed to decode JWT payload: {error}")))
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{derive_code_challenge, exchange_code_for_tokens_with_redirect_url, PKCECodes};
    use axum::{routing::post, Router};
    use std::time::{Duration, Instant};

    #[tokio::test]
    async fn token_exchange_request_times_out_for_slow_endpoint() {
        let app = Router::new().route(
            "/oauth/token",
            post(|| async {
                tokio::time::sleep(Duration::from_secs(30)).await;
                axum::Json(serde_json::json!({
                    "access_token": "access",
                    "refresh_token": "refresh",
                    "id_token": "id",
                    "expires_in": 3600
                }))
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let addr = listener.local_addr().expect("read mock server addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let client = reqwest::Client::new();
        let pkce = PKCECodes {
            code_verifier: "verifier".into(),
            code_challenge: derive_code_challenge("verifier"),
        };

        let start = Instant::now();
        let error = exchange_code_for_tokens_with_redirect_url(
            &client,
            "auth_code",
            "http://localhost:1455/auth/callback",
            &pkce,
            &format!("http://{addr}/oauth/token"),
        )
        .await
        .expect_err("slow token endpoint should time out");

        assert!(start.elapsed() < Duration::from_secs(15));
        assert!(error
            .to_string()
            .contains("codex token exchange request failed"));
    }
}
