//! KIRO (AWS CodeWhisperer) provider — translates OpenAI chat completions to/from KIRO's Claude-like API.
//!
//! KIRO uses AWS Event Stream binary protocol for streaming responses.
//! Supports multiple auth methods: Builder ID, Social (Google/GitHub), Enterprise IDC.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use tokio::sync::RwLock;
use tracing::debug;

use crate::auth::kiro::{KiroTokenData, BUILDER_ID_START_URL, REFRESH_SKEW_SECS};
use crate::auth::kiro_runtime::{QuotaStatus, UsageCheckRequest};
use crate::auth::kiro_login::{SSOOIDCClient, SocialAuthClient};
use crate::auth::store::AuthRecord;
use crate::error::{AppError, AppResult};
use crate::models::{ChatCompletionRequest, ChatCompletionResponse, ModelInfo};
use crate::providers::kiro_outcome::{
    classify_kiro_response, cooldown_for_outcome, cooldown_reason_for_outcome,
    registry_action_for_outcome, KiroRequestOutcome, RegistryAction,
};
use crate::providers::model_registry::ModelRegistry;
use crate::providers::{BoxStream, Provider};
use crate::proxy::KiroRuntimeState;

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
    registry_client_id: String,
    auth_key: String,
    token: RwLock<TokenState>,
    client: reqwest::Client,
    /// Path to auth file on disk for persisting refreshed tokens
    auth_file_path: PathBuf,
    /// API endpoint (CodeWhisperer or AmazonQ)
    endpoint: String,
    /// Retry configuration
    retry_config: RetryConfig,
    model_registry: Arc<ModelRegistry>,
    kiro_runtime: KiroRuntimeState,
}

impl KiroProvider {
    pub fn new(record: AuthRecord) -> AppResult<Self> {
        let client_id = record.id.clone();
        Self::new_with_runtime(
            record,
            client_id,
            Arc::new(ModelRegistry::new()),
            KiroRuntimeState::default(),
        )
    }

