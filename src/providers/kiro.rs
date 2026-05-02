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
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;
use tracing::debug;

use crate::auth::kiro::{KiroTokenData, BUILDER_ID_START_URL, REFRESH_SKEW_SECS};
use crate::auth::kiro_login::{SSOOIDCClient, SocialAuthClient};
use crate::auth::kiro_runtime::{QuotaStatus, UsageCheckRequest};
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

/// Native Kiro endpoint path (used by Kiro IDE/CLI)
const GENERATE_ASSISTANT_RESPONSE_PATH: &str = "/generateAssistantResponse";

/// Kiro version for fingerprinting
const KIRO_VERSION: &str = "0.10.0";

/// Node version for fingerprinting
const NODE_VERSION: &str = "22.21.1";

/// System version options for fingerprinting
const SYSTEM_VERSIONS: &[&str] = &["darwin#24.6.0", "win32#10.0.22631"];

/// Public Kiro model IDs exposed by Rusuh.
const KIRO_MODEL_IDS: &[&str] = &[
    "kiro-auto",
    "kiro-claude-opus-4-6",
    "kiro-claude-sonnet-4-6",
    "kiro-claude-opus-4-5",
    "kiro-claude-sonnet-4-5",
    "kiro-claude-sonnet-4",
    "kiro-claude-haiku-4-5",
    "kiro-deepseek-3-2",
    "kiro-minimax-m2-1",
    "kiro-qwen3-coder-next",
    "kiro-claude-opus-4-6-agentic",
    "kiro-claude-sonnet-4-6-agentic",
    "kiro-claude-opus-4-5-agentic",
    "kiro-claude-sonnet-4-5-agentic",
    "kiro-claude-sonnet-4-agentic",
    "kiro-claude-haiku-4-5-agentic",
    "kiro-deepseek-3-2-agentic",
    "kiro-minimax-m2-1-agentic",
    "kiro-qwen3-coder-next-agentic",
];

// ── Machine ID Generation ────────────────────────────────────────────────────

/// Generate machine ID from Kiro token data.
///
/// Priority:
/// 1. Use refresh_token hash if available
/// 2. Use access_token hash as fallback
/// 3. Return None if no tokens available
fn generate_machine_id(token_data: &KiroTokenData) -> Option<String> {
    if !token_data.refresh_token.is_empty() {
        return Some(sha256_hex(&format!(
            "KotlinNativeAPI/{}",
            token_data.refresh_token
        )));
    }

    if !token_data.access_token.is_empty() {
        return Some(sha256_hex(&format!(
            "KotlinNativeAPI/{}",
            token_data.access_token
        )));
    }

    None
}

/// SHA256 hash implementation (returns hex string).
fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    result.iter().map(|b| format!("{:02x}", b)).collect()
}

fn format_upstream_error_body<E>(body_result: Result<String, E>) -> String
where
    E: std::fmt::Display,
{
    match body_result {
        Ok(body) => body,
        Err(error) => format!("<failed to read upstream error body: {error}>"),
    }
}

fn cooldown_remaining_or_zero(remaining: Option<Duration>) -> Duration {
    remaining.unwrap_or(Duration::ZERO)
}

