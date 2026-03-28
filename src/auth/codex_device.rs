//! Codex device-flow parsing helpers.

use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auth::codex::{save_auth_bundle, CLIENT_ID};
use crate::auth::codex_login::{exchange_code_for_tokens_with_redirect_url, PKCECodes};
use crate::auth::store::FileTokenStore;
use crate::error::{AppError, AppResult};

#[derive(Debug, Serialize)]
struct DeviceUserCodeRequest {
    client_id: String,
}

#[derive(Debug, Serialize)]
struct DeviceTokenRequest {
    device_auth_id: String,
    user_code: String,
}

/// Parsed device-code bootstrap response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceUserCodeResponse {
    /// Device auth identifier.
    pub device_auth_id: String,
    /// User-facing code entered in browser flow.
    pub user_code: String,
    /// Browser URL where user enters device code.
    pub verification_uri: String,
    /// Direct URL that includes the user code when provided by upstream.
    pub verification_uri_complete: Option<String>,
    /// Polling interval in seconds.
    pub interval_secs: u64,
}

/// Parsed device-code token polling success payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceTokenResponse {
    /// Authorization code returned after user confirms device flow.
    pub authorization_code: String,
    /// PKCE verifier paired with authorization code.
    pub code_verifier: String,
    /// PKCE challenge paired with authorization code.
    pub code_challenge: String,
}

/// Request device user-code bootstrap payload from upstream.
pub async fn request_codex_device_user_code(
    client: &Client,
    user_code_url: &str,
    verification_url: &str,
) -> AppResult<DeviceUserCodeResponse> {
    let user_code_url = user_code_url.trim();
    if user_code_url.is_empty() {
        return Err(AppError::BadRequest(
            "codex device user-code URL must not be empty".into(),
        ));
    }

    let verification_url = verification_url.trim();
    if verification_url.is_empty() {
        return Err(AppError::BadRequest(
            "codex device verification URL must not be empty".into(),
        ));
    }

    let response = client
        .post(user_code_url)
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .json(&DeviceUserCodeRequest {
            client_id: CLIENT_ID.to_string(),
        })
        .send()
        .await
        .map_err(|e| AppError::Auth(format!("failed to request codex device code: {e}")))?;

    let status = response.status();
    let body_text = response
        .text()
        .await
        .map_err(|e| AppError::Auth(format!("failed to read codex device code response: {e}")))?;

    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(AppError::Auth(format!(
            "codex device endpoint is unavailable (status {})",
            status.as_u16()
        )));
    }

    if !codex_device_is_success_status(status.as_u16()) {
        let trimmed = if body_text.trim().is_empty() {
            "empty response body"
        } else {
            body_text.trim()
        };
        return Err(AppError::Auth(format!(
            "codex device code request failed with status {}: {}",
            status.as_u16(),
            trimmed
        )));
    }

    let mut payload: Value = serde_json::from_str(&body_text)
        .map_err(|e| AppError::Auth(format!("failed to decode codex device code response: {e}")))?;

    if let Some(object) = payload.as_object_mut() {
        let has_verification =
            object.contains_key("verification_uri") || object.contains_key("verification_url");
        if !has_verification {
            object.insert(
                "verification_uri".to_string(),
                json!(verification_url.to_string()),
            );
        }
    }

    parse_device_user_code_response(&payload)
}