    pub fn new_with_runtime(
        record: AuthRecord,
        client_id: String,
        model_registry: Arc<ModelRegistry>,
        kiro_runtime: KiroRuntimeState,
    ) -> AppResult<Self> {
        let auth_file_path = record.path.clone();
        let account_name = record.label.clone();
        let auth_key = record.id.clone();

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
            registry_client_id: client_id,
            auth_key,
            token: RwLock::new(token),
            client: reqwest::Client::new(),
            auth_file_path,
            endpoint: endpoint_url.to_string(),
            retry_config: RetryConfig::default(),
            model_registry,
            kiro_runtime,
        })
    }

    async fn pre_request_check(&self, model_id: &str, access_token: &str) -> AppResult<()> {
        let now = Instant::now();

        {
            let mut cooldown = self.kiro_runtime.cooldown.write().await;
            cooldown.purge_expired(now);
            if cooldown.is_in_cooldown(&self.auth_key, model_id, now) {
                let remaining = cooldown
                    .remaining_cooldown(&self.auth_key, model_id, now)
                    .unwrap_or_default();
                let reason = cooldown
                    .cooldown_reason(&self.auth_key, model_id, now)
                    .unwrap_or("cooldown");
                return Err(AppError::QuotaExceeded(format!(
                    "kiro auth {} in cooldown for {}s: {}",
                    self.registry_client_id,
                    remaining.as_secs(),
                    reason
                )));
            }
        }

        let rate_limiter_wait = {
            let limiter = self.kiro_runtime.rate_limiter.read().await;
            limiter.required_wait(&self.auth_key, now)
        };
        if let Some(wait) = rate_limiter_wait {
            tokio::time::sleep(wait).await;
        }

        let state = self.token.read().await;
        let quota = self
            .kiro_runtime
            .quota_checker
            .check_quota(&UsageCheckRequest {
                access_token: access_token.to_string(),
                profile_arn: state.profile_arn.clone(),
                client_id: state.client_id.clone(),
                refresh_token: Some(state.refresh_token.clone()),
            })
            .await;
        drop(state);

        match quota {
            QuotaStatus::Unknown => Ok(()),
            QuotaStatus::Available { .. } => {
                self.model_registry
                    .clear_quota_exceeded(&self.registry_client_id, model_id)
                    .await;
                Ok(())
            }
            QuotaStatus::Exhausted { detail } => {
                self.model_registry
                    .set_quota_exceeded(&self.registry_client_id, model_id)
                    .await;
                Err(AppError::QuotaExceeded(detail))
            }
        }
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

    /// Get endpoint candidates for fallback on quota exhaustion.
    /// Returns both CodeWhisperer and AmazonQ endpoints.
    /// For test endpoints (localhost/127.0.0.1), returns only the configured endpoint.
    fn endpoint_candidates(&self) -> Vec<String> {
        // If using a test/local endpoint, don't try fallback
        if self.endpoint.contains("localhost") || self.endpoint.contains("127.0.0.1") {
            return vec![self.endpoint.clone()];
        }

        // Try primary endpoint first, then alternate
        if self.endpoint.contains("codewhisperer") {
            vec![CODEWHISPERER_ENDPOINT.to_string(), AMAZONQ_ENDPOINT.to_string()]
        } else {
            vec![AMAZONQ_ENDPOINT.to_string(), CODEWHISPERER_ENDPOINT.to_string()]
        }
    }

    async fn apply_request_outcome(
        &self,
        model_id: &str,
        outcome: &KiroRequestOutcome,
        now: Instant,
    ) {
        match registry_action_for_outcome(outcome) {
            RegistryAction::None => {}
            RegistryAction::ClearFailureState => {
                self.model_registry
                    .clear_quota_exceeded(&self.registry_client_id, model_id)
                    .await;
                self.model_registry
                    .resume_client_model(&self.registry_client_id, model_id)
                    .await;
            }
            RegistryAction::MarkQuotaExceeded => {
                self.model_registry
                    .set_quota_exceeded(&self.registry_client_id, model_id)
                    .await;
            }
            RegistryAction::SuspendClient { reason } => {
                self.model_registry
                    .suspend_client_model(&self.registry_client_id, model_id, &reason)
                    .await;
            }
        }

        {
            let mut limiter = self.kiro_runtime.rate_limiter.write().await;
            match outcome {
                KiroRequestOutcome::Success => limiter.mark_token_success(&self.auth_key),
                KiroRequestOutcome::RateLimited { .. } | KiroRequestOutcome::QuotaExhausted => {
                    limiter.mark_token_failed(&self.auth_key, now);
                }
                KiroRequestOutcome::Suspended => {
                    limiter.check_and_mark_suspended(&self.auth_key, "SUSPENDED", now);
                }
                _ => {}
            }
        }

        {
            let mut cooldown = self.kiro_runtime.cooldown.write().await;
            match outcome {
                KiroRequestOutcome::Success => cooldown.clear_cooldown(&self.auth_key, model_id),
                _ => {
                    if let (Some(duration), Some(reason)) = (
                        cooldown_for_outcome(outcome),
                        cooldown_reason_for_outcome(outcome),
                    ) {
                        cooldown.set_cooldown(&self.auth_key, model_id, duration, reason, now);
                    }
                }
            }
        }
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
        self.pre_request_check(&req.model, &token).await?;
        let kiro_request = translate_request_to_kiro(req);
        let model = req.model.clone();
        let chat_id = format!("chatcmpl-{}", uuid::Uuid::new_v4());
        let created = chrono::Utc::now().timestamp();

        // Get endpoint candidates for fallback
        let endpoints = self.endpoint_candidates();
        let mut last_error = None;

        // Try each endpoint
        for (endpoint_idx, endpoint) in endpoints.iter().enumerate() {
            let url = format!("{}{}", endpoint, CONVERSATION_PATH);
            let is_fallback = endpoint_idx > 0;

            if is_fallback {
                debug!(
                    provider = "kiro",
                    account = %self.account_name,
                    endpoint = endpoint,
                    "trying fallback endpoint after 429"
                );
            }

            // Make request with retry logic for this endpoint
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
                let outcome = classify_kiro_response(status.as_u16(), &body, attempt);
                self.apply_request_outcome(&req.model, &outcome, Instant::now())
                    .await;
                let err = AppError::Upstream(format!("kiro error ({}): {}", status, body));

                // On 429, try alternate endpoint if available
                if status == StatusCode::TOO_MANY_REQUESTS && endpoint_idx + 1 < endpoints.len() {
                    debug!(
                        provider = "kiro",
                        account = %self.account_name,
                        "429 from {}, will try alternate endpoint",
                        endpoint
                    );
                    last_error = Some(err);
                    break; // Break inner retry loop to try next endpoint
                }

                // On 403 suspension, don't try alternate endpoint
                if status == StatusCode::FORBIDDEN {
                    return Err(err);
                }

                if Self::is_retryable_status(status) && attempt < self.retry_config.max_retries {
                    last_error = Some(err);
                    continue;
                }
                return Err(err);
            }

            self.apply_request_outcome(&req.model, &KiroRequestOutcome::Success, Instant::now())
                .await;

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
        }

        // All endpoints and retries exhausted
        Err(last_error.unwrap_or_else(|| {
            AppError::Upstream("kiro stream failed after trying all endpoints".into())
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Instant;

    use axum::body::Body;
    use axum::http::StatusCode as AxumStatusCode;
    use axum::routing::post;
    use axum::Router;
    use crate::auth::kiro_runtime::{
        CooldownManager, KiroRateLimiter, NoOpQuotaChecker, QuotaChecker, UsageCheckRequest,
    };
    use crate::auth::store::AuthStatus;
    use crate::providers::model_registry::ModelRegistry;
    use crate::proxy::KiroRuntimeState;
    use serde_json::json;
    use tokio::sync::oneshot;
    use tokio::sync::RwLock;

    struct FakeExhaustedChecker;

    #[async_trait::async_trait]
    impl QuotaChecker for FakeExhaustedChecker {
        async fn check_quota(&self, _request: &UsageCheckRequest) -> crate::auth::kiro_runtime::QuotaStatus {
            crate::auth::kiro_runtime::QuotaStatus::Exhausted {
                detail: "test exhausted".into(),
            }
        }
    }

    struct FakeAvailableChecker;

    #[async_trait::async_trait]
    impl QuotaChecker for FakeAvailableChecker {
        async fn check_quota(&self, _request: &UsageCheckRequest) -> crate::auth::kiro_runtime::QuotaStatus {
            crate::auth::kiro_runtime::QuotaStatus::Available {
                remaining: Some(10),
                next_reset: None,
                breakdown: None,
            }
        }
    }

    fn test_record() -> AuthRecord {
        let metadata: HashMap<String, serde_json::Value> = serde_json::from_value(json!({
            "type": "kiro",
            "provider_key": "kiro",
            "access_token": "test-access-token",
            "refresh_token": "test-refresh-token",
            "profile_arn": "arn:aws:iam::123456789012:role/test",
            "expires_at": "2030-01-01T00:00:00Z",
            "auth_method": "builder-id",
            "provider": "AWS",
            "region": "us-east-1",
            "client_id": "test-client-id",
            "client_secret": "test-client-secret"
        }))
        .unwrap();

        AuthRecord {
            id: "kiro-test.json".into(),
            provider: "kiro".into(),
            provider_key: "kiro".into(),
            label: "kiro test".into(),
            disabled: false,
            status: AuthStatus::Active,
            status_message: None,
            last_refreshed_at: None,
            path: std::env::temp_dir().join("kiro-test.json"),
            metadata,
            updated_at: Utc::now(),
        }
    }

    fn runtime_with_checker(checker: Arc<dyn QuotaChecker>) -> KiroRuntimeState {
        KiroRuntimeState {
            cooldown: Arc::new(RwLock::new(CooldownManager::new())),
            rate_limiter: Arc::new(RwLock::new(KiroRateLimiter::new())),
            quota_checker: checker,
        }
    }

    fn test_request(model_id: &str) -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: model_id.to_string(),
            messages: vec![crate::models::ChatMessage {
                role: "user".into(),
                content: crate::models::MessageContent::Text("hello".into()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            stream: Some(true),
            max_tokens: Some(16),
            temperature: None,
            top_p: None,
            tools: None,
            tool_choice: None,
            stop: None,
            extra: HashMap::new(),
        }
    }

    fn create_event_stream_message(event_type: &str, payload: &[u8]) -> Vec<u8> {
        let mut message = Vec::new();

        let mut headers = Vec::new();
        headers.push(11u8);
        headers.extend_from_slice(b":event-type");
        headers.push(7u8);
        headers.push(0u8);
        headers.push(event_type.len() as u8);
        headers.extend_from_slice(event_type.as_bytes());

        let headers_length = headers.len() as u32;
        let payload_length = payload.len() as u32;
        let total_length = 12 + headers_length + payload_length + 4;

        message.extend_from_slice(&total_length.to_be_bytes());
        message.extend_from_slice(&headers_length.to_be_bytes());
        message.extend_from_slice(&[0u8; 4]);
        message.extend_from_slice(&headers);
        message.extend_from_slice(payload);
        message.extend_from_slice(&[0u8; 4]);

        message
    }

    fn success_stream_bytes() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&create_event_stream_message(
            "messageStart",
            br#"{}"#,
        ));
        bytes.extend_from_slice(&create_event_stream_message(
            "assistantResponseEvent",
            br#"{"content":"hello"}"#,
        ));
        bytes.extend_from_slice(&create_event_stream_message(
            "messageStop",
            br#"{"stopReason":"end_turn"}"#,
        ));
        bytes
    }

    async fn spawn_test_server(status: AxumStatusCode, body: Vec<u8>, content_type: &'static str) -> (String, oneshot::Sender<()>) {
        async fn conversation_handler(
            axum::extract::State(state): axum::extract::State<(AxumStatusCode, Vec<u8>, &'static str)>,
        ) -> impl axum::response::IntoResponse {
            let (status, body, content_type) = state;
            (status, [("content-type", content_type)], Body::from(body))
        }

        let app = Router::new()
            .route("/conversation", post(conversation_handler))
            .with_state((status, body, content_type));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });

        (format!("http://{}", addr), shutdown_tx)
    }

    #[tokio::test]
    async fn chat_completion_stream_success_clears_failure_state() {
        let registry = Arc::new(ModelRegistry::new());
        let runtime = runtime_with_checker(Arc::new(FakeAvailableChecker));
        let client_id = "kiro_0";
        let model_id = "claude-sonnet-4";

        registry
            .register_client(
                client_id,
                "kiro",
                vec![crate::providers::model_info::ExtModelInfo {
                    id: model_id.to_string(),
                    object: "model".to_string(),
                    created: 0,
                    owned_by: "kiro".to_string(),
                    provider_type: "kiro".to_string(),
                    display_name: None,
                    name: Some(model_id.to_string()),
                    version: None,
                    description: None,
                    input_token_limit: 0,
                    output_token_limit: 0,
                    supported_generation_methods: vec![],
                    context_length: 0,
                    max_completion_tokens: 0,
                    supported_parameters: vec![],
                    thinking: None,
                    user_defined: false,
                }],
            )
            .await;
        registry.set_quota_exceeded(client_id, model_id).await;
        registry
            .suspend_client_model(client_id, model_id, "old failure")
            .await;
        runtime.cooldown.write().await.set_cooldown(
            "kiro-test.json",
            model_id,
            Duration::from_secs(30),
            "old cooldown",
            Instant::now() - Duration::from_secs(31),
        );
        runtime.rate_limiter.write().await.mark_token_failed(
            "kiro-test.json",
            Instant::now() - Duration::from_secs(31),
        );

        let (endpoint, shutdown) =
            spawn_test_server(AxumStatusCode::OK, success_stream_bytes(), "application/vnd.amazon.eventstream").await;
        let mut provider = KiroProvider::new_with_runtime(
            test_record(),
            client_id.to_string(),
            registry.clone(),
            runtime.clone(),
        )
        .unwrap();
        provider.endpoint = endpoint;

        let result = provider.chat_completion_stream(&test_request(model_id)).await;
        let _ = shutdown.send(());

        let _stream = result.unwrap();
        assert!(registry.client_is_effectively_available(client_id, model_id).await);
        assert!(runtime
            .rate_limiter
            .read()
            .await
            .is_token_available("kiro-test.json", Instant::now()));
        assert!(!runtime
            .cooldown
            .read()
            .await
            .is_in_cooldown("kiro-test.json", model_id, Instant::now()));
    }

    #[tokio::test]
    async fn chat_completion_stream_429_marks_quota_and_cooldown() {
        let registry = Arc::new(ModelRegistry::new());
        let runtime = runtime_with_checker(Arc::new(NoOpQuotaChecker));
        let client_id = "kiro_0";
        let model_id = "claude-sonnet-4";

        registry
            .register_client(
                client_id,
                "kiro",
                vec![crate::providers::model_info::ExtModelInfo {
                    id: model_id.to_string(),
                    object: "model".to_string(),
                    created: 0,
                    owned_by: "kiro".to_string(),
                    provider_type: "kiro".to_string(),
                    display_name: None,
                    name: Some(model_id.to_string()),
                    version: None,
                    description: None,
                    input_token_limit: 0,
                    output_token_limit: 0,
                    supported_generation_methods: vec![],
                    context_length: 0,
                    max_completion_tokens: 0,
                    supported_parameters: vec![],
                    thinking: None,
                    user_defined: false,
                }],
            )
            .await;

        let (endpoint, shutdown) = spawn_test_server(
            AxumStatusCode::TOO_MANY_REQUESTS,
            br#"{"message":"quota exceeded"}"#.to_vec(),
            "application/json",
        )
        .await;

        let mut provider = KiroProvider::new_with_runtime(
            test_record(),
            client_id.to_string(),
            registry.clone(),
            runtime.clone(),
        )
        .unwrap();
        provider.endpoint = endpoint;

        let result = provider.chat_completion_stream(&test_request(model_id)).await;
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("Expected error but got success"),
        };
        let _ = shutdown.send(());

        assert!(matches!(err, AppError::Upstream(message) if message.contains("429")));
        assert!(!registry.client_is_effectively_available(client_id, model_id).await);
        assert!(runtime
            .cooldown
            .read()
            .await
            .is_in_cooldown("kiro-test.json", model_id, Instant::now()));
        assert_eq!(
            runtime
                .cooldown
                .read()
                .await
                .cooldown_reason("kiro-test.json", model_id, Instant::now()),
            Some("rate_limit_exceeded")
        );
        assert!(!runtime
            .rate_limiter
            .read()
            .await
            .is_token_available("kiro-test.json", Instant::now()));
    }

    #[tokio::test]
    async fn chat_completion_stream_suspended_marks_runtime_unavailable() {
        let registry = Arc::new(ModelRegistry::new());
        let runtime = runtime_with_checker(Arc::new(NoOpQuotaChecker));
        let client_id = "kiro_0";
        let model_id = "claude-sonnet-4";

        registry
            .register_client(
                client_id,
                "kiro",
                vec![crate::providers::model_info::ExtModelInfo {
                    id: model_id.to_string(),
                    object: "model".to_string(),
                    created: 0,
                    owned_by: "kiro".to_string(),
                    provider_type: "kiro".to_string(),
                    display_name: None,
                    name: Some(model_id.to_string()),
                    version: None,
                    description: None,
                    input_token_limit: 0,
                    output_token_limit: 0,
                    supported_generation_methods: vec![],
                    context_length: 0,
                    max_completion_tokens: 0,
                    supported_parameters: vec![],
                    thinking: None,
                    user_defined: false,
                }],
            )
            .await;

        let (endpoint, shutdown) = spawn_test_server(
            AxumStatusCode::FORBIDDEN,
            b"TEMPORARILY_SUSPENDED".to_vec(),
            "text/plain",
        )
        .await;

        let mut provider = KiroProvider::new_with_runtime(
            test_record(),
            client_id.to_string(),
            registry.clone(),
            runtime.clone(),
        )
        .unwrap();
        provider.endpoint = endpoint;

        let result = provider.chat_completion_stream(&test_request(model_id)).await;
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("Expected error but got success"),
        };
        let _ = shutdown.send(());

        assert!(matches!(err, AppError::Upstream(message) if message.contains("403")));
        assert!(!registry.client_is_effectively_available(client_id, model_id).await);
        assert!(runtime
            .cooldown
            .read()
            .await
            .is_in_cooldown("kiro-test.json", model_id, Instant::now()));
        assert_eq!(
            runtime
                .cooldown
                .read()
                .await
                .cooldown_reason("kiro-test.json", model_id, Instant::now()),
            Some("account_suspended")
        );
        assert!(!runtime
            .rate_limiter
            .read()
            .await
            .is_token_available("kiro-test.json", Instant::now()));
    }

    #[tokio::test]
    async fn pre_request_check_blocks_auth_in_cooldown() {
        let registry = Arc::new(ModelRegistry::new());
        let runtime = runtime_with_checker(Arc::new(NoOpQuotaChecker));
        let client_id = "kiro_0";
        let model_id = "claude-sonnet-4";
        let record = test_record();
        let auth_key = record.id.clone();

        runtime.cooldown.write().await.set_cooldown(
            &auth_key,
            model_id,
            Duration::from_secs(30),
            "test cooldown",
            Instant::now(),
        );

        let provider = KiroProvider::new_with_runtime(record, client_id.to_string(), registry, runtime)
            .unwrap();

        let err = provider
            .pre_request_check(model_id, "test-access-token")
            .await
            .unwrap_err();

        assert!(matches!(err, AppError::QuotaExceeded(message) if message.contains("cooldown")));
    }

    #[tokio::test]
    async fn pre_request_check_waits_for_rate_limiter_before_continuing() {
        let registry = Arc::new(ModelRegistry::new());
        let runtime = runtime_with_checker(Arc::new(NoOpQuotaChecker));
        let client_id = "kiro_0";
        let model_id = "claude-sonnet-4";
        let record = test_record();
        let auth_key = record.id.clone();

        runtime.rate_limiter.write().await.mark_token_failed(
            &auth_key,
            Instant::now() - Duration::from_millis(29_700),
        );

        let provider = KiroProvider::new_with_runtime(record, client_id.to_string(), registry, runtime)
            .unwrap();

        let started = std::time::Instant::now();
        tokio::time::timeout(
            Duration::from_secs(2),
            provider.pre_request_check(model_id, "test-access-token"),
        )
        .await
        .unwrap()
        .unwrap();

        assert!(started.elapsed() >= Duration::from_millis(200));
    }

    #[tokio::test]
    async fn pre_request_check_marks_registry_on_exhausted_quota() {
        let registry = Arc::new(ModelRegistry::new());
        let runtime = runtime_with_checker(Arc::new(FakeExhaustedChecker));
        let client_id = "kiro_0";
        let model_id = "claude-sonnet-4";

        registry
            .register_client(
                client_id,
                "kiro",
                vec![crate::providers::model_info::ExtModelInfo {
                    id: model_id.to_string(),
                    object: "model".to_string(),
                    created: 0,
                    owned_by: "kiro".to_string(),
                    provider_type: "kiro".to_string(),
                    display_name: None,
                    name: Some(model_id.to_string()),
                    version: None,
                    description: None,
                    input_token_limit: 0,
                    output_token_limit: 0,
                    supported_generation_methods: vec![],
                    context_length: 0,
                    max_completion_tokens: 0,
                    supported_parameters: vec![],
                    thinking: None,
                    user_defined: false,
                }],
            )
            .await;

        let provider = KiroProvider::new_with_runtime(
            test_record(),
            client_id.to_string(),
            registry.clone(),
            runtime,
        )
        .unwrap();

        let err = provider
            .pre_request_check(model_id, "test-access-token")
            .await
            .unwrap_err();

        assert!(matches!(err, AppError::QuotaExceeded(_)));
        assert!(!registry.client_is_effectively_available(client_id, model_id).await);
    }

    #[tokio::test]
    async fn pre_request_check_clears_stale_quota_exceeded_on_available_quota() {
        let registry = Arc::new(ModelRegistry::new());
        let runtime = runtime_with_checker(Arc::new(FakeAvailableChecker));
        let client_id = "kiro_0";
        let model_id = "claude-sonnet-4";

        registry
            .register_client(
                client_id,
                "kiro",
                vec![crate::providers::model_info::ExtModelInfo {
                    id: model_id.to_string(),
                    object: "model".to_string(),
                    created: 0,
                    owned_by: "kiro".to_string(),
                    provider_type: "kiro".to_string(),
                    display_name: None,
                    name: Some(model_id.to_string()),
                    version: None,
                    description: None,
                    input_token_limit: 0,
                    output_token_limit: 0,
                    supported_generation_methods: vec![],
                    context_length: 0,
                    max_completion_tokens: 0,
                    supported_parameters: vec![],
                    thinking: None,
                    user_defined: false,
                }],
            )
            .await;
        registry.set_quota_exceeded(client_id, model_id).await;

        let provider = KiroProvider::new_with_runtime(
            test_record(),
            client_id.to_string(),
            registry.clone(),
            runtime,
        )
        .unwrap();

        provider
            .pre_request_check(model_id, "test-access-token")
            .await
            .unwrap();

        assert!(registry.client_is_effectively_available(client_id, model_id).await);
    }
}
