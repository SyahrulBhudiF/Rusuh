//! Kiro login flows — social OAuth (Google/GitHub) and AWS SSO (Builder ID/IDC).
//!
//! Combines social OAuth with PKCE and AWS SSO OIDC device/auth-code flows.
//! Mirrors CLIProxyAPIPlus internal/auth/kiro/social.go and sso_oidc.go.

use std::sync::Arc;
use std::time::Duration;

use crate::auth::kiro::{
    KiroTokenData, BUILDER_ID_START_URL, CALLBACK_PORT, DEFAULT_REGION, KIRO_AUTH_ENDPOINT,
    REFRESH_SKEW_SECS, SCOPES, SSO_OIDC_ENDPOINT,
};
use crate::error::{AppError, AppResult};
use axum::{
    extract::Query,
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use tokio::time::sleep;
use tracing::{debug, info, warn};

// ── PKCE Helper Functions ────────────────────────────────────────────────────

/// Generate a random code verifier for PKCE (43-128 characters, base64url).
pub fn generate_code_verifier() -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use rand::RngExt;

    let mut random_bytes = [0u8; 32];
    rand::rng().fill(&mut random_bytes);
    URL_SAFE_NO_PAD.encode(random_bytes)
}

/// Generate code challenge from verifier using SHA256 (base64url).
pub fn generate_code_challenge(verifier: &str) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();

    URL_SAFE_NO_PAD.encode(hash)
}

/// Generate random state parameter for OAuth flow.
pub fn generate_state() -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use rand::RngExt;

    let mut random_bytes = [0u8; 16];
    rand::rng().fill(&mut random_bytes);
    URL_SAFE_NO_PAD.encode(random_bytes)
}

// ── OAuth Response Types ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "refreshToken")]
    refresh_token: Option<String>,
    #[serde(rename = "profileArn")]
    profile_arn: Option<String>,
    #[serde(rename = "expiresIn")]
    expires_in: Option<i64>,
}

#[derive(Debug, Serialize)]
struct RefreshTokenRequest<'a> {
    #[serde(rename = "refreshToken")]
    refresh_token: &'a str,
}

fn format_oauth_error_message(error: &str, description: Option<String>) -> String {
    match description {
        Some(description) if !description.trim().is_empty() => {
            format!("OAuth error: {error} - {}", description.trim())
        }
        _ => format!("OAuth error: {error}"),
    }
}

fn format_non_success_response_body<E>(body_result: Result<String, E>) -> String
where
    E: std::fmt::Display,
{
    match body_result {
        Ok(body) => body,
        Err(error) => format!("<failed to read response body: {error}>"),
    }
}

fn option_string_or_empty(value: Option<String>) -> String {
    value.as_deref().unwrap_or("").to_owned()
}

// ── Callback Server ──────────────────────────────────────────────────────────

struct CallbackState {
    expected_state: String,
    sender: tokio::sync::Mutex<Option<oneshot::Sender<AppResult<String>>>>,
}

