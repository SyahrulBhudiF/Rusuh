//! GitHub Copilot runtime token exchange and live model helpers.

use chrono::{DateTime, Duration, Utc};
use reqwest::Client;
use serde::Deserialize;

use crate::auth::github_copilot::COPILOT_API_TOKEN_URL;

const COPILOT_USER_AGENT: &str = "GitHubCopilotChat/0.26.7";
const COPILOT_EDITOR_VERSION: &str = "vscode/1.99.0";
const COPILOT_EDITOR_PLUGIN_VERSION: &str = "copilot-chat/0.26.7";
const COPILOT_INTEGRATION_ID: &str = "vscode-chat";
const COPILOT_OPENAI_INTENT: &str = "conversation-panel";
use crate::providers::model_info::ExtModelInfo;
use crate::error::{AppError, AppResult};

const REFRESH_BUFFER_MINUTES: i64 = 5;

#[derive(Debug, Clone, Deserialize)]
pub struct CopilotApiToken {
    pub token: String,
    #[serde(default)]
    pub expires_at: i64,
    #[serde(default)]
    pub endpoint: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LiveModelsResponse {
    data: Vec<ExtModelInfo>,
}

pub fn is_trusted_copilot_host(host: &str) -> bool {
    matches!(
        host.trim().to_ascii_lowercase().as_str(),
        "api.githubcopilot.com"
            | "api.individual.githubcopilot.com"
            | "api.business.githubcopilot.com"
            | "copilot-proxy.githubusercontent.com"
    )
}

pub fn token_is_still_valid_until(expires_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    now < (expires_at - Duration::minutes(REFRESH_BUFFER_MINUTES))
}

pub async fn exchange_github_token_for_copilot_token(
    client: &Client,
    github_oauth_token: &str,
) -> AppResult<CopilotApiToken> {
    exchange_github_token_for_copilot_token_with_url(client, github_oauth_token, COPILOT_API_TOKEN_URL)
        .await
}

pub async fn exchange_github_token_for_copilot_token_with_url(
    client: &Client,
    github_oauth_token: &str,
    url: &str,
) -> AppResult<CopilotApiToken> {
    if github_oauth_token.trim().is_empty() {
        return Err(AppError::BadRequest(
            "github oauth token must not be empty".into(),
        ));
    }

    let response = client
        .get(url)
        .bearer_auth(github_oauth_token.trim())
        .header(reqwest::header::USER_AGENT, "rusuh")
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|error| AppError::Auth(format!("copilot token exchange request failed: {error}")))?;

    let status = response.status();
    let body_text = response
        .text()
        .await
        .map_err(|error| AppError::Auth(format!("failed to read copilot token exchange response: {error}")))?;

    if !status.is_success() {
        return Err(AppError::Auth(format!(
            "copilot token exchange failed ({status}): {}",
            body_text.trim()
        )));
    }

    let token: CopilotApiToken = serde_json::from_str(&body_text)
        .map_err(|error| AppError::Auth(format!("failed to parse copilot token exchange response: {error}")))?;

    if token.token.trim().is_empty() {
        return Err(AppError::Auth(
            "copilot token exchange response missing token".into(),
        ));
    }

    if let Some(endpoint) = token.endpoint.as_deref() {
        let parsed = reqwest::Url::parse(endpoint)
            .map_err(|error| AppError::Auth(format!("invalid copilot endpoint URL: {error}")))?;
        let host = parsed
            .host_str()
            .ok_or_else(|| AppError::Auth("copilot endpoint URL missing host".into()))?;
        if !is_trusted_copilot_host(host) {
            return Err(AppError::Auth(format!(
                "untrusted copilot endpoint host: {host}"
            )));
        }
    }

    Ok(token)
}

pub async fn list_models(
    client: &Client,
    base_url: &str,
    copilot_api_token: &str,
) -> AppResult<Vec<ExtModelInfo>> {
    if base_url.trim().is_empty() {
        return Err(AppError::BadRequest(
            "copilot models base URL must not be empty".into(),
        ));
    }
    if copilot_api_token.trim().is_empty() {
        return Err(AppError::BadRequest(
            "copilot api token must not be empty".into(),
        ));
    }

    let response = client
        .get(format!("{}/models", base_url.trim().trim_end_matches('/')))
        .bearer_auth(copilot_api_token.trim())
        .header(reqwest::header::USER_AGENT, COPILOT_USER_AGENT)
        .header(reqwest::header::ACCEPT, "application/json")
        .header("editor-version", COPILOT_EDITOR_VERSION)
        .header("editor-plugin-version", COPILOT_EDITOR_PLUGIN_VERSION)
        .header("copilot-integration-id", COPILOT_INTEGRATION_ID)
        .header("openai-intent", COPILOT_OPENAI_INTENT)
        .send()
        .await
        .map_err(|error| AppError::Auth(format!("live model request failed: {error}")))?;

    let status = response.status();
    let body_text = response
        .text()
        .await
        .map_err(|error| AppError::Auth(format!("failed to read live model response: {error}")))?;

    if !status.is_success() {
        return Err(AppError::Auth(format!(
            "live model request failed (status {}): {}",
            status.as_u16(),
            body_text.trim()
        )));
    }

    let payload: LiveModelsResponse = serde_json::from_str(&body_text)
        .map_err(|error| AppError::Auth(format!("failed to parse live model response: {error}")))?;

    if payload.data.is_empty() {
        return Err(AppError::Auth(
            "live model response returned no usable models".into(),
        ));
    }

    Ok(payload.data)
}
