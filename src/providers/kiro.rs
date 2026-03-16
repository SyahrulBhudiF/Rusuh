//! KIRO (AWS CodeWhisperer) provider — translates OpenAI chat completions to/from KIRO's Claude-like API.
//!
//! KIRO uses AWS Event Stream binary protocol for streaming responses.
//! Supports multiple auth methods: Builder ID, Social (Google/GitHub), Enterprise IDC.

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use tokio::sync::RwLock;
use tracing::debug;

use crate::auth::kiro::{KiroTokenData, BUILDER_ID_START_URL, REFRESH_SKEW_SECS};
use crate::auth::kiro_sso::SSOOIDCClient;
use crate::auth::kiro_social::SocialAuthClient;
use crate::auth::store::AuthRecord;
use crate::error::{AppError, AppResult};
use crate::models::{ChatCompletionRequest, ChatCompletionResponse, ModelInfo};
use crate::providers::{BoxStream, Provider};

// ── Constants ────────────────────────────────────────────────────────────────

/// CodeWhisperer API endpoint
const CODEWHISPERER_ENDPOINT: &str = "https://codewhisperer.us-east-1.amazonaws.com";

/// AmazonQ API endpoint
const AMAZONQ_ENDPOINT: &str = "https://amazonq.us-east-1.amazonaws.com";

/// Conversation endpoint path
const CONVERSATION_PATH: &str = "/conversation";

/// User agent for KIRO requests
const USER_AGENT: &str = "rusuh/0.1.0";

// ── Retry Configuration ──────────────────────────────────────────────────────

/// Retry configuration for socket errors and transient failures
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_retries: usize,
    /// Base delay between retries
    pub base_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Timeout for first token in streaming responses
    pub first_token_timeout: Duration,
    /// Timeout for reading stream data
    pub stream_read_timeout: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            first_token_timeout: Duration::from_secs(15),
            stream_read_timeout: Duration::from_secs(300),
        }
    }
}

// ── Token State ──────────────────────────────────────────────────────────────

/// Runtime token state, wrapped in RwLock for safe concurrent refresh.
pub struct TokenState {
    pub access_token: String,
    pub refresh_token: String,
    pub profile_arn: String,
    /// Absolute expiry time (UTC)
    pub expires_at: DateTime<Utc>,
    /// Authentication method
    pub auth_method: String,
    /// OAuth provider
    pub provider: String,
    /// OIDC client ID for refresh-capable flows
    pub client_id: Option<String>,
    /// OIDC client secret for refresh-capable flows
    pub client_secret: Option<String>,
    /// AWS region for OIDC refresh
    pub region: String,
    /// Optional AWS start URL
    pub start_url: Option<String>,
    /// Optional user email
    pub email: Option<String>,
    /// Tracks last successful refresh for logging
    pub last_refreshed_at: Option<DateTime<Utc>>,
}

impl TokenState {
    fn from_token_data(data: &KiroTokenData) -> AppResult<Self> {
        let expires_at = crate::auth::kiro::parse_expiry_str(&data.expires_at)
            .ok_or_else(|| AppError::Auth("invalid expires_at format".into()))?;

        Ok(Self {
            access_token: data.access_token.clone(),
            refresh_token: data.refresh_token.clone(),
            profile_arn: data.profile_arn.clone(),
            expires_at,
            auth_method: data.auth_method.clone(),
            provider: data.provider.clone(),
            client_id: data.client_id.clone(),
            client_secret: data.client_secret.clone(),
            region: data.region.clone(),
            start_url: data.start_url.clone(),
            email: data.email.clone(),
            last_refreshed_at: None,
        })
    }

    /// Check whether the token needs refreshing (expired or within skew window).
    pub fn needs_refresh(&self) -> bool {
        if self.access_token.is_empty() {
            return true;
        }
        let now = Utc::now();
        let deadline = self.expires_at - chrono::Duration::seconds(REFRESH_SKEW_SECS);
        now >= deadline
    }
}

// ── Provider ─────────────────────────────────────────────────────────────────

pub struct KiroProvider {
    account_name: String,
    token: RwLock<TokenState>,
    client: reqwest::Client,
    /// Path to auth file on disk for persisting refreshed tokens
    auth_file_path: PathBuf,
    /// API endpoint (CodeWhisperer or AmazonQ)
    endpoint: String,
    /// Retry configuration
    retry_config: RetryConfig,
}

