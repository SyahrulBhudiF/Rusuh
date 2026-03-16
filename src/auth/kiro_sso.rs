//! AWS SSO OIDC client for KIRO Builder ID and Enterprise IDC authentication.
//!
//! Implements device code flow for Builder ID and authorization code flow for Enterprise IDC.
//! Mirrors Go implementation from CLIProxyAPIPlus internal/auth/kiro/sso_oidc.go.

use std::time::Duration;

use serde::Deserialize;
use tokio::time::sleep;
use tracing::{debug, info};

use crate::auth::kiro::{
    KiroTokenData, BUILDER_ID_START_URL, DEFAULT_REGION, REFRESH_SKEW_SECS, SCOPES,
    SSO_OIDC_ENDPOINT,
};
use crate::error::{AppError, AppResult};

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
            let body = resp.text().await.unwrap_or_default();
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
            let body = resp.text().await.unwrap_or_default();
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
            let body = resp.text().await.unwrap_or_default();
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
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Auth(format!(
                "auth-code token exchange failed (status {}): {}",
                status, body
            )));
        }

        resp.json().await.map_err(|e| {
            AppError::Auth(format!("failed to parse auth-code token response: {}", e))
        })
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
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Auth(format!(
                "token refresh failed (status {}): {}",
                status, body
            )));
        }

        let result: CreateTokenResponse = resp
            .json()
            .await
            .map_err(|e| AppError::Auth(format!("failed to parse refresh token response: {}", e)))?;

        let expires_at = chrono::Utc::now() + chrono::Duration::seconds(i64::from(result.expires_in));

        Ok(KiroTokenData {
            access_token: result.access_token,
            refresh_token: result.refresh_token.unwrap_or_else(|| refresh_token.to_string()),
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
            let body = resp.text().await.unwrap_or_default();
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
            let body = resp.text().await.unwrap_or_default();
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
            refresh_token: token_resp.refresh_token.unwrap_or_default(),
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
        let state = crate::auth::kiro_social::generate_state();
        let code_verifier = crate::auth::kiro_social::generate_code_verifier();
        let code_challenge = crate::auth::kiro_social::generate_code_challenge(&code_verifier);
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

use crate::auth::store::FileTokenStore;

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