fn profile_arn_or_empty(record: &AuthRecord) -> String {
    record
        .metadata
        .get("profile_arn")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string()
}

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
    token_data: KiroTokenData,
    client: reqwest::Client,
    /// Path to auth file on disk for persisting refreshed tokens
    auth_file_path: PathBuf,
    /// AWS region for API endpoint
    region: String,
    /// Retry configuration
    retry_config: RetryConfig,
    model_registry: Arc<ModelRegistry>,
    kiro_runtime: KiroRuntimeState,
    #[cfg(test)]
    test_endpoint: Option<String>,
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
        let region = token_data.region.clone();

        Ok(Self {
            account_name,
            registry_client_id: client_id,
            auth_key,
            token: RwLock::new(token),
            token_data,
            client: reqwest::Client::new(),
            auth_file_path,
            region,
            retry_config: RetryConfig::default(),
            model_registry,
            kiro_runtime,
            #[cfg(test)]
            test_endpoint: None,
        })
    }

    /// Build native Kiro endpoint URL: https://q.<region>.amazonaws.com/generateAssistantResponse
    fn build_endpoint_url(&self) -> String {
        #[cfg(test)]
        if let Some(endpoint) = &self.test_endpoint {
            return endpoint.clone();
        }

        format!(
            "https://q.{}.amazonaws.com{}",
            self.region, GENERATE_ASSISTANT_RESPONSE_PATH
        )
    }

    /// Build Kiro fingerprint headers matching kiro-client behavior
    fn build_kiro_headers(&self, token: &str) -> AppResult<reqwest::header::HeaderMap> {
        use reqwest::header::{
            HeaderMap, HeaderValue, AUTHORIZATION, CONNECTION, CONTENT_TYPE, HOST, USER_AGENT,
        };

        #[cfg(test)]
        if self.test_endpoint.is_some() {
            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", token))
                    .map_err(|e| AppError::Auth(format!("invalid authorization: {e}")))?,
            );
            headers.insert(USER_AGENT, HeaderValue::from_static("rusuh-test"));
            return Ok(headers);
        }

        let machine_id = generate_machine_id(&self.token_data)
            .ok_or_else(|| AppError::Auth("cannot generate machine_id".into()))?;

        // Select random system version for fingerprinting
        let system_version = SYSTEM_VERSIONS[0]; // Use first version for consistency

        // Build x-amz-user-agent
        let x_amz_user_agent = format!("aws-sdk-js/1.0.27 KiroIDE-{}-{}", KIRO_VERSION, machine_id);

        // Build full User-Agent
        let user_agent = format!(
            "aws-sdk-js/1.0.27 ua/2.1 os/{} lang/js md/nodejs#{} api/codewhispererstreaming#1.0.27 m/E KiroIDE-{}-{}",
            system_version, NODE_VERSION, KIRO_VERSION, machine_id
        );

        let host = format!("q.{}.amazonaws.com", self.region);

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            "x-amzn-codewhisperer-optout",
            HeaderValue::from_static("true"),
        );
        headers.insert("x-amzn-kiro-agent-mode", HeaderValue::from_static("vibe"));
        headers.insert(
            "x-amz-user-agent",
            HeaderValue::from_str(&x_amz_user_agent)
                .map_err(|e| AppError::Auth(format!("invalid x-amz-user-agent: {e}")))?,
        );
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(&user_agent)
                .map_err(|e| AppError::Auth(format!("invalid user-agent: {e}")))?,
        );
        headers.insert(
            HOST,
            HeaderValue::from_str(&host)
                .map_err(|e| AppError::Auth(format!("invalid host: {e}")))?,
        );
        headers.insert(
            "amz-sdk-invocation-id",
            HeaderValue::from_str(&uuid::Uuid::new_v4().to_string())
                .map_err(|e| AppError::Auth(format!("invalid invocation-id: {e}")))?,
        );
        headers.insert(
            "amz-sdk-request",
            HeaderValue::from_static("attempt=1; max=3"),
        );
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", token))
                .map_err(|e| AppError::Auth(format!("invalid authorization: {e}")))?,
        );
        headers.insert(CONNECTION, HeaderValue::from_static("close"));

        Ok(headers)
    }

    async fn pre_request_check(&self, model_id: &str, access_token: &str) -> AppResult<()> {
        let now = Instant::now();

        {
            let mut cooldown = self.kiro_runtime.cooldown.write().await;
            cooldown.purge_expired(now);
            if cooldown.is_in_cooldown(&self.auth_key, model_id, now) {
                let remaining = cooldown_remaining_or_zero(cooldown.remaining_cooldown(
                    &self.auth_key,
                    model_id,
                    now,
                ));
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

        let profile_arn = profile_arn_or_empty(record);

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

        self.apply_refreshed_token(&mut state, refresh_result).await
    }

    async fn apply_refreshed_token(
        &self,
        state: &mut TokenState,
        refresh_result: KiroTokenData,
    ) -> AppResult<String> {
        state.access_token = refresh_result.access_token;
        state.refresh_token = refresh_result.refresh_token;
        state.expires_at = crate::auth::kiro::parse_expiry_str(&refresh_result.expires_at)
            .ok_or_else(|| AppError::Auth("invalid refreshed expires_at format".into()))?;
        state.last_refreshed_at = Some(Utc::now());

        let new_token = state.access_token.clone();

        // A successful upstream refresh updates the in-memory token for this process immediately.
        // If persistence fails we warn and continue the current request, but a later restart may lose
        // the refreshed token because the auth file was not updated.
        if let Err(error) = self.persist_token(state).await {
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
            StatusCode::BAD_GATEWAY | StatusCode::SERVICE_UNAVAILABLE | StatusCode::GATEWAY_TIMEOUT
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
        let jitter = (rand::random::<u64>() % (jitter_range * 2)).saturating_sub(jitter_range);
        let final_delay = delay_millis.saturating_add(jitter);

        Duration::from_millis(final_delay)
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
        metadata.insert(
            "refresh_token".into(),
            serde_json::json!(state.refresh_token),
        );
        metadata.insert("profile_arn".into(), serde_json::json!(state.profile_arn));
        metadata.insert(
            "expires_at".into(),
            serde_json::json!(state.expires_at.to_rfc3339()),
        );
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

    /// Execute buffered Kiro request and return parsed event messages.
    ///
    /// This helper performs the upstream request with retry logic, applies request outcomes,
    /// and returns parsed EventStreamMessage objects for further processing.
    async fn execute_buffered_kiro_request(
        &self,
        req: &ChatCompletionRequest,
    ) -> AppResult<Vec<crate::providers::kiro_stream::EventStreamMessage>> {
        use crate::providers::kiro_stream::EventStreamParser;
        use crate::providers::kiro_translator::build_native_kiro_request;

        let token = self.ensure_access_token().await?;
        self.pre_request_check(&req.model, &token).await?;

        // Build native Kiro request with profile ARN
        let profile_arn = {
            let state = self.token.read().await;
            if state.profile_arn.is_empty() {
                None
            } else {
                Some(state.profile_arn.clone())
            }
        };
        let kiro_request = build_native_kiro_request(req, profile_arn);

        // Build endpoint URL and headers
        let url = self.build_endpoint_url();
        let headers = self.build_kiro_headers(&token)?;

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
                    "retrying buffered request"
                );
                tokio::time::sleep(delay).await;
            }

            let resp = self
                .client
                .post(&url)
                .headers(headers.clone())
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
                let body = format_upstream_error_body(resp.text().await);
                let outcome = classify_kiro_response(status.as_u16(), &body, attempt);
                self.apply_request_outcome(&req.model, &outcome, Instant::now())
                    .await;
                let err = AppError::Upstream(format!("kiro error ({}): {}", status, body));

                if Self::is_retryable_status(status) && attempt < self.retry_config.max_retries {
                    last_error = Some(err);
                    continue;
                }
                return Err(err);
            }

            // Success - buffer and parse event stream
            let outcome = KiroRequestOutcome::Success;
            self.apply_request_outcome(&req.model, &outcome, Instant::now())
                .await;

            let bytes = resp
                .bytes()
                .await
                .map_err(|e| AppError::Upstream(format!("failed to read response body: {e}")))?;

            // Parse AWS Event Stream
            use std::io::Cursor;
            let cursor = Cursor::new(bytes.as_ref());
            let parser = EventStreamParser::new(cursor);
            let messages = parser
                .parse_all()
                .map_err(|e| AppError::Upstream(format!("failed to parse event stream: {e}")))?;

            return Ok(messages);
        }

        Err(last_error.unwrap_or_else(|| AppError::Upstream("all retries failed".into())))
    }
}