async fn oauth_callback(
    Query(params): Query<CallbackQuery>,
    axum::extract::State(state): axum::extract::State<Arc<CallbackState>>,
) -> impl IntoResponse {
    // Check for OAuth error
    if let Some(error) = params.error {
        let error_msg = format_oauth_error_message(&error, params.error_description);
        warn!("{}", error_msg);

        // Send error through channel
        if let Some(sender) = state.sender.lock().await.take() {
            let _ = sender.send(Err(AppError::Auth(error_msg.clone())));
        }

        return Html(format!(
            r#"<!DOCTYPE html>
<html>
<head><title>Authentication Failed</title></head>
<body style="font-family: sans-serif; text-align: center; padding: 50px;">
    <h1>❌ Authentication Failed</h1>
    <p>{}</p>
    <p>You can close this window.</p>
</body>
</html>"#,
            error_msg
        ));
    }

    // Validate state parameter
    if let Some(ref received_state) = params.state {
        if received_state != &state.expected_state {
            let error_msg = "State parameter mismatch (possible CSRF attack)";
            warn!("{}", error_msg);

            if let Some(sender) = state.sender.lock().await.take() {
                let _ = sender.send(Err(AppError::Auth(error_msg.into())));
            }

            return Html(format!(
                r#"<!DOCTYPE html>
<html>
<head><title>Authentication Failed</title></head>
<body style="font-family: sans-serif; text-align: center; padding: 50px;">
    <h1>❌ Authentication Failed</h1>
    <p>{}</p>
    <p>You can close this window.</p>
</body>
</html>"#,
                error_msg
            ));
        }
    }

    // Extract authorization code
    match params.code {
        Some(code) => {
            info!("received OAuth callback with authorization code");

            // Send code through channel
            if let Some(sender) = state.sender.lock().await.take() {
                let _ = sender.send(Ok(code));
            }

            Html(
                r#"<!DOCTYPE html>
<html>
<head><title>Authentication Successful</title></head>
<body style="font-family: sans-serif; text-align: center; padding: 50px;">
    <h1>✅ Authentication Successful</h1>
    <p>You can close this window and return to the terminal.</p>
</body>
</html>"#
                    .to_string(),
            )
        }
        None => {
            let error_msg = "Missing authorization code in callback";
            warn!("{}", error_msg);

            if let Some(sender) = state.sender.lock().await.take() {
                let _ = sender.send(Err(AppError::Auth(error_msg.into())));
            }

            Html(format!(
                r#"<!DOCTYPE html>
<html>
<head><title>Authentication Failed</title></head>
<body style="font-family: sans-serif; text-align: center; padding: 50px;">
    <h1>❌ Authentication Failed</h1>
    <p>{}</p>
    <p>You can close this window.</p>
</body>
</html>"#,
                error_msg
            ))
        }
    }
}

/// Start local callback server and return redirect URI + code receiver.
async fn start_callback_server(
    expected_state: String,
) -> AppResult<(String, oneshot::Receiver<AppResult<String>>)> {
    let (sender, receiver) = oneshot::channel();

    let callback_state = Arc::new(CallbackState {
        expected_state,
        sender: tokio::sync::Mutex::new(Some(sender)),
    });

    let app = Router::new()
        .route("/oauth/callback", get(oauth_callback))
        .with_state(callback_state);

    let addr = format!("127.0.0.1:{}", CALLBACK_PORT);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| AppError::Auth(format!("failed to bind callback server: {}", e)))?;

    let redirect_uri = format!("http://localhost:{}/oauth/callback", CALLBACK_PORT);

    debug!("callback server listening on {}", addr);

    // Spawn server in background
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            warn!("callback server error: {}", e);
        }
    });

    Ok((redirect_uri, receiver))
}

// ── Social Auth Client ───────────────────────────────────────────────────────

pub struct SocialAuthClient {
    http_client: reqwest::Client,
}

impl SocialAuthClient {
    /// Create a new social auth client with default HTTP client.
    pub fn new() -> Self {
        Self {
            http_client: reqwest::Client::new(),
        }
    }