/// Poll device-token endpoint until success or timeout.
pub async fn poll_codex_device_token(
    client: &Client,
    token_url: &str,
    device_auth_id: &str,
    user_code: &str,
    poll_interval: Duration,
    timeout: Duration,
) -> AppResult<DeviceTokenResponse> {
    let token_url = token_url.trim();
    if token_url.is_empty() {
        return Err(AppError::BadRequest(
            "codex device token URL must not be empty".into(),
        ));
    }

    let device_auth_id = device_auth_id.trim();
    if device_auth_id.is_empty() {
        return Err(AppError::BadRequest(
            "codex device auth id must not be empty".into(),
        ));
    }

    let user_code = user_code.trim();
    if user_code.is_empty() {
        return Err(AppError::BadRequest(
            "codex device user code must not be empty".into(),
        ));
    }

    let poll_interval = if poll_interval.is_zero() {
        Duration::from_secs(1)
    } else {
        poll_interval
    };

    let timeout = if timeout.is_zero() {
        Duration::from_secs(1)
    } else {
        timeout
    };

    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(AppError::Auth(
                "codex device authentication timed out after 15 minutes".into(),
            ));
        }

        let response = client
            .post(token_url)
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .json(&DeviceTokenRequest {
                device_auth_id: device_auth_id.to_string(),
                user_code: user_code.to_string(),
            })
            .send()
            .await
            .map_err(|e| AppError::Auth(format!("failed to poll codex device token: {e}")))?;

        let status = response.status();
        let body_text = response.text().await.map_err(|e| {
            AppError::Auth(format!("failed to read codex device poll response: {e}"))
        })?;

        if codex_device_is_success_status(status.as_u16()) {
            let payload: Value = serde_json::from_str(&body_text).map_err(|e| {
                AppError::Auth(format!("failed to decode codex device token response: {e}"))
            })?;
            return parse_device_token_response(&payload);
        }

        if matches!(
            status,
            reqwest::StatusCode::FORBIDDEN | reqwest::StatusCode::NOT_FOUND
        ) {
            tokio::time::sleep(poll_interval).await;
            continue;
        }

        let trimmed = if body_text.trim().is_empty() {
            "empty response body"
        } else {
            body_text.trim()
        };

        return Err(AppError::Auth(format!(
            "codex device token polling failed with status {}: {}",
            status.as_u16(),
            trimmed
        )));
    }
}

/// Parse user-code response payload.
pub fn parse_device_user_code_response(value: &Value) -> AppResult<DeviceUserCodeResponse> {
    let device_auth_id = value
        .get("device_auth_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| AppError::Auth("device flow response missing device_auth_id".into()))?
        .to_string();

    let user_code = value
        .get("user_code")
        .and_then(Value::as_str)
        .or_else(|| value.get("usercode").and_then(Value::as_str))
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| AppError::Auth("device flow response missing user_code".into()))?
        .to_string();

    let verification_uri = value
        .get("verification_uri")
        .and_then(Value::as_str)
        .or_else(|| value.get("verification_url").and_then(Value::as_str))
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| AppError::Auth("device flow response missing verification_uri".into()))?
        .to_string();

    let verification_uri_complete = value
        .get("verification_uri_complete")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned);

    let interval_secs =
        parse_codex_device_poll_interval_secs(value.get("interval").unwrap_or(&Value::Null));

    Ok(DeviceUserCodeResponse {
        device_auth_id,
        user_code,
        verification_uri,
        verification_uri_complete,
        interval_secs,
    })
}

/// Parse device token polling success payload.
pub fn parse_device_token_response(value: &Value) -> AppResult<DeviceTokenResponse> {
    let authorization_code = value
        .get("authorization_code")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    let code_verifier = value
        .get("code_verifier")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    let code_challenge = value
        .get("code_challenge")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");

    if authorization_code.is_empty() || code_verifier.is_empty() || code_challenge.is_empty() {
        return Err(AppError::Auth(
            "device token response missing required fields".into(),
        ));
    }

    Ok(DeviceTokenResponse {
        authorization_code: authorization_code.to_string(),
        code_verifier: code_verifier.to_string(),
        code_challenge: code_challenge.to_string(),
    })
}

/// Parse polling interval from number or numeric string.
pub fn parse_codex_device_poll_interval_secs(value: &Value) -> u64 {
    const DEFAULT_INTERVAL: u64 = 5;

    if let Some(raw) = value.as_u64() {
        return raw.max(1);
    }

    if let Some(raw) = value.as_i64() {
        return (raw.max(1)) as u64;
    }

    if let Some(raw) = value.as_str() {
        return raw.trim().parse::<u64>().unwrap_or(DEFAULT_INTERVAL).max(1);
    }

    DEFAULT_INTERVAL
}

/// Parse device code countdown start from number or numeric string.
pub fn parse_codex_device_countdown_start_secs(value: &Value) -> u64 {
    const DEFAULT_COUNTDOWN: u64 = 600;

    if let Some(raw) = value.as_u64() {
        return raw.max(1);
    }

    if let Some(raw) = value.as_i64() {
        return (raw.max(1)) as u64;
    }

    if let Some(raw) = value.as_str() {
        return raw
            .trim()
            .parse::<u64>()
            .unwrap_or(DEFAULT_COUNTDOWN)
            .max(1);
    }

    DEFAULT_COUNTDOWN
}