impl KiroProvider {
    pub fn new(record: AuthRecord) -> AppResult<Self> {
        let auth_file_path = record.path.clone();
        let account_name = record.label.clone();

        // Extract KIRO token data from metadata
        let token_data = Self::extract_token_data(&record)?;
        let token = TokenState::from_token_data(&token_data)?;

        // Determine endpoint (default to CodeWhisperer)
        let endpoint = record
            .metadata
            .get("endpoint")
            .and_then(|v| v.as_str())
            .unwrap_or("codewhisperer")
            .to_lowercase();

        let endpoint_url = match endpoint.as_str() {
            "amazonq" => AMAZONQ_ENDPOINT,
            _ => CODEWHISPERER_ENDPOINT,
        };

        Ok(Self {
            account_name,
            token: RwLock::new(token),
            client: reqwest::Client::new(),
            auth_file_path,
            endpoint: endpoint_url.to_string(),
            retry_config: RetryConfig::default(),
        })
    }

    /// Extract KiroTokenData from AuthRecord metadata
    fn extract_token_data(record: &AuthRecord) -> AppResult<KiroTokenData> {
        let access_token = record
            .access_token()
            .ok_or_else(|| AppError::Auth("missing access_token".into()))?
            .to_string();

        let refresh_token = record
            .metadata
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::Auth("missing refresh_token".into()))?
            .to_string();