    /// Exchange authorization code for access token.
    async fn exchange_code_for_token(
        &self,
        code: &str,
        code_verifier: &str,
        redirect_uri: &str,
    ) -> AppResult<TokenResponse> {
        let url = format!("{}/oauth/token", KIRO_AUTH_ENDPOINT);

        let payload = serde_json::json!({
            "code": code,
            "code_verifier": code_verifier,
            "redirect_uri": redirect_uri,
            "grant_type": "authorization_code",
        });

        debug!("exchanging authorization code for token");

        let resp = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("User-Agent", "KiroIDE/1.0.0")
            .json(&payload)
            .send()
            .await
            .map_err(|e| AppError::Auth(format!("token exchange request failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = format_non_success_response_body(resp.text().await);
            return Err(AppError::Auth(format!(
                "token exchange failed (status {}): {}",
                status, body
            )));
        }

        let result: TokenResponse = resp
            .json()
            .await
            .map_err(|e| AppError::Auth(format!("failed to parse token response: {}", e)))?;

        Ok(result)
    }

    /// Refresh an existing social token using only the refresh token.
    pub async fn refresh_social_token(&self, refresh_token: &str) -> AppResult<KiroTokenData> {
        let url = format!("{}/refreshToken", KIRO_AUTH_ENDPOINT);
        let payload = RefreshTokenRequest { refresh_token };

        let resp = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("User-Agent", "KiroIDE/1.0.0")
            .header("Accept", "application/json, text/plain, */*")
            .json(&payload)
            .send()
            .await
            .map_err(|e| AppError::Auth(format!("refresh request failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = format_non_success_response_body(resp.text().await);
            return Err(AppError::Auth(format!(
                "token refresh failed (status {}): {}",
                status, body
            )));
        }

        let token_resp: TokenResponse = resp
            .json()
            .await
            .map_err(|e| AppError::Auth(format!("failed to parse refresh response: {}", e)))?;

        let expires_at = chrono::Utc::now()
            + chrono::Duration::seconds(token_resp.expires_in.unwrap_or(3600).max(1));

        Ok(KiroTokenData {
            access_token: token_resp.access_token,
            refresh_token: token_resp
                .refresh_token
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| refresh_token.to_string()),
            profile_arn: option_string_or_empty(token_resp.profile_arn),
            expires_at: expires_at.to_rfc3339(),
            auth_method: "social".to_string(),
            provider: "imported".to_string(),
            client_id: None,
            client_secret: None,
            region: "us-east-1".to_string(),
            start_url: None,
            email: None,
        })
    }

    /// Complete Google OAuth login flow.
    pub async fn login_with_google(&self) -> AppResult<KiroTokenData> {
        self.login_with_provider("google", "Google").await
    }

    /// Complete GitHub OAuth login flow.
    pub async fn login_with_github(&self) -> AppResult<KiroTokenData> {
        self.login_with_provider("github", "GitHub").await
    }

    /// Generic social OAuth flow for any provider.
    async fn login_with_provider(
        &self,
        _provider_id: &str,
        provider_name: &str,
    ) -> AppResult<KiroTokenData> {
        info!("starting {} OAuth login flow", provider_name);

        // Generate PKCE parameters
        let code_verifier = generate_code_verifier();
        let code_challenge = generate_code_challenge(&code_verifier);
        let state = generate_state();

        // Start callback server
        let (redirect_uri, receiver) = start_callback_server(state.clone()).await?;

        // Build authorization URL
        let auth_url = format!(
            "{}/login?idp={}&redirect_uri={}&code_challenge={}&code_challenge_method=S256&state={}&prompt=select_account",
            KIRO_AUTH_ENDPOINT,
            provider_name,
            urlencoding::encode(&redirect_uri),
            urlencoding::encode(&code_challenge),
            urlencoding::encode(&state),
        );

        println!("\n┌─────────────────────────────────────────────────────────┐");
        println!(
            "│  {} Authentication                              │",
            provider_name
        );
        println!("├─────────────────────────────────────────────────────────┤");
        println!("│                                                         │");
        println!("│  Opening browser for authentication...                  │");
        println!("│                                                         │");
        println!("│  If browser doesn't open, visit:                       │");
        println!("│  {}  │", truncate_for_terminal_preview(&auth_url, 57));
        println!("│                                                         │");
        println!("└─────────────────────────────────────────────────────────┘\n");

        // Try to open browser automatically
        if let Err(e) = open::that(&auth_url) {
            debug!("could not open browser automatically: {}", e);
        }

        info!("waiting for OAuth callback...");

        // Wait for callback
        let code = receiver
            .await
            .map_err(|_| AppError::Auth("callback channel closed unexpectedly".into()))??;

        // Exchange code for token
        let token_resp = self
            .exchange_code_for_token(&code, &code_verifier, &redirect_uri)
            .await?;

        info!("{} authentication successful", provider_name);

        // Convert to KiroTokenData
        let expires_at = chrono::Utc::now()
            + chrono::Duration::seconds(token_resp.expires_in.unwrap_or(3600) - REFRESH_SKEW_SECS);

        Ok(KiroTokenData {
            access_token: token_resp.access_token,
            refresh_token: option_string_or_empty(token_resp.refresh_token),
            profile_arn: option_string_or_empty(token_resp.profile_arn),
            expires_at: expires_at.to_rfc3339(),
            auth_method: "social".to_string(),
            provider: provider_name.to_string(),
            client_id: None,
            client_secret: None,
            region: "us-east-1".to_string(),
            start_url: None,
            email: None,
        })
    }
}

impl Default for SocialAuthClient {
    fn default() -> Self {
        Self::new()
    }
}

fn truncate_for_terminal_preview(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        format_non_success_response_body, format_oauth_error_message, truncate_for_terminal_preview,
    };

    #[test]
    fn preview_keeps_short_ascii_unchanged() {
        let input = "https://example.com/auth";
        assert_eq!(truncate_for_terminal_preview(input, 57), input);
    }

    #[test]
    fn preview_truncates_long_ascii_to_limit() {
        let input = "https://example.com/abcdefghijklmnopqrstuvwxyz0123456789/extra";
        let preview = truncate_for_terminal_preview(input, 10);

        assert_eq!(preview, "https://ex");
        assert_eq!(preview.chars().count(), 10);
    }

    #[test]
    fn preview_handles_multibyte_utf8_near_boundary() {
        let input = "https://example.com/こんにちは世界";
        let preview = truncate_for_terminal_preview(input, 22);

        assert!(std::str::from_utf8(preview.as_bytes()).is_ok());
        assert_eq!(preview.chars().count(), 22);
    }

    #[test]
    fn preview_keeps_exact_boundary_stable() {
        let input = "1234567890";
        assert_eq!(truncate_for_terminal_preview(input, 10), input);
    }

    #[test]
    fn oauth_error_message_omits_empty_description() {
        assert_eq!(
            format_oauth_error_message("access_denied", None),
            "OAuth error: access_denied"
        );
        assert_eq!(
            format_oauth_error_message("access_denied", Some(String::new())),
            "OAuth error: access_denied"
        );
    }

    #[test]
    fn oauth_error_message_includes_non_empty_description() {
        assert_eq!(
            format_oauth_error_message("access_denied", Some("user cancelled".to_string())),
            "OAuth error: access_denied - user cancelled"
        );
    }

    #[test]
    fn non_success_response_body_reports_read_errors() {
        let formatted = format_non_success_response_body(Err(std::io::Error::other("boom")));

        assert_eq!(formatted, "<failed to read response body: boom>");
    }

    #[test]
    fn non_success_response_body_preserves_text() {
        let formatted =
            format_non_success_response_body::<std::io::Error>(Ok("upstream body".into()));

        assert_eq!(formatted, "upstream body");
    }
}

// ── CLI Login Functions ────────────────────────────────────────────────────────

use crate::auth::store::FileTokenStore;

/// CLI login for KIRO via social OAuth (Google or GitHub).
pub async fn login(store: &FileTokenStore, provider: &str) -> AppResult<()> {
    let client = SocialAuthClient::new();
    // Call the appropriate login method based on provider
    let token_data = match provider {
        "google" => client.login_with_google().await?,
        "github" => client.login_with_github().await?,
        _ => {
            return Err(AppError::Auth(format!(
                "Unsupported provider: {}",
                provider
            )))
        }
    };
    let label = format!(
        "KIRO ({}) - {}",
        provider,
        token_data.email.as_deref().unwrap_or("user")
    );
    let record = crate::auth::kiro_record::KiroRecordInput {
        token_data,
        label: Some(label),
        source: crate::auth::kiro::KiroTokenSource::LegacySocial,
    }
    .into_auth_record();
    store.save(&record).await?;
    println!(
        "✓ KIRO {} login successful! Saved as: {}",
        provider, record.id
    );
    Ok(())
}

// ══════════════════════════════════════════════════════════════════════════════
// AWS SSO OIDC (Builder ID / Enterprise IDC)
// ══════════════════════════════════════════════════════════════════════════════

// ── Response Types ───────────────────────────────────────────────────────────

/// Response from AWS SSO OIDC client registration.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterClientResponse {
    pub client_id: String,
    pub client_secret: String,
    pub client_id_issued_at: i64,
    pub client_secret_expires_at: i64,
}

/// Response from AWS SSO OIDC device authorization start.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartDeviceAuthResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_in: i32,
    pub interval: i32,
}

