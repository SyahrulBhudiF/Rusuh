//! Social OAuth client for KIRO Google and GitHub authentication.
//!
//! Implements OAuth 2.0 with PKCE for Google and GitHub social login.
//! Mirrors Go implementation from CLIProxyAPIPlus internal/auth/kiro/social.go.

use std::sync::Arc;

use axum::{
    extract::Query,
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use serde::Deserialize;
use tokio::sync::oneshot;
use tracing::{debug, info, warn};

use crate::auth::kiro::{KiroTokenData, CALLBACK_PORT, KIRO_AUTH_ENDPOINT, REFRESH_SKEW_SECS};
use crate::error::{AppError, AppResult};

// ── PKCE Helper Functions ────────────────────────────────────────────────────

/// Generate a random code verifier for PKCE (43-128 characters, base64url).
fn generate_code_verifier() -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use rand::RngExt;

    let mut random_bytes = [0u8; 32];
    rand::rng().fill(&mut random_bytes);
    URL_SAFE_NO_PAD.encode(random_bytes)
}

/// Generate code challenge from verifier using SHA256 (base64url).
fn generate_code_challenge(verifier: &str) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();

    URL_SAFE_NO_PAD.encode(hash)
}

/// Generate random state parameter for OAuth flow.
fn generate_state() -> String {
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
    access_token: String,
    refresh_token: Option<String>,
    profile_arn: Option<String>,
    expires_in: Option<i64>,
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
        let description = params.error_description.unwrap_or_default();
        let error_msg = format!("OAuth error: {} - {}", error, description);
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
            let body = resp.text().await.unwrap_or_default();
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
        provider_id: &str,
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
            "{}/oauth/authorize?response_type=code&client_id=kiro-ide&redirect_uri={}&state={}&code_challenge={}&code_challenge_method=S256&provider={}",
            KIRO_AUTH_ENDPOINT,
            urlencoding::encode(&redirect_uri),
            urlencoding::encode(&state),
            urlencoding::encode(&code_challenge),
            provider_id
        );

        println!("\n┌─────────────────────────────────────────────────────────┐");
        println!("│  {} Authentication                              │", provider_name);
        println!("├─────────────────────────────────────────────────────────┤");
        println!("│                                                         │");
        println!("│  Opening browser for authentication...                  │");
        println!("│                                                         │");
        println!("│  If browser doesn't open, visit:                       │");
        println!("│  {}  │", &auth_url[..57.min(auth_url.len())]);
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
            + chrono::Duration::seconds(
                token_resp.expires_in.unwrap_or(3600) - REFRESH_SKEW_SECS,
            );

        Ok(KiroTokenData {
            access_token: token_resp.access_token,
            refresh_token: token_resp.refresh_token.unwrap_or_default(),
            profile_arn: token_resp.profile_arn.unwrap_or_default(),
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

// ── CLI Login Functions ────────────────────────────────────────────────────────

use crate::auth::store::FileTokenStore;

/// CLI login for KIRO via social OAuth (Google or GitHub).
pub async fn login(store: &FileTokenStore, provider: &str) -> AppResult<()> {
    let client = SocialAuthClient::new();
    // Call the appropriate login method based on provider
    let token_data = match provider {
        "google" => client.login_with_google().await?,
        "github" => client.login_with_github().await?,
        _ => return Err(AppError::Auth(format!("Unsupported provider: {}", provider))),
    };
    // Save to store - need to convert metadata to HashMap
    let mut metadata = std::collections::HashMap::new();
    metadata.insert("access_token".to_string(), serde_json::json!(token_data.access_token));
    metadata.insert("refresh_token".to_string(), serde_json::json!(token_data.refresh_token));
    metadata.insert("profile_arn".to_string(), serde_json::json!(token_data.profile_arn));
    metadata.insert("expires_at".to_string(), serde_json::json!(token_data.expires_at));
    metadata.insert("auth_method".to_string(), serde_json::json!(token_data.auth_method));
    metadata.insert("provider".to_string(), serde_json::json!(token_data.provider));
    metadata.insert("region".to_string(), serde_json::json!(token_data.region));
    if let Some(email) = &token_data.email {
        metadata.insert("email".to_string(), serde_json::json!(email));
    }
    let record = crate::auth::store::AuthRecord {
        id: format!("kiro-{}-{}", provider, uuid::Uuid::new_v4()),
        provider: "kiro".to_string(),
        label: format!("KIRO ({}) - {}", provider, token_data.email.as_deref().unwrap_or("user")),
        disabled: false,
        status: crate::auth::store::AuthStatus::Active,
        status_message: None,
        last_refreshed_at: Some(chrono::Utc::now()),
        updated_at: chrono::Utc::now(),
        path: std::path::PathBuf::new(),
        metadata,
    };
    store.save(&record).await?;
    println!("✓ KIRO {} login successful! Saved as: {}", provider, record.id);
    Ok(())
}