        let profile_arn = record
            .metadata
            .get("profile_arn")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        let expires_at = record
            .metadata
            .get("expires_at")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::Auth("missing expires_at".into()))?
            .to_string();

        let auth_method = record
            .metadata
            .get("auth_method")
            .and_then(|v| v.as_str())
            .unwrap_or("builder-id")
            .to_string();

        let provider = record
            .metadata
            .get("provider")
            .and_then(|v| v.as_str())
            .unwrap_or("AWS")
            .to_string();

        let region = record
            .metadata
            .get("region")
            .and_then(|v| v.as_str())
            .unwrap_or("us-east-1")
            .to_string();

        Ok(KiroTokenData {
            access_token,
            refresh_token,
            profile_arn,
            expires_at,
            auth_method,
            provider,
            client_id: record
                .metadata
                .get("client_id")
                .and_then(|v| v.as_str())
                .map(String::from),
            client_secret: record
                .metadata
                .get("client_secret")
                .and_then(|v| v.as_str())
                .map(String::from),
            region,
            start_url: record
                .metadata
                .get("start_url")
                .and_then(|v| v.as_str())
                .map(String::from),
            email: record
                .metadata
                .get("email")
                .and_then(|v| v.as_str())
                .map(String::from),
        })
    }

    /// Get a valid access token, refreshing if within the 50-minute skew window.
    async fn ensure_access_token(&self) -> AppResult<String> {
        // Fast path: token is still valid
        {
            let state = self.token.read().await;
            if !state.needs_refresh() {
                return Ok(state.access_token.clone());
            }
        }

        // Slow path: need to refresh
        let mut state = self.token.write().await;
        // Double-check after acquiring write lock (another task may have refreshed)
        if !state.needs_refresh() {
            return Ok(state.access_token.clone());
        }
        if state.refresh_token.is_empty() {
            return Err(AppError::Auth("missing refresh_token".into()));
        }

        debug!(
            provider = "kiro",
            account = %self.account_name,
            auth_method = %state.auth_method,
            region = %state.region,
            "refreshing access token (expired or within {}s skew)",
            REFRESH_SKEW_SECS
        );

        let refresh_result = if state.auth_method == "social" {
            SocialAuthClient::new()
                .refresh_social_token(&state.refresh_token)
                .await?
        } else {
            let client_id = state
                .client_id
                .as_deref()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| AppError::Auth("missing client_id for kiro refresh".into()))?;
            let client_secret = state
                .client_secret
                .as_deref()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| AppError::Auth("missing client_secret for kiro refresh".into()))?;
            let start_url = state
                .start_url
                .as_deref()
                .filter(|value| !value.is_empty())
            .unwrap_or(BUILDER_ID_START_URL);
            SSOOIDCClient::new()
                .refresh_token_with_region(
                    client_id,
                    client_secret,
                    &state.refresh_token,
                    &state.region,
                    start_url,
                )
                .await?
        };
        state.access_token = refresh_result.access_token;
        state.refresh_token = refresh_result.refresh_token;
        state.expires_at = crate::auth::kiro::parse_expiry_str(&refresh_result.expires_at)
            .ok_or_else(|| AppError::Auth("invalid refreshed expires_at format".into()))?;
        state.last_refreshed_at = Some(Utc::now());

        let new_token = state.access_token.clone();

        if let Err(error) = self.persist_token(&state).await {
            tracing::warn!(
                provider = "kiro",
                account = %self.account_name,
                "failed to persist refreshed token: {error}"
            );
        }

        Ok(new_token)
    }


    /// Check if an error is retryable
    fn is_retryable_error(error: &AppError) -> bool {
        match error {
            AppError::Upstream(msg) => {
                // Socket errors, connection errors, timeouts
                msg.contains("connection")
                    || msg.contains("timeout")
                    || msg.contains("socket")
                    || msg.contains("broken pipe")
            }
            _ => false,
        }
    }

    /// Check if HTTP status code is retryable
    fn is_retryable_status(status: StatusCode) -> bool {
        matches!(
            status,
            StatusCode::BAD_GATEWAY
                | StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::GATEWAY_TIMEOUT
        )
    }

    /// Calculate retry delay with exponential backoff and jitter
    fn calculate_retry_delay(&self, attempt: usize) -> Duration {
        let base_millis = self.retry_config.base_delay.as_millis() as u64;
        let max_millis = self.retry_config.max_delay.as_millis() as u64;

        // Exponential backoff: base * 2^attempt
        let delay_millis = base_millis.saturating_mul(2u64.saturating_pow(attempt as u32));
        let delay_millis = delay_millis.min(max_millis);

        // Add ±30% jitter
        let jitter_range = (delay_millis * 3) / 10; // 30%
        let jitter = (rand::random::<u64>() % (jitter_range * 2))
            .saturating_sub(jitter_range);
        let final_delay = delay_millis.saturating_add(jitter);

        Duration::from_millis(final_delay)
    }

    async fn persist_token(&self, state: &TokenState) -> AppResult<()> {
        let refreshed_at = Utc::now().to_rfc3339();
        let data = tokio::fs::read_to_string(&self.auth_file_path)
            .await
            .map_err(|e| AppError::Config(format!("read auth file for persist: {e}")))?;
        let mut metadata: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&data)
            .map_err(|e| AppError::Config(format!("parse auth file for persist: {e}")))?;

        metadata.insert("type".into(), serde_json::json!("kiro"));
        metadata.insert("provider_key".into(), serde_json::json!("kiro"));
        metadata.insert("access_token".into(), serde_json::json!(state.access_token));
        metadata.insert("refresh_token".into(), serde_json::json!(state.refresh_token));
        metadata.insert("profile_arn".into(), serde_json::json!(state.profile_arn));
        metadata.insert("expires_at".into(), serde_json::json!(state.expires_at.to_rfc3339()));
        metadata.insert("auth_method".into(), serde_json::json!(state.auth_method));
        metadata.insert("provider".into(), serde_json::json!(state.provider));
        metadata.insert("region".into(), serde_json::json!(state.region));
        metadata.insert("last_refresh".into(), serde_json::json!(&refreshed_at));
        metadata.insert("last_refreshed_at".into(), serde_json::json!(&refreshed_at));
        metadata.remove("status_message");

        if let Some(client_id) = state.client_id.as_deref().filter(|value| !value.is_empty()) {
            metadata.insert("client_id".into(), serde_json::json!(client_id));
        }
        if let Some(client_secret) = state
            .client_secret
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            metadata.insert("client_secret".into(), serde_json::json!(client_secret));
        }
        if let Some(start_url) = state.start_url.as_deref().filter(|value| !value.is_empty()) {
            metadata.insert("start_url".into(), serde_json::json!(start_url));
        }
        if let Some(email) = state.email.as_deref().filter(|value| !value.is_empty()) {
            metadata.insert("email".into(), serde_json::json!(email));
        }

        let json = serde_json::to_string_pretty(&metadata)
            .map_err(|e| AppError::Config(format!("serialize auth file: {e}")))?;
        tokio::fs::write(&self.auth_file_path, json)
            .await
            .map_err(|e| AppError::Config(format!("write auth file: {e}")))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            let _ = tokio::fs::set_permissions(&self.auth_file_path, perms).await;
        }

        Ok(())
    }
}

#[async_trait]
impl Provider for KiroProvider {
    fn name(&self) -> &str {
        "kiro"
    }

