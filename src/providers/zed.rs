//! Zed Cloud provider runtime — endpoint helpers, provider scanning, token caching,
//! model caching, and request execution.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use serde::Deserialize;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::auth::store::{AuthRecord, FileTokenStore};
use crate::auth::zed::parse_zed_credential;
use crate::error::{AppError, AppResult};
use crate::models::{ChatCompletionRequest, ChatCompletionResponse, ModelInfo};
use crate::providers::zed_request::translate_to_zed_request;
use crate::providers::zed_response::{format_sse_event, parse_zed_response_with_model};
use crate::providers::{BoxStream, Provider};

/// Token refresh buffer in seconds (refresh when less than this remains)
const TOKEN_REFRESH_BUFFER_SECS: u64 = 60;

/// Fixed TTL for Zed tokens (1 hour)
const ZED_TOKEN_TTL_SECS: u64 = 3600;

/// Zed API version header value
const ZED_VERSION: &str = "0.222.4";

/// System ID for token refresh requests
const ZED_SYSTEM_ID: &str = "6b87ab66-af2c-49c7-b986-ef4c27c9e1fb";

/// Client for Zed Cloud API endpoints.
#[derive(Debug)]
pub struct ZedClient;

impl ZedClient {
    /// Returns the token endpoint URL.
    pub fn token_endpoint(&self) -> &'static str {
        "https://cloud.zed.dev/client/llm_tokens"
    }

    /// Returns the completions endpoint URL.
    pub fn completions_endpoint(&self) -> &'static str {
        "https://cloud.zed.dev/completions"
    }

    /// Returns the models endpoint URL.
    pub fn models_endpoint(&self) -> &'static str {
        "https://cloud.zed.dev/models"
    }

    /// Returns the users/me endpoint URL.
    pub fn users_me_endpoint(&self) -> &'static str {
        "https://cloud.zed.dev/client/users/me"
    }

    /// Checks if a response indicates a stale token.
    /// Returns true only for 401 status with x-zed-expired-token or x-zed-outdated-token header.
    pub fn is_stale_token_response(
        &self,
        status_code: u16,
        headers: &reqwest::header::HeaderMap,
    ) -> bool {
        let headers = headers
            .keys()
            .map(|name| (name.as_str(), ""))
            .collect::<Vec<_>>();
        is_stale_token_response(status_code, &headers)
    }
}

/// Standalone helper to check if a response indicates a stale token.
/// Returns true only for 401 status with x-zed-expired-token or x-zed-outdated-token header.
pub fn is_stale_token_response(status: u16, headers: &[(&str, &str)]) -> bool {
    if status != 401 {
        return false;
    }

    headers.iter().any(|(k, _)| {
        k.eq_ignore_ascii_case("x-zed-expired-token")
            || k.eq_ignore_ascii_case("x-zed-outdated-token")
    })
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

fn should_refresh_token(cache: Option<&TokenCache>) -> bool {
    cache.is_none_or(TokenCache::is_expired)
}

fn map_cached_model_ids(model_ids: &[String]) -> Vec<ModelInfo> {
    model_ids
        .iter()
        .map(|id| ModelInfo {
            id: id.clone(),
            object: "model".to_string(),
            created: 0,
            owned_by: "zed".to_string(),
        })
        .collect()
}

fn read_cached_models(models_cache: Option<&Vec<String>>) -> AppResult<Vec<ModelInfo>> {
    models_cache
        .map(|model_ids| map_cached_model_ids(model_ids))
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("zed models cache missing after refresh"))
        })
}

/// Cached Zed API token with expiry tracking.
#[derive(Debug, Clone)]
pub struct TokenCache {
    pub token: String,
    pub expires_at: SystemTime,
}

impl TokenCache {
    /// Create a new token cache with the given TTL in seconds.
    pub fn new(token: String, ttl_seconds: u64) -> Self {
        let expires_at = SystemTime::now() + Duration::from_secs(ttl_seconds);
        Self { token, expires_at }
    }

