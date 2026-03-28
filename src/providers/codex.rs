//! Codex provider runtime.

use async_trait::async_trait;
use futures::StreamExt;
use serde_json::{json, Value};

use crate::auth::store::AuthRecord;
use crate::error::{AppError, AppResult};
use crate::models::{ChatCompletionRequest, ChatCompletionResponse, ModelInfo, Usage};
use crate::providers::static_models;
use crate::providers::{BoxStream, Provider};

pub struct CodexProvider {
    record: AuthRecord,
}

impl CodexProvider {
    pub fn new(record: AuthRecord) -> AppResult<Self> {
        if record.access_token().is_none() {
            return Err(AppError::Auth("codex account missing access_token".into()));
        }

        Ok(Self { record })
    }

    fn access_token(&self) -> AppResult<&str> {
        self.record
            .access_token()
            .ok_or_else(|| AppError::Auth("codex account missing access_token".into()))
    }

    fn base_url(&self) -> String {
        self.record
            .metadata
            .get("base_url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("https://api.openai.com/v1")
            .trim_end_matches('/')
            .to_string()
    }

    fn map_upstream_error(status: reqwest::StatusCode, body: String) -> AppError {
        match status {
            reqwest::StatusCode::UNAUTHORIZED => AppError::Auth(format!(
                "codex upstream unauthorized ({}): {}",
                status,
                body.trim()
            )),
            reqwest::StatusCode::TOO_MANY_REQUESTS => AppError::QuotaExceeded(format!(
                "codex upstream rate limited ({}): {}",
                status,
                body.trim()
            )),
            _ => AppError::Upstream(format!(
                "codex upstream error ({}): {}",
                status,
                body.trim()
            )),
        }
    }

    fn decode_chat_completion_response(body: Value) -> AppResult<ChatCompletionResponse> {
        let mut response: ChatCompletionResponse = serde_json::from_value(body.clone())
            .map_err(|e| AppError::Upstream(format!("failed to decode codex response: {e}")))?;

        if response.usage.is_none() {
            response.usage = body.get("usage").cloned().and_then(parse_usage);
        }

        Ok(response)
    }
}

pub fn normalize_codex_model(model: &str) -> String {
    model
        .trim()
        .strip_suffix("-thinking")
        .unwrap_or_else(|| model.trim())
        .to_string()
}

pub fn prepare_codex_request(mut request: ChatCompletionRequest) -> ChatCompletionRequest {
    request.model = normalize_codex_model(&request.model);

    request.extra.remove("previous_response_id");
    request.extra.remove("prompt_cache_retention");
    request.extra.remove("safety_identifier");
    request.extra.remove("selected_auth_id");
    request.extra.remove("execution_session_id");

    request
        .extra
        .entry("instructions".to_string())
        .or_insert_with(|| json!(""));

    request
}

pub fn is_non_retryable_refresh_error(message: &str) -> bool {
    crate::auth::codex_runtime::is_non_retryable_refresh_error(message)
}

pub fn parse_usage(usage: serde_json::Value) -> Option<Usage> {
    let prompt_tokens = usage
        .get("prompt_tokens")
        .and_then(|value| value.as_u64())
        .or_else(|| usage.get("input_tokens").and_then(|value| value.as_u64()))?;

    let completion_tokens = usage
        .get("completion_tokens")
        .and_then(|value| value.as_u64())
        .or_else(|| usage.get("output_tokens").and_then(|value| value.as_u64()))?;

    let total_tokens = usage
        .get("total_tokens")
        .and_then(|value| value.as_u64())
        .unwrap_or(prompt_tokens + completion_tokens);

    Some(Usage {
        prompt_tokens: prompt_tokens as u32,
        completion_tokens: completion_tokens as u32,
        total_tokens: total_tokens as u32,
    })
}

#[async_trait]
impl Provider for CodexProvider {
    fn name(&self) -> &str {
        "codex"
    }

    async fn list_models(&self) -> AppResult<Vec<ModelInfo>> {
        let now = chrono::Utc::now().timestamp();

        Ok(static_models::openai_models()
            .into_iter()
            .map(|model| ModelInfo {
                id: model.id,
                object: "model".to_string(),
                created: if model.created > 0 {
                    model.created
                } else {
                    now
                },
                owned_by: model.owned_by,
            })
            .collect())
    }

    async fn chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> AppResult<ChatCompletionResponse> {
        let access_token = self.access_token()?;
        let prepared_request = prepare_codex_request(request.clone());
        let endpoint = format!("{}/chat/completions", self.base_url());

        let response = reqwest::Client::new()
            .post(endpoint)
            .bearer_auth(access_token)
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .json(&prepared_request)
            .send()
            .await
            .map_err(|e| AppError::Upstream(format!("codex request failed: {e}")))?;

        let status = response.status();
        let body_text = response
            .text()
            .await
            .map_err(|e| AppError::Upstream(format!("failed reading codex response body: {e}")))?;

        if !status.is_success() {
            return Err(Self::map_upstream_error(status, body_text));
        }

        let body: Value = serde_json::from_str(&body_text)
            .map_err(|e| AppError::Upstream(format!("failed to parse codex response JSON: {e}")))?;

        Self::decode_chat_completion_response(body)
    }

    async fn chat_completion_stream(
        &self,
        request: &ChatCompletionRequest,
    ) -> AppResult<BoxStream> {
        let access_token = self.access_token()?;
        let prepared_request = prepare_codex_request(request.clone());
        let endpoint = format!("{}/chat/completions/stream", self.base_url());

        let response = reqwest::Client::new()
            .post(endpoint)
            .bearer_auth(access_token)
            .header("content-type", "application/json")
            .header("accept", "text/event-stream")
            .json(&prepared_request)
            .send()
            .await
            .map_err(|e| AppError::Upstream(format!("codex stream request failed: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.map_err(|e| {
                AppError::Upstream(format!("failed reading codex stream body: {e}"))
            })?;
            return Err(Self::map_upstream_error(status, body));
        }

        let stream = response.bytes_stream().map(|item| {
            item.map_err(|e| AppError::Upstream(format!("codex stream read error: {e}")))
        });

        Ok(Box::pin(stream))
    }
}