#[async_trait]
impl Provider for KiroProvider {
    fn name(&self) -> &str {
        "kiro"
    }

    fn client_id(&self) -> &str {
        &self.registry_client_id
    }

    async fn list_models(&self) -> AppResult<Vec<ModelInfo>> {
        let now = chrono::Utc::now().timestamp();

        if !self
            .model_registry
            .has_client(&self.registry_client_id)
            .await
        {
            return Ok(KIRO_MODEL_IDS
                .iter()
                .map(|kiro_model| ModelInfo {
                    id: (*kiro_model).to_string(),
                    object: "model".into(),
                    created: now,
                    owned_by: "kiro".into(),
                })
                .collect());
        }

        let mut available_models = Vec::new();

        for kiro_model in KIRO_MODEL_IDS {
            let is_available = self
                .model_registry
                .client_is_effectively_available(&self.registry_client_id, kiro_model)
                .await;

            if is_available {
                available_models.push(ModelInfo {
                    id: (*kiro_model).to_string(),
                    object: "model".into(),
                    created: now,
                    owned_by: "kiro".into(),
                });
            }
        }

        Ok(available_models)
    }

    async fn chat_completion(
        &self,
        req: &ChatCompletionRequest,
    ) -> AppResult<ChatCompletionResponse> {
        use crate::providers::kiro_translator::{
            aggregate_kiro_messages, build_openai_chat_completion_response,
        };

        let messages = self.execute_buffered_kiro_request(req).await?;
        let aggregate = aggregate_kiro_messages(&messages);
        let chat_id = format!("chatcmpl-{}", uuid::Uuid::new_v4());
        let created = chrono::Utc::now().timestamp();

        Ok(build_openai_chat_completion_response(
            aggregate, &chat_id, &req.model, created,
        ))
    }