    /// Check if the token is expired or within the refresh buffer.
    pub fn is_expired(&self) -> bool {
        match self.expires_at.duration_since(SystemTime::now()) {
            Ok(remaining) => remaining.as_secs() <= TOKEN_REFRESH_BUFFER_SECS,
            Err(_) => true,
        }
    }

    /// Get the token string.
    pub fn token(&self) -> &str {
        &self.token
    }
}

/// Zed Cloud provider instance.
///
/// Each instance corresponds to one Zed auth account.
#[derive(Debug)]
pub struct ZedProvider {
    record_id: String,
    user_id: String,
    credential_json: String,
    pub token_cache: Arc<Mutex<Option<TokenCache>>>,
    pub models_cache: Arc<Mutex<Option<Vec<String>>>>,
    client: ZedClient,
    http_client: reqwest::Client,
}

impl ZedProvider {
    /// Create a new Zed provider from an auth record.
    pub fn new(record: AuthRecord) -> AppResult<Self> {
        let metadata_value = serde_json::to_value(&record.metadata)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("serialize metadata: {e}")))?;
        let (user_id, credential_json) = parse_zed_credential(&metadata_value)
            .map_err(|e| AppError::Auth(format!("parse zed credential: {e}")))?;

        Ok(Self {
            record_id: record.id,
            user_id,
            credential_json,
            token_cache: Arc::new(Mutex::new(None)),
            models_cache: Arc::new(Mutex::new(None)),
            client: ZedClient,
            http_client: reqwest::Client::new(),
        })
    }

    /// Get the user ID for this provider.
    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    /// Get the credential JSON for this provider.
    pub fn credential_json(&self) -> &str {
        &self.credential_json
    }

    /// Build HTTP headers for Zed API requests.
    pub async fn build_headers(&self) -> AppResult<reqwest::header::HeaderMap> {
        let cache = self.token_cache.lock().await;
        let token = cache
            .as_ref()
            .ok_or_else(|| AppError::Auth("no cached token".into()))?
            .token();

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "authorization",
            format!("Bearer {}", token)
                .parse()
                .map_err(|e| AppError::Internal(anyhow::anyhow!("invalid auth header: {e}")))?,
        );
        headers.insert(
            "content-type",
            "application/json"
                .parse()
                .map_err(|e| AppError::Internal(anyhow::anyhow!("invalid content-type: {e}")))?,
        );
        headers.insert(
            "x-zed-version",
            ZED_VERSION
                .parse()
                .map_err(|e| AppError::Internal(anyhow::anyhow!("invalid version header: {e}")))?,
        );

        Ok(headers)
    }

    /// Refresh the API token from Zed Cloud.
    pub async fn refresh_token(&self) -> AppResult<()> {
        let needs_refresh = {
            let cache = self.token_cache.lock().await;
            should_refresh_token(cache.as_ref())
        };

        if !needs_refresh {
            debug!(
                "skipping zed token refresh for user {} because cache is already valid",
                self.user_id
            );
            return Ok(());
        }

        debug!("refreshing zed token for user {}", self.user_id);

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "authorization",
            format!("{} {}", self.user_id, self.credential_json)
                .parse()
                .map_err(|e| AppError::Internal(anyhow::anyhow!("invalid auth header: {e}")))?,
        );
        headers.insert(
            "x-zed-system-id",
            ZED_SYSTEM_ID.parse().map_err(|e| {
                AppError::Internal(anyhow::anyhow!("invalid system-id header: {e}"))
            })?,
        );
        headers.insert(
            "content-type",
            "application/json"
                .parse()
                .map_err(|e| AppError::Internal(anyhow::anyhow!("invalid content-type: {e}")))?,
        );

        let response = self
            .http_client
            .post(self.client.token_endpoint())
            .headers(headers)
            .body("")
            .send()
            .await
            .map_err(|e| AppError::Upstream(format!("token refresh request failed: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = format_non_success_response_body(response.text().await);
            return Err(AppError::Upstream(format!(
                "token refresh failed: {} - {}",
                status, body
            )));
        }

        #[derive(Deserialize)]
        struct TokenResponse {
            token: String,
        }

        let token_resp: TokenResponse = response
            .json()
            .await
            .map_err(|e| AppError::Upstream(format!("parse token response: {e}")))?;

        if token_resp.token.is_empty() {
            return Err(AppError::Upstream(
                "token refresh returned empty token".into(),
            ));
        }

        let mut cache = self.token_cache.lock().await;
        *cache = Some(TokenCache::new(token_resp.token, ZED_TOKEN_TTL_SECS));

        debug!("zed token refreshed for user {}", self.user_id);
        Ok(())
    }

    /// Refresh the models list from Zed Cloud.
    pub async fn refresh_models(&self) -> AppResult<()> {
        debug!("refreshing zed models for user {}", self.user_id);

        #[derive(Deserialize)]
        struct ModelItem {
            id: String,
        }

        #[derive(Deserialize)]
        struct ModelsResponse {
            models: Vec<ModelItem>,
        }

        for attempt in 0..2 {
            let headers = self.build_headers().await?;

            let response = self
                .http_client
                .get(self.client.models_endpoint())
                .headers(headers)
                .send()
                .await
                .map_err(|e| AppError::Upstream(format!("models fetch failed: {e}")))?;

            if !response.status().is_success() {
                let status = response.status();
                let is_stale = self
                    .client
                    .is_stale_token_response(status.as_u16(), response.headers());

                if is_stale && attempt == 0 {
                    debug!(
                        "stale zed token detected while fetching models for user {}; refreshing and retrying",
                        self.user_id
                    );
                    self.force_refresh_token().await?;
                    continue;
                }

                let body = format_non_success_response_body(response.text().await);
                return Err(AppError::Upstream(format!(
                    "models fetch failed: {} - {}",
                    status, body
                )));
            }

            let models_resp: ModelsResponse = response
                .json()
                .await
                .map_err(|e| AppError::Upstream(format!("parse models response: {e}")))?;

            let model_ids: Vec<String> = models_resp.models.into_iter().map(|m| m.id).collect();

            let mut cache = self.models_cache.lock().await;
            *cache = Some(model_ids);

            debug!("zed models refreshed for user {}", self.user_id);
            return Ok(());
        }

        Err(AppError::Upstream(
            "models fetch failed after stale-token retry".into(),
        ))
    }

    async fn force_refresh_token(&self) -> AppResult<()> {
        {
            let mut cache = self.token_cache.lock().await;
            *cache = None;
        }
        self.refresh_token().await
    }

    /// Ensure we have a valid token, refreshing if needed.
    async fn ensure_token(&self) -> AppResult<()> {
        let needs_refresh = {
            let cache = self.token_cache.lock().await;
            should_refresh_token(cache.as_ref())
        };

        if !needs_refresh {
            return Ok(());
        }

        self.refresh_token().await?;

        Ok(())
    }
}