/// Response from AWS SSO OIDC token creation.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i32,
    pub refresh_token: Option<String>,
}

/// Error response from AWS SSO OIDC.
#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: String,
    #[allow(dead_code)]
    error_description: Option<String>,
}

// ── SSO OIDC Client ──────────────────────────────────────────────────────────

pub struct SSOOIDCClient {
    http_client: reqwest::Client,
}

impl SSOOIDCClient {
    /// Create a new SSO OIDC client with default HTTP client.
    pub fn new() -> Self {
        Self {
            http_client: reqwest::Client::new(),
        }
    }

    /// Get OIDC endpoint for the given region.
    fn get_oidc_endpoint(region: &str) -> String {
        if region.is_empty() {
            return SSO_OIDC_ENDPOINT.to_string();
        }
        format!("https://oidc.{}.amazonaws.com", region)
    }

    /// Register a new OIDC client with AWS.
    pub async fn register_client(&self, region: &str) -> AppResult<RegisterClientResponse> {
        let endpoint = Self::get_oidc_endpoint(region);
        let url = format!("{}/client/register", endpoint);

        let payload = serde_json::json!({
            "clientName": "Kiro IDE",
            "clientType": "public",
            "scopes": SCOPES,
            "grantTypes": ["urn:ietf:params:oauth:grant-type:device_code", "refresh_token"],
        });

        debug!("registering OIDC client with AWS SSO");

        let resp = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("x-amz-target", "com.amazonaws.sso.oauth.RegisterClient")
            .json(&payload)
            .send()
            .await
            .map_err(|e| AppError::Auth(format!("register client request failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = format_non_success_response_body(resp.text().await);
            return Err(AppError::Auth(format!(
                "register client failed (status {}): {}",
                status, body
            )));
        }

        let result: RegisterClientResponse = resp
            .json()
            .await
            .map_err(|e| AppError::Auth(format!("failed to parse register response: {}", e)))?;

        debug!(
            "registered client: {} (expires at {})",
            result.client_id, result.client_secret_expires_at
        );

        Ok(result)
    }

    /// Register a new OIDC client for authorization-code flow.
    pub async fn register_client_for_auth_code(
        &self,
        redirect_uri: &str,
        issuer_url: &str,
        region: &str,
    ) -> AppResult<RegisterClientResponse> {
        let endpoint = Self::get_oidc_endpoint(region);
        let url = format!("{}/client/register", endpoint);
        let payload = serde_json::json!({
            "clientName": "Kiro IDE",
            "clientType": "public",
            "scopes": SCOPES,
            "grantTypes": ["authorization_code", "refresh_token"],
            "redirectUris": [redirect_uri],
            "issuerUrl": issuer_url,
        });

        let resp = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("x-amz-target", "com.amazonaws.sso.oauth.RegisterClient")
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                AppError::Auth(format!("register auth-code client request failed: {}", e))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = format_non_success_response_body(resp.text().await);
            return Err(AppError::Auth(format!(
                "register auth-code client failed (status {}): {}",
                status, body
            )));
        }

        resp.json().await.map_err(|e| {
            AppError::Auth(format!(
                "failed to parse auth-code register response: {}",
                e
            ))
        })
    }
    pub async fn create_token_with_auth_code(
        &self,
        client_id: &str,
        client_secret: &str,
        code: &str,
        code_verifier: &str,
        redirect_uri: &str,
    ) -> AppResult<CreateTokenResponse> {
        let payload = serde_json::json!({
            "clientId": client_id,
            "clientSecret": client_secret,
            "code": code,
            "codeVerifier": code_verifier,
            "redirectUri": redirect_uri,
            "grantType": "authorization_code",
        });

        let resp = self
            .http_client
            .post(format!("{}/token", SSO_OIDC_ENDPOINT))
            .header("Content-Type", "application/json")
            .header("x-amz-target", "com.amazonaws.sso.oauth.CreateToken")
            .json(&payload)
            .send()
            .await
            .map_err(|e| AppError::Auth(format!("auth-code token request failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = format_non_success_response_body(resp.text().await);
            return Err(AppError::Auth(format!(
                "auth-code token exchange failed (status {}): {}",
                status, body
            )));
        }

        resp.json()
            .await
            .map_err(|e| AppError::Auth(format!("failed to parse auth-code token response: {}", e)))
    }

    pub async fn create_token_with_auth_code_and_region(
        &self,
        client_id: &str,
        client_secret: &str,
        code: &str,
        code_verifier: &str,
        redirect_uri: &str,
        region: &str,
    ) -> AppResult<CreateTokenResponse> {
        let endpoint = Self::get_oidc_endpoint(region);
        let payload = serde_json::json!({
            "clientId": client_id,
            "clientSecret": client_secret,
            "code": code,
            "codeVerifier": code_verifier,
            "redirectUri": redirect_uri,
            "grantType": "authorization_code",
        });

        let resp = self
            .http_client
            .post(format!("{}/token", endpoint))
            .header("Content-Type", "application/json")
            .header("x-amz-target", "com.amazonaws.sso.oauth.CreateToken")
            .json(&payload)
            .send()
            .await
            .map_err(|e| AppError::Auth(format!("auth-code token request failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = format_non_success_response_body(resp.text().await);
            return Err(AppError::Auth(format!(
                "auth-code token exchange failed (status {}): {}",
                status, body
            )));
        }

        resp.json()
            .await
            .map_err(|e| AppError::Auth(format!("failed to parse auth-code token response: {}", e)))
    }

    pub async fn refresh_token_with_region(
        &self,
        client_id: &str,
        client_secret: &str,
        refresh_token: &str,
        region: &str,
        start_url: &str,
    ) -> AppResult<KiroTokenData> {
        let endpoint = Self::get_oidc_endpoint(region);
        let payload = serde_json::json!({
            "clientId": client_id,
            "clientSecret": client_secret,
            "refreshToken": refresh_token,
            "grantType": "refresh_token",
        });

        let resp = self
            .http_client
            .post(format!("{}/token", endpoint))
            .header("Content-Type", "application/json")
            .header("x-amz-target", "com.amazonaws.sso.oauth.CreateToken")
            .json(&payload)
            .send()
            .await
            .map_err(|e| AppError::Auth(format!("refresh token request failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = format_non_success_response_body(resp.text().await);
            return Err(AppError::Auth(format!(
                "token refresh failed (status {}): {}",
                status, body
            )));
        }

        let result: CreateTokenResponse = resp.json().await.map_err(|e| {
            AppError::Auth(format!("failed to parse refresh token response: {}", e))
        })?;

        let expires_at =
            chrono::Utc::now() + chrono::Duration::seconds(i64::from(result.expires_in));

        Ok(KiroTokenData {
            access_token: result.access_token,
            refresh_token: result
                .refresh_token
                .unwrap_or_else(|| refresh_token.to_string()),
            profile_arn: String::new(),
            expires_at: expires_at.to_rfc3339(),
            auth_method: "idc".to_string(),
            provider: "AWS".to_string(),
            client_id: Some(client_id.to_string()),
            client_secret: Some(client_secret.to_string()),
            region: region.to_string(),
            start_url: Some(start_url.to_string()),
            email: None,
        })
    }

    pub async fn fetch_builder_id_email(&self, access_token: &str) -> Option<String> {
        let resp = self
            .http_client
            .get(format!("{}/userinfo", SSO_OIDC_ENDPOINT))
            .bearer_auth(access_token)
            .header("Accept", "application/json")
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }

        let value: serde_json::Value = resp.json().await.ok()?;
        value
            .get("email")
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                value
                    .get("preferred_username")
                    .and_then(serde_json::Value::as_str)
            })
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    }
    pub fn build_builder_id_authorization_url(
        &self,
        client_id: &str,
        redirect_uri: &str,
        state: &str,
        code_challenge: &str,
    ) -> String {
        let scopes = [
            "codewhisperer:completions",
            "codewhisperer:analysis",
            "codewhisperer:conversations",
        ]
        .join(",");
        format!(
            "{}/authorize?response_type=code&client_id={}&redirect_uri={}&scopes={}&state={}&code_challenge={}&code_challenge_method=S256",
            SSO_OIDC_ENDPOINT,
            urlencoding::encode(client_id),
            urlencoding::encode(redirect_uri),
            urlencoding::encode(&scopes),
            urlencoding::encode(state),
            urlencoding::encode(code_challenge),
        )
    }

    /// Start device authorization flow for Builder ID.
    pub async fn start_device_authorization(
        &self,
        client_id: &str,
        client_secret: &str,
        start_url: &str,
        region: &str,
    ) -> AppResult<StartDeviceAuthResponse> {
        let endpoint = Self::get_oidc_endpoint(region);
        let url = format!("{}/device_authorization", endpoint);

        let payload = serde_json::json!({
            "clientId": client_id,
            "clientSecret": client_secret,
            "startUrl": start_url,
        });

        debug!("starting device authorization flow");

        let resp = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .header(
                "x-amz-target",
                "com.amazonaws.sso.oauth.StartDeviceAuthorization",
            )
            .json(&payload)
            .send()
            .await
            .map_err(|e| AppError::Auth(format!("device authorization request failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = format_non_success_response_body(resp.text().await);
            return Err(AppError::Auth(format!(
                "device authorization failed (status {}): {}",
                status, body
            )));
        }

        let result: StartDeviceAuthResponse = resp
            .json()
            .await
            .map_err(|e| AppError::Auth(format!("failed to parse device auth response: {}", e)))?;

        Ok(result)
    }

    /// Poll for token using device code.
    pub async fn poll_for_token(
        &self,
        client_id: &str,
        client_secret: &str,
        device_code: &str,
        interval: i32,
        region: &str,
    ) -> AppResult<CreateTokenResponse> {
        let endpoint = Self::get_oidc_endpoint(region);
        let url = format!("{}/token", endpoint);

        let mut poll_interval = Duration::from_secs(interval.max(1) as u64);
        let max_attempts = 120; // 10 minutes with 5s interval

        let payload = serde_json::json!({
            "clientId": client_id,
            "clientSecret": client_secret,
            "grantType": "urn:ietf:params:oauth:grant-type:device_code",
            "deviceCode": device_code,
        });

        for attempt in 1..=max_attempts {
            sleep(poll_interval).await;

            debug!("polling for token (attempt {})", attempt);

            let resp = self
                .http_client
                .post(&url)
                .header("Content-Type", "application/json")
                .header("x-amz-target", "com.amazonaws.sso.oauth.CreateToken")
                .json(&payload)
                .send()
                .await
                .map_err(|e| AppError::Auth(format!("token poll request failed: {}", e)))?;

            if resp.status().is_success() {
                let result: CreateTokenResponse = resp.json().await.map_err(|e| {
                    AppError::Auth(format!("failed to parse token response: {}", e))
                })?;
                info!("device authorization successful");
                return Ok(result);
            }

            // Check for error response
            let body = format_non_success_response_body(resp.text().await);
            if let Ok(err_resp) = serde_json::from_str::<ErrorResponse>(&body) {
                match err_resp.error.as_str() {
                    "authorization_pending" => {
                        // User hasn't authorized yet, continue polling
                        continue;
                    }
                    "slow_down" => {
                        // Increase polling interval
                        poll_interval += Duration::from_secs(5);
                        debug!(
                            "slow_down received, increasing interval to {:?}",
                            poll_interval
                        );
                        continue;
                    }
                    _ => {
                        return Err(AppError::Auth(format!(
                            "device authorization failed: {}",
                            err_resp.error
                        )));
                    }
                }
            }

            // Unknown error
            return Err(AppError::Auth(format!(
                "token poll failed with unexpected response: {}",
                body
            )));
        }

        Err(AppError::Auth(
            "device authorization timed out (user did not authorize)".into(),
        ))
    }

    /// Complete Builder ID login flow (register → device auth → poll).
    pub async fn login_with_builder_id(&self) -> AppResult<KiroTokenData> {
        let region = DEFAULT_REGION;

        info!("starting Builder ID login flow");

        // Step 1: Register client
        let client_resp = self.register_client(region).await?;

        // Step 2: Start device authorization
        let device_resp = self
            .start_device_authorization(
                &client_resp.client_id,
                &client_resp.client_secret,
                BUILDER_ID_START_URL,
                region,
            )
            .await?;

        // Display user code and verification URL
        println!("\n┌─────────────────────────────────────────────────────────┐");
        println!("│  AWS Builder ID Authentication                          │");
        println!("├─────────────────────────────────────────────────────────┤");
        println!("│                                                         │");
        println!("│  1. Open this URL in your browser:                     │");
        println!("│     {}  │", device_resp.verification_uri);
        println!("│                                                         │");
        println!("│  2. Enter this code:                                   │");
        println!(
            "│     {}                                        │",
            device_resp.user_code
        );
        println!("│                                                         │");
        println!(
            "│  Or visit: {}                                          │",
            device_resp.verification_uri_complete
        );
        println!("│                                                         │");
        println!("└─────────────────────────────────────────────────────────┘\n");

        // Try to open browser automatically
        if let Err(e) = open::that(&device_resp.verification_uri_complete) {
            debug!("could not open browser automatically: {}", e);
        }

        info!("waiting for user authorization...");

        // Step 3: Poll for token
        let token_resp = self
            .poll_for_token(
                &client_resp.client_id,
                &client_resp.client_secret,
                &device_resp.device_code,
                device_resp.interval,
                region,
            )
            .await?;

        // Convert to KiroTokenData
        let expires_at = chrono::Utc::now()
            + chrono::Duration::seconds(token_resp.expires_in as i64 - REFRESH_SKEW_SECS);

        Ok(KiroTokenData {
            access_token: token_resp.access_token,
            refresh_token: option_string_or_empty(token_resp.refresh_token),
            profile_arn: String::new(), // Will be populated by CodeWhisperer API
            expires_at: expires_at.to_rfc3339(),
            auth_method: "builder-id".to_string(),
            provider: "AWS".to_string(),
            client_id: Some(client_resp.client_id),
            client_secret: Some(client_resp.client_secret),
            region: region.to_string(),
            start_url: Some(BUILDER_ID_START_URL.to_string()),
            email: None,
        })
    }
}

impl Default for SSOOIDCClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
pub struct BuilderIdAuthCodeStart {
    pub auth_url: String,
    pub state: String,
    pub code_verifier: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

impl SSOOIDCClient {
    pub async fn prepare_builder_id_auth_code(
        &self,
        redirect_uri: &str,
    ) -> AppResult<BuilderIdAuthCodeStart> {
        let state = generate_state();
        let code_verifier = generate_code_verifier();
        let code_challenge = generate_code_challenge(&code_verifier);
        let registration = self
            .register_client_for_auth_code(redirect_uri, BUILDER_ID_START_URL, DEFAULT_REGION)
            .await?;
        let auth_url = self.build_builder_id_authorization_url(
            &registration.client_id,
            redirect_uri,
            &state,
            &code_challenge,
        );

        Ok(BuilderIdAuthCodeStart {
            auth_url,
            state,
            code_verifier,
            client_id: registration.client_id,
            client_secret: registration.client_secret,
            redirect_uri: redirect_uri.to_string(),
        })
    }
}

// ── CLI Login Functions ──────────────────────────────────────────────────────

/// CLI login for KIRO via AWS SSO (Builder ID or Enterprise IDC).
pub async fn login_sso(store: &FileTokenStore, start_url: &str) -> AppResult<()> {
    let client = SSOOIDCClient::new();
    // Determine if Builder ID or Enterprise IDC based on start_url
    let token_data = if start_url == BUILDER_ID_START_URL {
        client.login_with_builder_id().await?
    } else {
        return Err(AppError::Auth(
            "Enterprise IDC SSO not yet implemented. Use Builder ID or social OAuth.".into(),
        ));
    };
    let record = crate::auth::kiro_record::KiroRecordInput {
        token_data,
        label: Some(format!("KIRO (SSO) - {}", start_url)),
        source: crate::auth::kiro::KiroTokenSource::BuilderIdWeb,
    }
    .into_auth_record();
    store.save(&record).await?;
    println!("✓ KIRO SSO login successful! Saved as: {}", record.id);
    Ok(())
}
