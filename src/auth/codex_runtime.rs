//! Codex runtime helpers: refresh policy, refresh requests, and quota parsing.

use chrono::{DateTime, Duration, Utc};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;

use crate::auth::codex::{CodexTokenData, CLIENT_ID, REFRESH_LEAD_DAYS, TOKEN_URL};
use crate::auth::codex_login::{get_account_id, get_user_email, parse_jwt_token};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Deserialize)]
struct RefreshTokenEndpointResponse {
    access_token: String,
    refresh_token: String,
    id_token: String,
    expires_in: i64,
}

/// Parse Codex retry-after seconds from a 429 usage_limit_reached error response.
pub fn parse_codex_retry_after_seconds(
    status_code: u16,
    error_body: &Value,
    now: DateTime<Utc>,
) -> Option<u64> {
    if status_code != 429 {
        return None;
    }

    let error = error_body.get("error")?;
    let error_type = error.get("type")?.as_str()?;
    if error_type != "usage_limit_reached" {
        return None;
    }

    if let Some(resets_at) = error.get("resets_at").and_then(Value::as_i64) {
        let diff = resets_at - now.timestamp();
        if diff > 0 {
            return Some(diff as u64);
        }
    }

    error.get("resets_in_seconds").and_then(Value::as_u64)
}

/// Parse `expired` value into UTC timestamp.
pub fn parse_expired(expired: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(expired)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

/// Whether token should be refreshed according to 5-day lead policy.
pub fn token_needs_refresh(expired: &str, now: DateTime<Utc>) -> bool {
    let Some(expiry) = parse_expired(expired) else {
        return true;
    };

    now >= (expiry - Duration::days(REFRESH_LEAD_DAYS))
}

/// Return whether a refresh error is non-retryable.
pub fn is_non_retryable_refresh_error(message: &str) -> bool {
    message.to_lowercase().contains("refresh_token_reused")
}

/// Refresh Codex tokens using refresh token grant.
pub async fn refresh_tokens(client: &Client, refresh_token: &str) -> AppResult<CodexTokenData> {
    if refresh_token.trim().is_empty() {
        return Err(AppError::BadRequest(
            "refresh_token must not be empty".into(),
        ));
    }

    let params = [
        ("grant_type", "refresh_token"),
        ("client_id", CLIENT_ID),
        ("refresh_token", refresh_token.trim()),
        ("scope", "openid profile email"),
    ];

    let response = client
        .post(TOKEN_URL)
        .form(&params)
        .send()
        .await
        .map_err(|error| AppError::Auth(format!("codex token refresh request failed: {error}")))?;

    let status = response.status();
    let body_text = response.text().await.map_err(|error| {
        AppError::Auth(format!("failed to read token refresh response: {error}"))
    })?;

    if !status.is_success() {
        return Err(AppError::Auth(format!(
            "codex token refresh failed ({status}): {body_text}"
        )));
    }

    let payload: RefreshTokenEndpointResponse =
        serde_json::from_str(&body_text).map_err(|error| {
            AppError::Auth(format!("failed to parse token refresh response: {error}"))
        })?;

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
    let expired = (Utc::now() + Duration::seconds(payload.expires_in.max(0))).to_rfc3339();

    Ok(CodexTokenData {
        id_token: id_token.to_string(),
        access_token: access_token.to_string(),
        refresh_token: refresh_token.to_string(),
        account_id,
        email,
        expired,
    })
}

/// Refresh tokens with bounded retries unless a non-retryable error is returned.
pub async fn refresh_tokens_with_retry(
    client: &Client,
    refresh_token: &str,
    max_retries: usize,
) -> AppResult<CodexTokenData> {
    let attempts = max_retries.max(1);
    let mut last_error: Option<AppError> = None;

    for attempt in 0..attempts {
        match refresh_tokens(client, refresh_token).await {
            Ok(token_data) => return Ok(token_data),
            Err(error) => {
                if is_non_retryable_refresh_error(&error.to_string()) {
                    return Err(error);
                }
                last_error = Some(error);
                if attempt + 1 < attempts {
                    tokio::time::sleep(std::time::Duration::from_secs((attempt + 1) as u64)).await;
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| AppError::Auth("codex token refresh failed".into())))
}