#[async_trait]
impl Provider for ZedProvider {
    fn name(&self) -> &str {
        "zed"
    }

    fn client_id(&self) -> &str {
        &self.record_id
    }

    async fn list_models(&self) -> AppResult<Vec<ModelInfo>> {
        self.ensure_token().await?;

        {
            let cache = self.models_cache.lock().await;
            if let Some(models) = cache.as_ref() {
                return Ok(map_cached_model_ids(models));
            }
        }

        self.refresh_models().await?;

        let cache = self.models_cache.lock().await;
        read_cached_models(cache.as_ref())
    }

    async fn chat_completion(
        &self,
        req: &ChatCompletionRequest,
    ) -> AppResult<ChatCompletionResponse> {
        self.ensure_token().await?;

        let req_json = serde_json::to_value(req)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("serialize request: {e}")))?;
        let zed_req = translate_to_zed_request(&req_json)
            .map_err(|e| AppError::BadRequest(format!("translate request: {e}")))?;

        for attempt in 0..2 {
            let headers = self.build_headers().await?;

            let response = self
                .http_client
                .post(self.client.completions_endpoint())
                .headers(headers)
                .json(&zed_req)
                .send()
                .await
                .map_err(|e| AppError::Upstream(format!("completions request failed: {e}")))?;

            if !response.status().is_success() {
                let status = response.status();
                let is_stale = self
                    .client
                    .is_stale_token_response(status.as_u16(), response.headers());

                if is_stale && attempt == 0 {
                    debug!(
                        "stale zed token detected for completion request user {}; refreshing and retrying",
                        self.user_id
                    );
                    self.force_refresh_token().await?;
                    continue;
                }

                let body = format_non_success_response_body(response.text().await);
                return Err(AppError::Upstream(format!(
                    "completions failed: {} - {}",
                    status, body
                )));
            }

            let zed_resp_text = response
                .text()
                .await
                .map_err(|e| AppError::Upstream(format!("read completions response: {e}")))?;
            let zed_resp: serde_json::Value = match serde_json::from_str(&zed_resp_text) {
                Ok(value) => value,
                Err(_) => serde_json::Value::String(zed_resp_text),
            };

            let validated = parse_zed_response_with_model(&zed_resp, Some(&req.model))
                .map_err(|e| AppError::Upstream(format!("validate response: {e}")))?;

            return serde_json::from_value(validated)
                .map_err(|e| AppError::Internal(anyhow::anyhow!("deserialize response: {e}")));
        }

        Err(AppError::Upstream(
            "completions failed after stale-token retry".into(),
        ))
    }

    async fn chat_completion_stream(&self, req: &ChatCompletionRequest) -> AppResult<BoxStream> {
        self.ensure_token().await?;

        let mut streaming_req = req.clone();
        streaming_req.stream = Some(true);

        let req_json = serde_json::to_value(&streaming_req)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("serialize request: {e}")))?;
        let zed_req = translate_to_zed_request(&req_json)
            .map_err(|e| AppError::BadRequest(format!("translate request: {e}")))?;

        for attempt in 0..2 {
            let headers = self.build_headers().await?;

            let response = self
                .http_client
                .post(self.client.completions_endpoint())
                .headers(headers)
                .json(&zed_req)
                .send()
                .await
                .map_err(|e| AppError::Upstream(format!("streaming request failed: {e}")))?;

            if !response.status().is_success() {
                let status = response.status();
                let is_stale = self
                    .client
                    .is_stale_token_response(status.as_u16(), response.headers());

                if is_stale && attempt == 0 {
                    debug!(
                        "stale zed token detected for streaming request user {}; refreshing and retrying",
                        self.user_id
                    );
                    self.force_refresh_token().await?;
                    continue;
                }

                let body = format_non_success_response_body(response.text().await);
                return Err(AppError::Upstream(format!(
                    "streaming failed: {} - {}",
                    status, body
                )));
            }

            let upstream = response.bytes_stream();
            let stream = async_stream::try_stream! {
                let mut buffer = String::new();
                futures::pin_mut!(upstream);

                while let Some(chunk_result) = upstream.next().await {
                    let chunk = chunk_result
                        .map_err(|e| AppError::Upstream(format!("stream error: {e}")))?;

                    buffer.push_str(&String::from_utf8_lossy(&chunk));

                    while let Some(newline_pos) = buffer.find('\n') {
                        let line = buffer[..newline_pos].trim_end_matches('\r').to_string();
                        buffer = buffer[newline_pos + 1..].to_string();

                        if line.trim().is_empty() {
                            continue;
                        }

                        if serde_json::from_str::<serde_json::Value>(&line).is_err() {
                            continue;
                        }

                        yield Bytes::from(format_sse_event(&line));
                    }
                }

                let trailing = buffer.trim();
                if !trailing.is_empty() && serde_json::from_str::<serde_json::Value>(trailing).is_ok() {
                    yield Bytes::from(format_sse_event(trailing));
                }
            };

            return Ok(Box::pin(stream));
        }

        Err(AppError::Upstream(
            "streaming failed after stale-token retry".into(),
        ))
    }
}

