//! GitHub Copilot device login flow for GitHub.com.

use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;

use crate::auth::github_copilot::{
    save_auth_bundle, GithubCopilotAuthBundle, GithubOAuthTokenData, GithubUserInfo, CLIENT_ID,
    COPILOT_API_TOKEN_URL, DEVICE_CODE_URL, TOKEN_URL, USER_URL,
};
use crate::auth::store::FileTokenStore;
use crate::error::{AppError, AppResult};

const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct TokenGrantResponse {
    access_token: Option<String>,
    token_type: Option<String>,
    scope: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct CopilotEntitlementResponse {
    token: String,
}

#[derive(Debug, Clone)]
struct GithubCopilotLoginEndpoints {
    device_code_url: String,
    token_url: String,
    user_url: String,
    copilot_token_url: String,
}

impl GithubCopilotLoginEndpoints {
    fn production() -> Self {
        Self {
            device_code_url: DEVICE_CODE_URL.to_string(),
            token_url: TOKEN_URL.to_string(),
            user_url: USER_URL.to_string(),
            copilot_token_url: COPILOT_API_TOKEN_URL.to_string(),
        }
    }

    fn from_base_url(base_url: &str) -> AppResult<Self> {
        let base_url = base_url.trim().trim_end_matches('/');
        if base_url.is_empty() {
            return Err(AppError::BadRequest(
                "github copilot auth base URL must not be empty".into(),
            ));
        }

        Ok(Self {
            device_code_url: format!("{base_url}/login/device/code"),
            token_url: format!("{base_url}/login/oauth/access_token"),
            user_url: format!("{base_url}/user"),
            copilot_token_url: format!("{base_url}/copilot_internal/v2/token"),
        })
    }
}

pub async fn login(store: &FileTokenStore) -> AppResult<std::path::PathBuf> {
    let endpoints = GithubCopilotLoginEndpoints::production();
    let client = build_http_client()?;
    login_with_endpoints(store, &client, &endpoints).await
}

pub async fn login_with_base_url(store: &FileTokenStore, base_url: &str) -> AppResult<std::path::PathBuf> {
    let endpoints = GithubCopilotLoginEndpoints::from_base_url(base_url)?;
    let client = build_http_client()?;
    login_with_endpoints(store, &client, &endpoints).await
}

async fn login_with_endpoints(
    store: &FileTokenStore,
    client: &Client,
    endpoints: &GithubCopilotLoginEndpoints,
) -> AppResult<std::path::PathBuf> {
    let device_code = request_device_code(client, &endpoints.device_code_url).await?;

    if let Err(error) = open::that(&device_code.verification_uri) {
        tracing::warn!(
            "failed to open GitHub Copilot device verification URL automatically: {error}"
        );
    }

    let token_data = poll_for_github_token(client, &endpoints.token_url, &device_code).await?;
    let user_info = fetch_user_info(client, &endpoints.user_url, &token_data.access_token).await?;
    validate_copilot_entitlement(
        client,
        &endpoints.copilot_token_url,
        &token_data.access_token,
    )
    .await?;

    let bundle = GithubCopilotAuthBundle {
        token_data,
        user_info,
    };

    save_auth_bundle(store, &bundle).await
}

async fn request_device_code(client: &Client, url: &str) -> AppResult<DeviceCodeResponse> {
    let response = client
        .post(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(&[("client_id", CLIENT_ID), ("scope", "read:user")])
        .send()
        .await
        .map_err(|error| AppError::Auth(format!("failed to request github device code: {error}")))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::Auth(format!(
            "github device code request failed (status {}): {}",
            status.as_u16(),
            body.trim()
        )));
    }

    response
        .json::<DeviceCodeResponse>()
        .await
        .map_err(|error| AppError::Auth(format!("failed to parse github device code response: {error}")))
}

async fn poll_for_github_token(
    client: &Client,
    url: &str,
    device_code: &DeviceCodeResponse,
) -> AppResult<GithubOAuthTokenData> {
    let mut poll_interval = Duration::from_secs(device_code.interval.max(1));
    let max_deadline = tokio::time::Instant::now() + Duration::from_secs(device_code.expires_in.max(1));

    loop {
        tokio::time::sleep(poll_interval).await;

        let response = client
            .post(url)
            .header(reqwest::header::ACCEPT, "application/json")
            .form(&[
                ("client_id", CLIENT_ID),
                ("device_code", device_code.device_code.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await
            .map_err(|error| AppError::Auth(format!("failed to poll github device token: {error}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AppError::Auth(format!(
                "github device token polling failed (status {}): {}",
                status.as_u16(),
                body.trim()
            )));
        }

        let payload = response
            .json::<TokenGrantResponse>()
            .await
            .map_err(|error| AppError::Auth(format!("failed to parse github token response: {error}")))?;

        if let Some(error) = payload.error.as_deref() {
            match error {
                "authorization_pending" => {
                    if tokio::time::Instant::now() >= max_deadline {
                        return Err(AppError::Auth(
                            "github device authorization timed out".into(),
                        ));
                    }
                    continue;
                }
                "slow_down" => {
                    poll_interval += Duration::from_secs(5);
                    if tokio::time::Instant::now() >= max_deadline {
                        return Err(AppError::Auth(
                            "github device authorization timed out".into(),
                        ));
                    }
                    continue;
                }
                _ => {
                    return Err(AppError::Auth(format!(
                        "github device authorization failed: {error}"
                    )));
                }
            }
        }

        let access_token = payload
            .access_token
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| AppError::Auth("github token response missing access_token".into()))?;

        return Ok(GithubOAuthTokenData {
            access_token,
            token_type: payload.token_type.unwrap_or_else(|| "bearer".to_string()),
            scope: payload.scope.unwrap_or_default(),
        });
    }
}

async fn fetch_user_info(client: &Client, url: &str, access_token: &str) -> AppResult<GithubUserInfo> {
    let response = client
        .get(url)
        .bearer_auth(access_token)
        .header(reqwest::header::USER_AGENT, "rusuh")
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|error| AppError::Auth(format!("failed to fetch github user info: {error}")))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::Auth(format!(
            "github user info request failed (status {}): {}",
            status.as_u16(),
            body.trim()
        )));
    }

    response
        .json::<GithubUserInfo>()
        .await
        .map_err(|error| AppError::Auth(format!("failed to parse github user info: {error}")))
}

async fn validate_copilot_entitlement(client: &Client, url: &str, access_token: &str) -> AppResult<()> {
    let response = client
        .get(url)
        .bearer_auth(access_token)
        .header(reqwest::header::USER_AGENT, "rusuh")
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|error| AppError::Auth(format!("failed to validate copilot entitlement: {error}")))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::Auth(format!(
            "copilot entitlement validation failed (status {}): {}",
            status.as_u16(),
            body.trim()
        )));
    }

    let payload = response
        .json::<CopilotEntitlementResponse>()
        .await
        .map_err(|error| AppError::Auth(format!("failed to parse copilot entitlement response: {error}")))?;

    if payload.token.trim().is_empty() {
        return Err(AppError::Auth(
            "copilot entitlement response missing token".into(),
        ));
    }

    Ok(())
}

fn build_http_client() -> AppResult<Client> {
    Client::builder()
        .timeout(DEFAULT_HTTP_TIMEOUT)
        .build()
        .map_err(|error| AppError::Auth(format!("failed to build github copilot login client: {error}")))
}