    async fn chat_completion_stream(&self, req: &ChatCompletionRequest) -> AppResult<BoxStream> {
        let messages = self.execute_buffered_kiro_request(req).await?;

        let chat_id = format!("chatcmpl-{}", uuid::Uuid::new_v4());
        let created = chrono::Utc::now().timestamp();

        // Convert to SSE stream
        use crate::providers::kiro_translator::translate_kiro_event_to_openai_sse;
        use futures::stream;
        let sse_chunks: Vec<_> = messages
            .into_iter()
            .filter_map(|msg| {
                translate_kiro_event_to_openai_sse(
                    &msg.event_type,
                    &msg.payload,
                    &chat_id,
                    &req.model,
                    created,
                )
            })
            .map(Ok)
            .collect();

        let stream = stream::iter(sse_chunks);
        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Instant;

    use crate::auth::kiro_runtime::{
        CooldownManager, KiroRateLimiter, NoOpQuotaChecker, QuotaChecker, UsageCheckRequest,
    };
    use crate::auth::store::AuthStatus;
    use crate::providers::model_registry::ModelRegistry;
    use crate::proxy::KiroRuntimeState;
    use axum::body::Body;
    use axum::http::StatusCode as AxumStatusCode;
    use axum::routing::post;
    use axum::Router;
    use serde_json::json;
    use tokio::sync::oneshot;
    use tokio::sync::RwLock;

    struct FakeExhaustedChecker;

    #[async_trait::async_trait]
    impl QuotaChecker for FakeExhaustedChecker {
        async fn check_quota(
            &self,
            _request: &UsageCheckRequest,
        ) -> crate::auth::kiro_runtime::QuotaStatus {
            crate::auth::kiro_runtime::QuotaStatus::Exhausted {
                detail: "test exhausted".into(),
            }
        }
    }

    struct FakeAvailableChecker;

    #[async_trait::async_trait]
    impl QuotaChecker for FakeAvailableChecker {
        async fn check_quota(
            &self,
            _request: &UsageCheckRequest,
        ) -> crate::auth::kiro_runtime::QuotaStatus {
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

    #[test]
    fn format_upstream_error_body_preserves_response_text() {
        let body = format_upstream_error_body::<std::io::Error>(Ok("quota exceeded".to_string()));

        assert_eq!(body, "quota exceeded");
    }

    #[test]
    fn format_upstream_error_body_describes_read_failures() {
        let body = format_upstream_error_body::<std::io::Error>(Err(std::io::Error::other("boom")));

        assert!(body.contains("failed to read upstream error body"));
        assert!(body.contains("boom"));
    }

    #[test]
    fn cooldown_remaining_or_zero_defaults_to_zero() {
        assert_eq!(cooldown_remaining_or_zero(None), Duration::ZERO);
        assert_eq!(
            cooldown_remaining_or_zero(Some(Duration::from_secs(7))),
            Duration::from_secs(7)
        );
    }

    #[test]
    fn profile_arn_or_empty_defaults_when_missing() {
        let mut record = test_record();
        record.metadata.remove("profile_arn");

        assert!(profile_arn_or_empty(&record).is_empty());
    }

    #[test]
    fn generate_machine_id_prefers_refresh_token_fingerprint() {
        let token_data = KiroTokenData {
            access_token: "test-access-token".into(),
            refresh_token: "test-refresh-token".into(),
            profile_arn: "arn:aws:iam::123456789012:role/test".into(),
            expires_at: "2030-01-01T00:00:00Z".into(),
            auth_method: "builder-id".into(),
            provider: "AWS".into(),
            client_id: Some("test-client-id".into()),
            client_secret: Some("test-client-secret".into()),
            region: "us-east-1".into(),
            start_url: Some(BUILDER_ID_START_URL.into()),
            email: None,
        };

        assert_eq!(
            generate_machine_id(&token_data),
            Some(sha256_hex("KotlinNativeAPI/test-refresh-token"))
        );
    }

    #[test]
    fn build_endpoint_url_uses_native_generate_assistant_response_path() {
        let registry = Arc::new(ModelRegistry::new());
        let runtime = runtime_with_checker(Arc::new(NoOpQuotaChecker));
        let Ok(provider) =
            KiroProvider::new_with_runtime(test_record(), "kiro_0".to_string(), registry, runtime)
        else {
            panic!("provider should be constructed");
        };

        assert_eq!(
            provider.build_endpoint_url(),
            format!(
                "https://q.us-east-1.amazonaws.com{}",
                GENERATE_ASSISTANT_RESPONSE_PATH
            )
        );
    }

    #[test]
    fn build_kiro_headers_include_native_fingerprint_fields() {
        use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HOST, USER_AGENT};

        let registry = Arc::new(ModelRegistry::new());
        let runtime = runtime_with_checker(Arc::new(NoOpQuotaChecker));
        let Ok(provider) =
            KiroProvider::new_with_runtime(test_record(), "kiro_0".to_string(), registry, runtime)
        else {
            panic!("provider should be constructed");
        };
        let Some(machine_id) = generate_machine_id(&provider.token_data) else {
            panic!("machine_id should be generated");
        };
        let Ok(headers) = provider.build_kiro_headers("test-access-token") else {
            panic!("headers should be built");
        };

        assert_eq!(
            headers
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        assert_eq!(
            headers
                .get("x-amzn-kiro-agent-mode")
                .and_then(|value| value.to_str().ok()),
            Some("vibe")
        );
        assert_eq!(
            headers.get(HOST).and_then(|value| value.to_str().ok()),
            Some("q.us-east-1.amazonaws.com")
        );
        assert_eq!(
            headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer test-access-token")
        );
        assert_eq!(
            headers
                .get("amz-sdk-request")
                .and_then(|value| value.to_str().ok()),
            Some("attempt=1; max=3")
        );
        assert!(headers.contains_key("amz-sdk-invocation-id"));

        let Some(x_amz_user_agent) = headers
            .get("x-amz-user-agent")
            .and_then(|value| value.to_str().ok())
        else {
            panic!("x-amz-user-agent should be present");
        };
        assert!(
            x_amz_user_agent.starts_with(&format!("aws-sdk-js/1.0.27 KiroIDE-{}-", KIRO_VERSION))
        );
        assert!(x_amz_user_agent.ends_with(&machine_id));

        let Some(user_agent) = headers
            .get(USER_AGENT)
            .and_then(|value| value.to_str().ok())
        else {
            panic!("user-agent should be present");
        };
        assert!(user_agent.contains("aws-sdk-js/1.0.27"));
        assert!(user_agent.contains("api/codewhispererstreaming#1.0.27"));
        assert!(user_agent.contains(&format!("KiroIDE-{}-{}", KIRO_VERSION, machine_id)));
    }

    #[tokio::test]
    async fn apply_refreshed_token_keeps_in_memory_token_when_persist_fails() {
        let registry = Arc::new(ModelRegistry::new());
        let runtime = runtime_with_checker(Arc::new(NoOpQuotaChecker));
        let mut record = test_record();
        record.path =
            std::env::temp_dir().join(format!("kiro-missing-{}.json", uuid::Uuid::new_v4()));

        let provider =
            KiroProvider::new_with_runtime(record, "kiro_0".to_string(), registry, runtime)
                .unwrap();
        let mut state = TokenState::from_token_data(&provider.token_data).unwrap();
        let refreshed = KiroTokenData {
            access_token: "refreshed-access-token".into(),
            refresh_token: "refreshed-refresh-token".into(),
            profile_arn: state.profile_arn.clone(),
            expires_at: "2030-01-02T00:00:00Z".into(),
            auth_method: state.auth_method.clone(),
            provider: state.provider.clone(),
            client_id: state.client_id.clone(),
            client_secret: state.client_secret.clone(),
            region: state.region.clone(),
            start_url: state.start_url.clone(),
            email: state.email.clone(),
        };

        let token = provider
            .apply_refreshed_token(&mut state, refreshed)
            .await
            .unwrap();

        assert_eq!(token, "refreshed-access-token");
        assert_eq!(state.access_token, "refreshed-access-token");
        assert_eq!(state.refresh_token, "refreshed-refresh-token");
        assert!(state.last_refreshed_at.is_some());
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

    fn test_non_stream_request(model_id: &str) -> ChatCompletionRequest {
        let mut req = test_request(model_id);
        req.stream = Some(false);
        req
    }

    async fn register_kiro_test_model(
        registry: &Arc<ModelRegistry>,
        client_id: &str,
        model_id: &str,
    ) {
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
                    supported_endpoints: None,
                    thinking: None,
                    user_defined: false,
                }],
            )
            .await;
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
        bytes.extend_from_slice(&create_event_stream_message("messageStart", br#"{}"#));
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

    fn success_stream_with_usage_bytes() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&create_event_stream_message("messageStart", br#"{}"#));
        bytes.extend_from_slice(&create_event_stream_message(
            "assistantResponseEvent",
            br#"{"content":"hello world"}"#,
        ));
        bytes.extend_from_slice(&create_event_stream_message(
            "usageEvent",
            br#"{"inputTokens":10,"outputTokens":5}"#,
        ));
        bytes.extend_from_slice(&create_event_stream_message(
            "messageStop",
            br#"{"stopReason":"end_turn"}"#,
        ));
        bytes
    }

    async fn spawn_test_server(
        status: AxumStatusCode,
        body: Vec<u8>,
        content_type: &'static str,
    ) -> (String, oneshot::Sender<()>) {
        async fn conversation_handler(
            axum::extract::State(state): axum::extract::State<(
                AxumStatusCode,
                Vec<u8>,
                &'static str,
            )>,
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
                    supported_endpoints: None,
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
        runtime
            .rate_limiter
            .write()
            .await
            .mark_token_failed("kiro-test.json", Instant::now() - Duration::from_secs(31));

        let (endpoint, shutdown) = spawn_test_server(
            AxumStatusCode::OK,
            success_stream_bytes(),
            "application/vnd.amazon.eventstream",
        )
        .await;
        let mut provider = KiroProvider::new_with_runtime(
            test_record(),
            client_id.to_string(),
            registry.clone(),
            runtime.clone(),
        )
        .unwrap();
        provider.test_endpoint = Some(format!("{}/conversation", endpoint));

        let result = provider
            .chat_completion_stream(&test_request(model_id))
            .await;
        let _ = shutdown.send(());

        let _stream = result.unwrap();
        assert!(
            registry
                .client_is_effectively_available(client_id, model_id)
                .await
        );
        assert!(runtime
            .rate_limiter
            .read()
            .await
            .is_token_available("kiro-test.json", Instant::now()));
        assert!(!runtime.cooldown.read().await.is_in_cooldown(
            "kiro-test.json",
            model_id,
            Instant::now()
        ));
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
                    supported_endpoints: None,
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
        provider.test_endpoint = Some(format!("{}/conversation", endpoint));

        let result = provider
            .chat_completion_stream(&test_request(model_id))
            .await;
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("Expected error but got success"),
        };
        let _ = shutdown.send(());

        assert!(matches!(err, AppError::Upstream(message) if message.contains("429")));
        assert!(
            !registry
                .client_is_effectively_available(client_id, model_id)
                .await
        );
        assert!(runtime.cooldown.read().await.is_in_cooldown(
            "kiro-test.json",
            model_id,
            Instant::now()
        ));
        assert_eq!(
            runtime.cooldown.read().await.cooldown_reason(
                "kiro-test.json",
                model_id,
                Instant::now()
            ),
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
                    supported_endpoints: None,
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
        provider.test_endpoint = Some(format!("{}/conversation", endpoint));

        let result = provider
            .chat_completion_stream(&test_request(model_id))
            .await;
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("Expected error but got success"),
        };
        let _ = shutdown.send(());

        assert!(matches!(err, AppError::Upstream(message) if message.contains("403")));
        assert!(
            !registry
                .client_is_effectively_available(client_id, model_id)
                .await
        );
        assert!(runtime.cooldown.read().await.is_in_cooldown(
            "kiro-test.json",
            model_id,
            Instant::now()
        ));
        assert_eq!(
            runtime.cooldown.read().await.cooldown_reason(
                "kiro-test.json",
                model_id,
                Instant::now()
            ),
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

        let provider =
            KiroProvider::new_with_runtime(record, client_id.to_string(), registry, runtime)
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

        runtime
            .rate_limiter
            .write()
            .await
            .mark_token_failed(&auth_key, Instant::now() - Duration::from_millis(29_700));

        let provider =
            KiroProvider::new_with_runtime(record, client_id.to_string(), registry, runtime)
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
                    supported_endpoints: None,
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
        assert!(
            !registry
                .client_is_effectively_available(client_id, model_id)
                .await
        );
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
                    supported_endpoints: None,
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

        assert!(
            registry
                .client_is_effectively_available(client_id, model_id)
                .await
        );
    }

    #[tokio::test]
    async fn list_models_exposes_raw_kiro_catalog_before_registry_registration() {
        let registry = Arc::new(ModelRegistry::new());
        let provider = KiroProvider::new_with_runtime(
            test_record(),
            "kiro_0".to_string(),
            registry,
            runtime_with_checker(Arc::new(NoOpQuotaChecker)),
        )
        .unwrap();

        let models = provider.list_models().await.unwrap();
        let ids: std::collections::HashSet<String> = models.into_iter().map(|m| m.id).collect();

        assert!(ids.contains("kiro-claude-opus-4-6"));
        assert!(ids.contains("kiro-claude-sonnet-4-6"));
        assert!(ids.contains("kiro-claude-opus-4-5"));
        assert!(ids.contains("kiro-claude-sonnet-4-5"));
        assert!(ids.contains("kiro-claude-sonnet-4"));
        assert!(ids.contains("kiro-claude-haiku-4-5"));

        assert!(ids.contains("kiro-claude-opus-4-6-agentic"));
        assert!(ids.contains("kiro-claude-sonnet-4-6-agentic"));
        assert!(ids.contains("kiro-claude-opus-4-5-agentic"));
        assert!(ids.contains("kiro-claude-sonnet-4-5-agentic"));
        assert!(ids.contains("kiro-claude-sonnet-4-agentic"));
        assert!(ids.contains("kiro-claude-haiku-4-5-agentic"));

        assert!(ids.contains("kiro-deepseek-3-2"));
        assert!(ids.contains("kiro-minimax-m2-1"));
        assert!(ids.contains("kiro-qwen3-coder-next"));

        assert!(ids.contains("kiro-deepseek-3-2-agentic"));
        assert!(ids.contains("kiro-minimax-m2-1-agentic"));
        assert!(ids.contains("kiro-qwen3-coder-next-agentic"));

        assert_eq!(ids.len(), KIRO_MODEL_IDS.len());
        assert!(!ids.contains("claude-sonnet-4.5"));
        assert!(!ids.contains("claude-sonnet-4.5-thinking"));
        assert!(!ids.contains("deepseek-3.2"));
    }

    #[tokio::test]
    async fn chat_completion_non_stream_success() {
        let registry = Arc::new(ModelRegistry::new());
        let runtime = runtime_with_checker(Arc::new(NoOpQuotaChecker));
        let client_id = "kiro_0";
        let model_id = "claude-sonnet-4";

        register_kiro_test_model(&registry, client_id, model_id).await;

        let (endpoint, shutdown) = spawn_test_server(
            AxumStatusCode::OK,
            success_stream_bytes(),
            "application/vnd.amazon.eventstream",
        )
        .await;
        let mut provider = KiroProvider::new_with_runtime(
            test_record(),
            client_id.to_string(),
            registry.clone(),
            runtime.clone(),
        )
        .unwrap();
        provider.test_endpoint = Some(format!("{}/conversation", endpoint));

        let result = provider
            .chat_completion(&test_non_stream_request(model_id))
            .await;
        let _ = shutdown.send(());

        let response = result.unwrap();
        assert_eq!(response.object, "chat.completion");
        assert_eq!(response.model, model_id);
        assert_eq!(response.choices.len(), 1);

        let choice = &response.choices[0];
        let message = choice.message.as_ref().unwrap();
        assert_eq!(message.role, "assistant");
        if let crate::models::MessageContent::Text(text) = &message.content {
            assert!(text.contains("hello"));
        } else {
            panic!("Expected text content");
        }
        assert_eq!(choice.finish_reason, Some("stop".to_string()));
    }

    #[tokio::test]
    async fn chat_completion_non_stream_aggregates_usage() {
        let registry = Arc::new(ModelRegistry::new());
        let runtime = runtime_with_checker(Arc::new(NoOpQuotaChecker));
        let client_id = "kiro_0";
        let model_id = "claude-sonnet-4";

        register_kiro_test_model(&registry, client_id, model_id).await;

        let (endpoint, shutdown) = spawn_test_server(
            AxumStatusCode::OK,
            success_stream_with_usage_bytes(),
            "application/vnd.amazon.eventstream",
        )
        .await;
        let mut provider = KiroProvider::new_with_runtime(
            test_record(),
            client_id.to_string(),
            registry.clone(),
            runtime.clone(),
        )
        .unwrap();
        provider.test_endpoint = Some(format!("{}/conversation", endpoint));

        let result = provider
            .chat_completion(&test_non_stream_request(model_id))
            .await;
        let _ = shutdown.send(());

        let response = result.unwrap();
        assert!(response.usage.is_some());
        let usage = response.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
        assert_eq!(usage.total_tokens, 15);
    }

    #[tokio::test]
    async fn chat_completion_non_stream_tool_calls() {
        let registry = Arc::new(ModelRegistry::new());
        let runtime = runtime_with_checker(Arc::new(NoOpQuotaChecker));
        let client_id = "kiro_0";
        let model_id = "claude-sonnet-4";

        register_kiro_test_model(&registry, client_id, model_id).await;

        let mut tool_stream_bytes = Vec::new();
        tool_stream_bytes.extend_from_slice(&create_event_stream_message("messageStart", br#"{}"#));
        tool_stream_bytes.extend_from_slice(&create_event_stream_message(
            "toolUseEvent",
            br#"{"toolUseId":"tool_123","name":"get_weather","input":{"location":"NYC"}}"#,
        ));
        tool_stream_bytes.extend_from_slice(&create_event_stream_message(
            "messageStop",
            br#"{"stopReason":"tool_use"}"#,
        ));

        let (endpoint, shutdown) = spawn_test_server(
            AxumStatusCode::OK,
            tool_stream_bytes,
            "application/vnd.amazon.eventstream",
        )
        .await;
        let mut provider = KiroProvider::new_with_runtime(
            test_record(),
            client_id.to_string(),
            registry.clone(),
            runtime.clone(),
        )
        .unwrap();
        provider.test_endpoint = Some(format!("{}/conversation", endpoint));

        let result = provider
            .chat_completion(&test_non_stream_request(model_id))
            .await;
        let _ = shutdown.send(());

        let response = result.unwrap();
        assert_eq!(response.choices.len(), 1);

        let choice = &response.choices[0];
        let message = choice.message.as_ref().unwrap();
        assert_eq!(message.role, "assistant");
        assert!(message.tool_calls.is_some());
        let tool_calls = message.tool_calls.as_ref().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0]["id"], "tool_123");
        assert_eq!(tool_calls[0]["function"]["name"], "get_weather");
        assert_eq!(choice.finish_reason, Some("tool_calls".to_string()));

        // When there are tool calls but no text content, content should be empty text
        if let crate::models::MessageContent::Text(text) = &message.content {
            assert_eq!(text, "");
        } else {
            panic!("Expected text content");
        }
    }
}