/// Scan the auth store for Zed accounts and create provider instances.
///
/// Returns a list of (filename, provider) tuples for each valid Zed auth file.
/// Invalid auth files are logged and skipped.
pub async fn scan_zed_providers(
    store: &FileTokenStore,
) -> AppResult<Vec<(String, Arc<ZedProvider>)>> {
    let records = store.list().await?;
    let mut providers = Vec::new();

    for record in records {
        if record.provider != "zed" {
            continue;
        }

        match ZedProvider::new(record.clone()) {
            Ok(provider) => {
                let filename = record
                    .path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&record.id)
                    .to_string();
                providers.push((filename, Arc::new(provider)));
            }
            Err(e) => {
                warn!("skipping zed account {} ({}): {e}", record.label, record.id);
            }
        }
    }

    Ok(providers)
}

#[cfg(test)]
mod tests {
    use super::{
        format_non_success_response_body, map_cached_model_ids, read_cached_models,
        should_refresh_token, TokenCache, ZedProvider,
    };
    use crate::auth::store::{AuthRecord, AuthStatus};
    use crate::error::AppError;
    use chrono::Utc;
    use serde_json::json;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    fn make_zed_record(user_id: &str, credential_json: &str) -> AuthRecord {
        let mut metadata = HashMap::new();
        metadata.insert("user_id".to_string(), json!(user_id));
        metadata.insert("credential_json".to_string(), json!(credential_json));

        AuthRecord {
            id: "test.json".into(),
            provider: "zed".into(),
            provider_key: "zed".into(),
            label: user_id.to_string(),
            disabled: false,
            status: AuthStatus::Active,
            status_message: None,
            last_refreshed_at: None,
            path: PathBuf::from("test.json"),
            metadata,
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn format_non_success_response_body_returns_body_text_on_success() {
        let formatted = format_non_success_response_body::<std::io::Error>(Ok("zed body".into()));

        assert_eq!(formatted, "zed body");
    }

    #[test]
    fn format_non_success_response_body_returns_explicit_fallback_on_read_error() {
        let formatted = format_non_success_response_body(Err(std::io::Error::other("boom")));

        assert_eq!(formatted, "<failed to read response body: boom>");
    }

    #[test]
    fn map_cached_model_ids_preserves_ids_and_sets_zed_metadata() {
        let models =
            map_cached_model_ids(&["claude-3.7-sonnet".to_string(), "gpt-4.1".to_string()]);

        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "claude-3.7-sonnet");
        assert_eq!(models[0].object, "model");
        assert_eq!(models[0].created, 0);
        assert_eq!(models[0].owned_by, "zed");
        assert_eq!(models[1].id, "gpt-4.1");
        assert_eq!(models[1].object, "model");
        assert_eq!(models[1].created, 0);
        assert_eq!(models[1].owned_by, "zed");
    }

    #[test]
    fn read_cached_models_returns_internal_error_when_cache_is_missing() {
        let error = read_cached_models(None).unwrap_err();

        assert!(matches!(error, AppError::Internal(_)));
        assert!(error
            .to_string()
            .contains("models cache missing after refresh"));
    }

    #[test]
    fn should_refresh_token_returns_false_for_valid_cached_token() {
        let cache = TokenCache::new("test-token".to_string(), 120);

        assert!(!should_refresh_token(Some(&cache)));
    }

    #[test]
    fn should_refresh_token_returns_true_for_empty_cache() {
        assert!(should_refresh_token(None));
    }

    #[test]
    fn should_refresh_token_returns_true_for_expired_cached_token() {
        let cache = TokenCache {
            token: "test-token".to_string(),
            expires_at: SystemTime::now() - Duration::from_secs(1),
        };

        assert!(should_refresh_token(Some(&cache)));
    }

    #[tokio::test]
    async fn ensure_token_skips_refresh_when_valid_cache_appears_before_dispatch() {
        let record = make_zed_record("user123", "bad\ncredential");
        let provider = ZedProvider::new(record).unwrap();

        {
            let mut cache = provider.token_cache.lock().await;
            *cache = Some(TokenCache {
                token: "stale-token".to_string(),
                expires_at: SystemTime::now() - Duration::from_secs(1),
            });
        }

        {
            let mut cache = provider.token_cache.lock().await;
            *cache = Some(TokenCache::new("fresh-token".to_string(), 120));
        }

        provider.refresh_token().await.unwrap();

        let cache = provider.token_cache.lock().await;
        assert_eq!(cache.as_ref().unwrap().token(), "fresh-token");
    }
}