/// Return best browser URL for approving the device login.
pub fn codex_device_approval_url(payload: &DeviceUserCodeResponse) -> &str {
    payload
        .verification_uri_complete
        .as_deref()
        .unwrap_or(payload.verification_uri.as_str())
}

/// Codex device user-code endpoint.
pub const DEVICE_USER_CODE_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/usercode";
/// Codex device token polling endpoint.
pub const DEVICE_TOKEN_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/token";
/// Codex device browser verification URL.
pub const DEVICE_VERIFICATION_URL: &str = "https://auth.openai.com/codex/device";
/// Redirect URI used by Codex device token exchange.
pub const DEVICE_TOKEN_EXCHANGE_REDIRECT_URI: &str = "https://auth.openai.com/deviceauth/callback";
/// Default polling timeout for Codex device login.
pub const DEVICE_LOGIN_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Device endpoints used by Codex device login.
#[derive(Debug, Clone)]
pub struct CodexDeviceEndpoints {
    pub user_code_url: String,
    pub token_url: String,
    pub verification_url: String,
    pub token_exchange_url: String,
}

impl CodexDeviceEndpoints {
    pub fn production() -> Self {
        Self {
            user_code_url: DEVICE_USER_CODE_URL.to_string(),
            token_url: DEVICE_TOKEN_URL.to_string(),
            verification_url: DEVICE_VERIFICATION_URL.to_string(),
            token_exchange_url: crate::auth::codex::TOKEN_URL.to_string(),
        }
    }

    pub fn from_auth_base_url(base_url: &str) -> AppResult<Self> {
        let base_url = base_url.trim().trim_end_matches('/');
        if base_url.is_empty() {
            return Err(AppError::BadRequest(
                "codex auth base URL must not be empty".into(),
            ));
        }

        Ok(Self {
            user_code_url: format!("{base_url}/api/accounts/deviceauth/usercode"),
            token_url: format!("{base_url}/api/accounts/deviceauth/token"),
            verification_url: format!("{base_url}/api/accounts/deviceauth/verify"),
            token_exchange_url: format!("{base_url}/oauth/token"),
        })
    }
}

/// Codex device-login entrypoint.
pub async fn device_login(store: &FileTokenStore) -> AppResult<std::path::PathBuf> {
    let endpoints = match std::env::var("RUSUH_CODEX_AUTH_BASE_URL") {
        Ok(base_url) => CodexDeviceEndpoints::from_auth_base_url(&base_url)?,
        Err(_) => CodexDeviceEndpoints::production(),
    };

    device_login_with_endpoints(store, &reqwest::Client::new(), &endpoints).await
}

pub async fn device_login_with_endpoints(
    store: &FileTokenStore,
    client: &reqwest::Client,
    endpoints: &CodexDeviceEndpoints,
) -> AppResult<std::path::PathBuf> {
    let user_code = request_codex_device_user_code(
        client,
        &endpoints.user_code_url,
        &endpoints.verification_url,
    )
    .await?;

    let _countdown_secs = parse_codex_device_countdown_start_secs(&Value::Null);
    let approval_url = codex_device_approval_url(&user_code);
    let _ = open::that(approval_url);

    let poll_interval = Duration::from_secs(user_code.interval_secs.max(1));
    let token_payload = poll_codex_device_token(
        client,
        &endpoints.token_url,
        &user_code.device_auth_id,
        &user_code.user_code,
        poll_interval,
        DEVICE_LOGIN_TIMEOUT,
    )
    .await?;

    let bundle = exchange_code_for_tokens_with_redirect_url(
        client,
        token_payload.authorization_code.as_str(),
        DEVICE_TOKEN_EXCHANGE_REDIRECT_URI,
        &PKCECodes {
            code_verifier: token_payload.code_verifier,
            code_challenge: token_payload.code_challenge,
        },
        &endpoints.token_exchange_url,
    )
    .await?;

    save_auth_bundle(store, &bundle, true).await
}

/// Whether a status code is a successful polling result.
pub fn codex_device_is_success_status(code: u16) -> bool {
    (200..300).contains(&code)
}