    async fn list_models(&self) -> AppResult<Vec<ModelInfo>> {
        // KIRO doesn't have a models endpoint
        // Return static list based on endpoint type
        let now = chrono::Utc::now().timestamp();
        let models = if self.endpoint.contains("amazonq") {
            vec![
                ModelInfo {
                    id: "anthropic.claude-3-5-sonnet-20241022-v2:0".to_string(),
                    object: "model".into(),
                    created: now,
                    owned_by: "kiro".into(),
                },
                ModelInfo {
                    id: "anthropic.claude-3-5-haiku-20241022-v1:0".to_string(),
                    object: "model".into(),
                    created: now,
                    owned_by: "kiro".into(),
                },
            ]
        } else {
            vec![
                ModelInfo {
                    id: "anthropic.claude-3-5-sonnet-20241022-v2:0".to_string(),
                    object: "model".into(),
                    created: now,
                    owned_by: "kiro".into(),
                },
                ModelInfo {
                    id: "gpt-4o".to_string(),
                    object: "model".into(),
                    created: now,
                    owned_by: "kiro".into(),
                },
            ]
        };

        Ok(models)
    }

    async fn chat_completion(
        &self,
        _req: &ChatCompletionRequest,
    ) -> AppResult<ChatCompletionResponse> {
        // KIRO primarily supports streaming
        // Non-streaming would require buffering the entire stream
        Err(AppError::Upstream(
            "KIRO provider requires streaming mode".into(),
        ))
    }

    async fn chat_completion_stream(&self, req: &ChatCompletionRequest) -> AppResult<BoxStream> {
        use crate::providers::kiro_translator::{translate_kiro_event_to_openai_sse, translate_request_to_kiro};
        use crate::providers::kiro_stream::EventStreamParser;
        use futures::stream;
        use std::io::Cursor;

        let token = self.ensure_access_token().await?;
        let kiro_request = translate_request_to_kiro(req);
        let model = req.model.clone();
        let chat_id = format!("chatcmpl-{}", uuid::Uuid::new_v4());
        let created = chrono::Utc::now().timestamp();

        // Build request URL
        let url = format!("{}{}", self.endpoint, CONVERSATION_PATH);

        // Make request with retry logic
        let mut last_error = None;
        for attempt in 0..=self.retry_config.max_retries {
            if attempt > 0 {
                let delay = self.calculate_retry_delay(attempt - 1);
                debug!(
                    provider = "kiro",
                    account = %self.account_name,
                    attempt = attempt,
                    delay_ms = delay.as_millis(),
                    "retrying stream request"
                );
                tokio::time::sleep(delay).await;
            }

            let resp = self
                .client
                .post(&url)
                .bearer_auth(&token)
                .header("Content-Type", "application/json")
                .header("User-Agent", USER_AGENT)
                .header("Accept", "application/vnd.amazon.eventstream")
                .json(&kiro_request)
                .timeout(self.retry_config.stream_read_timeout)
                .send()
                .await;

            let resp = match resp {
                Ok(r) => r,
                Err(e) => {
                    let err = AppError::Upstream(format!("kiro request failed: {e}"));
                    if Self::is_retryable_error(&err) && attempt < self.retry_config.max_retries {
                        last_error = Some(err);
                        continue;
                    }
                    return Err(err);
                }
            };

            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                let err = AppError::Upstream(format!("kiro error ({}): {}", status, body));
                if Self::is_retryable_status(status) && attempt < self.retry_config.max_retries {
                    last_error = Some(err);
                    continue;
                }
                return Err(err);
            }

            // Success - process stream
            let bytes = resp.bytes().await.map_err(|e| {
                AppError::Upstream(format!("failed to read response body: {e}"))
            })?;

            // Parse AWS Event Stream
            let cursor = Cursor::new(bytes.as_ref());
            let parser = EventStreamParser::new(cursor);
            let messages = parser.parse_all().map_err(|e| {
                AppError::Upstream(format!("failed to parse event stream: {e}"))
            })?;

            // Convert to SSE stream
            let sse_chunks: Vec<_> = messages
                .into_iter()
                .filter_map(|msg| {
                    translate_kiro_event_to_openai_sse(
                        &msg.event_type,
                        &msg.payload,
                        &chat_id,
                        &model,
                        created,
                    )
                })
                .map(Ok)
                .collect();

            let stream = stream::iter(sse_chunks);
            return Ok(Box::pin(stream));
        }

        // All retries exhausted
        Err(last_error.unwrap_or_else(|| {
            AppError::Upstream("kiro stream failed after all retries".into())
        }))
    }
}
