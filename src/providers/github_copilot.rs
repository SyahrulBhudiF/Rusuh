use std::time::Duration;

use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use futures::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error};
use url::Url;

use crate::auth::github_copilot_runtime::{
    exchange_github_token_for_copilot_token_with_url, list_models as list_live_models,
    token_is_still_valid_until,
};
use crate::auth::store::AuthRecord;
use crate::error::{AppError, AppResult};
use crate::models::{
    ChatCompletionRequest, ChatCompletionResponse, ChatMessage, Choice, MessageContent,
    ModelInfo, Usage,
};
use crate::providers::static_models;
use crate::providers::{BoxStream, Provider};

const DEFAULT_API_BASE_URL: &str = "https://api.githubcopilot.com";
const DEFAULT_TOKEN_URL: &str = "https://api.github.com/copilot_internal/v2/token";
const USER_AGENT: &str = "GitHubCopilotChat/0.26.7";
const EDITOR_VERSION: &str = "vscode/1.99.0";
const EDITOR_PLUGIN_VERSION: &str = "copilot-chat/0.26.7";
const OPENAI_INTENT: &str = "conversation-panel";
const COPILOT_INTEGRATION_ID: &str = "vscode-chat";
const X_GITHUB_API_VERSION: &str = "2025-04-01";
const X_INITIATOR: &str = "user";

struct CachedApiToken {
    token: String,
    expires_at: chrono::DateTime<Utc>,
}

pub struct GithubCopilotProvider {
    record: AuthRecord,
    client: Client,
    stream_client: Client,
    cached_token: RwLock<Option<CachedApiToken>>,
    refresh_mutex: Mutex<()>,
}

impl GithubCopilotProvider {
    pub fn new(record: AuthRecord) -> AppResult<Self> {
        if record.access_token().is_none() {
            return Err(AppError::Auth(
                "github copilot account missing github oauth token".into(),
            ));
        }

        Ok(Self {
            record,
            client: build_client(Duration::from_secs(30))?,
            stream_client: build_stream_client(Duration::from_secs(120))?,
            cached_token: RwLock::new(None),
            refresh_mutex: Mutex::new(()),
        })
    }

    fn github_oauth_token(&self) -> AppResult<&str> {
        self.record.access_token().ok_or_else(|| {
            AppError::Auth("github copilot account missing github oauth token".into())
        })
    }

    fn api_base_url(&self) -> String {
        let candidate = self.record
            .metadata
            .get("copilot_api_url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());

        if let Some(url_str) = candidate {
            if let Ok(parsed) = Url::parse(url_str) {
                if parsed.scheme() == "https" {
                    if let Some(host) = parsed.host_str() {
                        const ALLOWED_API_HOSTS: &[&str] = &["api.githubcopilot.com"];
                        if ALLOWED_API_HOSTS.contains(&host) {
                            return url_str.trim_end_matches('/').to_string();
                        }
                    }
                }
            }
        }

        DEFAULT_API_BASE_URL.trim_end_matches('/').to_string()
    }

    fn token_url(&self) -> String {
        let candidate = self.record
            .metadata
            .get("copilot_token_url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());

        if let Some(url_str) = candidate {
            if let Ok(parsed) = Url::parse(url_str) {
                if parsed.scheme() == "https" {
                    if let Some(host) = parsed.host_str() {
                        const ALLOWED_TOKEN_HOSTS: &[&str] = &["api.github.com"];
                        if ALLOWED_TOKEN_HOSTS.contains(&host) {
                            return url_str.to_string();
                        }
                    }
                }
            }
        }

        DEFAULT_TOKEN_URL.to_string()
    }

    async fn copilot_api_token(&self) -> AppResult<String> {
        {
            let cached = self.cached_token.read().await;
            if let Some(cached) = cached.as_ref() {
                if token_is_still_valid_until(cached.expires_at, Utc::now()) {
                    return Ok(cached.token.clone());
                }
            }
        }

        let _guard = self.refresh_mutex.lock().await;

        {
            let cached = self.cached_token.read().await;
            if let Some(cached) = cached.as_ref() {
                if token_is_still_valid_until(cached.expires_at, Utc::now()) {
                    return Ok(cached.token.clone());
                }
            }
        }

        let exchanged = exchange_github_token_for_copilot_token_with_url(
            &self.client,
            self.github_oauth_token()?,
            &self.token_url(),
        )
        .await?;

        let expires_at = Utc
            .timestamp_opt(exchanged.expires_at, 0)
            .single()
            .ok_or_else(|| AppError::Auth("invalid copilot token expiry".into()))?;

        let token = exchanged.token;
        *self.cached_token.write().await = Some(CachedApiToken {
            token: token.clone(),
            expires_at,
        });
        Ok(token)
    }

    pub async fn live_models(&self) -> AppResult<Vec<ModelInfo>> {
        let token = self.copilot_api_token().await?;
        let models = list_live_models(&self.client, &self.api_base_url(), &token).await?;
        let now = Utc::now().timestamp();
        Ok(models
            .into_iter()
            .map(|model| ModelInfo {
                id: model.id,
                object: if model.object.is_empty() {
                    "model".to_string()
                } else {
                    model.object
                },
                created: if model.created > 0 { model.created } else { now },
                owned_by: if model.owned_by.is_empty() {
                    "github-copilot".to_string()
                } else {
                    model.owned_by
                },
            })
            .collect())
    }

    fn static_models(&self) -> Vec<ModelInfo> {
        let now = Utc::now().timestamp();
        static_models::github_copilot_models()
            .into_iter()
            .map(|model| ModelInfo {
                id: model.id,
                object: "model".to_string(),
                created: if model.created > 0 { model.created } else { now },
                owned_by: model.owned_by,
            })
            .collect()
    }

    fn normalize_model(model: &str) -> String {
        let trimmed = model.trim();

        // Strip -thinking suffix first
        let without_thinking = trimmed
            .strip_suffix("-thinking")
            .unwrap_or(trimmed);

        // Apply Copilot-specific aliases (dotted to hyphenated)
        // This matches the alias table used in static_models
        let normalized = match without_thinking {
            "claude-sonnet-4.5" => "claude-sonnet-4-5",
            "claude-sonnet-4.6" => "claude-sonnet-4-6",
            "claude-opus-4.5" => "claude-opus-4-5",
            "claude-opus-4.6" => "claude-opus-4-6",
            "claude-haiku-4.5" => "claude-haiku-4-5",
            "claude-haiku-4.6" => "claude-haiku-4-6",
            other => other,
        };

        normalized.to_string()
    }

    fn flatten_message_content(content: &MessageContent) -> MessageContent {
        match content {
            MessageContent::Text(text) => MessageContent::Text(text.clone()),
            MessageContent::Parts(parts) => {
                let text_parts = parts
                    .iter()
                    .filter_map(|part| {
                        if part.part_type == "text" {
                            part.text.clone()
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                MessageContent::Text(text_parts.join("\n"))
            }
        }
    }

    fn normalize_chat_request(&self, request: &ChatCompletionRequest) -> Value {
        let mut value = match serde_json::to_value(request) {
            Ok(v) => v,
            Err(e) => {
                error!(
                    error = %e,
                    "failed to serialize ChatCompletionRequest to JSON, using empty object"
                );
                json!({})
            }
        };
        let body = value.as_object_mut().expect("chat request should serialize to object");
        body.insert("model".to_string(), json!(Self::normalize_model(&request.model)));

        if let Some(Value::Array(messages)) = body.get_mut("messages") {
            for message in messages.iter_mut() {
                if let Some(map) = message.as_object_mut() {
                    if let Some(role) = map.get("role").and_then(Value::as_str) {
                        if role == "assistant" {
                            let content = map
                                .get("content")
                                .cloned()
                                .unwrap_or_else(|| json!(""));
                            let flattened: MessageContent = serde_json::from_value(content)
                                .map(|parsed| Self::flatten_message_content(&parsed))
                                .unwrap_or_else(|_| MessageContent::Text(String::new()));
                            map.insert(
                                "content".to_string(),
                                serde_json::to_value(flattened).unwrap_or_else(|_| json!("")),
                            );
                        }
                    }
                }
            }
        }

        if let Some(Value::Array(tools)) = body.get_mut("tools") {
            tools.retain(|tool| {
                tool.get("type")
                    .and_then(Value::as_str)
                    .map(|tool_type| tool_type == "function")
                    .unwrap_or(false)
            });
        }

        if body.contains_key("tool_choice") {
            let needs_auto = !matches!(
                body.get("tool_choice"),
                Some(Value::String(value)) if value == "auto" || value == "none"
            );
            if needs_auto {
                body.insert("tool_choice".to_string(), json!("auto"));
            }
        }

        value
    }

    fn is_responses_request(&self, canonical_model: &str, request: &ChatCompletionRequest) -> bool {
        if request.extra.contains_key("input") {
            return true;
        }

        static_models::lookup_static_model(canonical_model)
            .and_then(|model| model.supported_endpoints)
            .map(|endpoints| {
                endpoints.len() == 1 && endpoints.iter().any(|endpoint| endpoint == "responses")
            })
            .unwrap_or_else(|| canonical_model.contains("codex"))
    }

    fn convert_chat_to_responses_body(&self, request: &ChatCompletionRequest, canonical_model: &str) -> AppResult<Value> {
        // If already has input field, use it directly
        if let Some(input) = request.extra.get("input") {
            let mut body = serde_json::to_value(request)
                .map_err(|error| AppError::BadRequest(format!("serialize responses request: {error}")))?;
            let map = body.as_object_mut().expect("responses request should serialize to object");
            map.insert("model".to_string(), json!(canonical_model));
            return Ok(body);
        }

        // Convert messages to input array
        let input: Vec<Value> = request.messages.iter().map(|msg| {
            json!({
                "role": msg.role,
                "content": match &msg.content {
                    MessageContent::Text(text) => json!(text),
                    MessageContent::Parts(parts) => json!(parts),
                }
            })
        }).collect();

        let mut body = json!({
            "model": canonical_model,
            "input": input,
        });

        // Copy over other relevant fields
        if let Some(obj) = body.as_object_mut() {
            if let Some(temp) = request.extra.get("temperature") {
                obj.insert("temperature".to_string(), temp.clone());
            }
            if let Some(max_tokens) = request.extra.get("max_tokens") {
                obj.insert("max_tokens".to_string(), max_tokens.clone());
            }
            if let Some(stream) = request.extra.get("stream") {
                obj.insert("stream".to_string(), stream.clone());
            }
        }

        Ok(body)
    }

    fn has_image_input(&self, request: &ChatCompletionRequest) -> bool {
        if let Some(input) = request.extra.get("input") {
            if input.to_string().contains("input_image") || input.to_string().contains("image_url") {
                return true;
            }
        }

        request.messages.iter().any(|message| match &message.content {
            MessageContent::Text(_) => false,
            MessageContent::Parts(parts) => parts.iter().any(|part| {
                part.part_type == "image_url" || part.image_url.is_some()
            }),
        })
    }

    fn request_headers(
        &self,
        token: &str,
        include_vision: bool,
    ) -> AppResult<reqwest::header::HeaderMap> {
        use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};

        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|error| AppError::Auth(format!("invalid authorization header: {error}")))?,
        );
        headers.insert(
            reqwest::header::USER_AGENT,
            HeaderValue::from_static(USER_AGENT),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(
            "editor-version",
            HeaderValue::from_static(EDITOR_VERSION),
        );
        headers.insert(
            "editor-plugin-version",
            HeaderValue::from_static(EDITOR_PLUGIN_VERSION),
        );
        headers.insert("openai-intent", HeaderValue::from_static(OPENAI_INTENT));
        headers.insert(
            "copilot-integration-id",
            HeaderValue::from_static(COPILOT_INTEGRATION_ID),
        );
        headers.insert(
            "x-github-api-version",
            HeaderValue::from_static(X_GITHUB_API_VERSION),
        );
        headers.insert(
            "x-request-id",
            HeaderValue::from_str(&uuid::Uuid::new_v4().to_string())
                .map_err(|error| AppError::Auth(format!("invalid request id header: {error}")))?,
        );
        headers.insert("x-initiator", HeaderValue::from_static(X_INITIATOR));
        if include_vision {
            headers.insert("copilot-vision-request", HeaderValue::from_static("true"));
        }
        Ok(headers)
    }

    fn map_upstream_error(status: reqwest::StatusCode, body: String) -> AppError {
        match status {
            reqwest::StatusCode::UNAUTHORIZED => AppError::Auth(format!(
                "github copilot upstream unauthorized ({}): {}",
                status,
                body.trim()
            )),
            reqwest::StatusCode::TOO_MANY_REQUESTS => AppError::QuotaExceeded(format!(
                "github copilot upstream rate limited ({}): {}",
                status,
                body.trim()
            )),
            _ => AppError::Upstream(format!(
                "github copilot upstream error ({}): {}",
                status,
                body.trim()
            )),
        }
    }

    fn parse_chat_response(body: Value) -> AppResult<ChatCompletionResponse> {
        let usage = body.get("usage").map(|usage| Usage {
            prompt_tokens: usage
                .get("prompt_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
            completion_tokens: usage
                .get("completion_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
            total_tokens: usage
                .get("total_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
        });

        let choices = body
            .get("choices")
            .and_then(Value::as_array)
            .ok_or_else(|| AppError::Upstream("failed to decode copilot chat response: missing choices".into()))?
            .iter()
            .enumerate()
            .map(|(idx, choice)| {
                let message = choice.get("message").cloned().map(|message| ChatMessage {
                    role: message
                        .get("role")
                        .and_then(Value::as_str)
                        .unwrap_or("assistant")
                        .to_string(),
                    content: serde_json::from_value(
                        message.get("content").cloned().unwrap_or_else(|| json!("")),
                    )
                    .unwrap_or_else(|_| MessageContent::Text(String::new())),
                    name: message
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    tool_calls: message
                        .get("tool_calls")
                        .and_then(Value::as_array)
                        .cloned(),
                    tool_call_id: message
                        .get("tool_call_id")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                });

                let delta = choice.get("delta").cloned().map(|delta| ChatMessage {
                    role: delta
                        .get("role")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    content: serde_json::from_value(
                        delta.get("content").cloned().unwrap_or_else(|| json!("")),
                    )
                    .unwrap_or_else(|_| MessageContent::Text(String::new())),
                    name: delta.get("name").and_then(Value::as_str).map(str::to_string),
                    tool_calls: delta.get("tool_calls").and_then(Value::as_array).cloned(),
                    tool_call_id: delta
                        .get("tool_call_id")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                });

                Choice {
                    index: choice
                        .get("index")
                        .and_then(Value::as_u64)
                        .map(|value| value as u32)
                        .unwrap_or(idx as u32),
                    message,
                    delta,
                    finish_reason: choice
                        .get("finish_reason")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                }
            })
            .collect::<Vec<_>>();

        Ok(ChatCompletionResponse {
            id: body
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("chatcmpl_github_copilot")
                .to_string(),
            object: body
                .get("object")
                .and_then(Value::as_str)
                .unwrap_or("chat.completion")
                .to_string(),
            created: body.get("created").and_then(Value::as_i64).unwrap_or(0),
            model: body
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            choices,
            usage,
        })
    }

    fn parse_responses_response(body: Value) -> AppResult<ChatCompletionResponse> {
        // Extract text with step-by-step validation and logging
        let text = match body.get("output") {
            Some(output) => match output.as_array() {
                Some(output_array) => match output_array.first() {
                    Some(first_output) => match first_output.get("content") {
                        Some(content) => match content.as_array() {
                            Some(content_array) => match content_array.first() {
                                Some(first_content) => match first_content.get("text") {
                                    Some(text_value) => match text_value.as_str() {
                                        Some(text_str) => text_str.to_string(),
                                        None => {
                                            debug!("parse_responses_response: 'text' field is not a string");
                                            String::new()
                                        }
                                    },
                                    None => {
                                        debug!("parse_responses_response: 'text' field missing in first content item");
                                        String::new()
                                    }
                                },
                                None => {
                                    debug!("parse_responses_response: content array is empty");
                                    String::new()
                                }
                            },
                            None => {
                                debug!("parse_responses_response: 'content' field is not an array");
                                String::new()
                            }
                        },
                        None => {
                            debug!("parse_responses_response: 'content' field missing in first output item");
                            String::new()
                        }
                    },
                    None => {
                        debug!("parse_responses_response: output array is empty");
                        String::new()
                    }
                },
                None => {
                    debug!("parse_responses_response: 'output' field is not an array");
                    String::new()
                }
            },
            None => {
                debug!("parse_responses_response: 'output' field missing");
                String::new()
            }
        };

        let usage = body.get("usage").map(|usage| Usage {
            prompt_tokens: usage
                .get("input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
            completion_tokens: usage
                .get("output_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
            total_tokens: usage
                .get("total_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
        });

        Ok(ChatCompletionResponse {
            id: body
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("resp_github_copilot")
                .to_string(),
            object: "chat.completion".to_string(),
            created: body.get("created_at").and_then(Value::as_i64).unwrap_or(0),
            model: body
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            choices: vec![Choice {
                index: 0,
                message: Some(ChatMessage {
                    role: "assistant".to_string(),
                    content: MessageContent::Text(text),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                }),
                delta: None,
                finish_reason: Some("stop".to_string()),
            }],
            usage,
        })
    }
}

fn build_client(timeout: Duration) -> AppResult<Client> {
    Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(timeout)
        .build()
        .map_err(|error| AppError::Config(format!("failed to build github copilot client: {error}")))
}

fn build_stream_client(timeout: Duration) -> AppResult<Client> {
    Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(timeout)
        .read_timeout(timeout)
        .build()
        .map_err(|error| AppError::Config(format!("failed to build github copilot stream client: {error}")))
}

#[async_trait]
impl Provider for GithubCopilotProvider {
    fn name(&self) -> &str {
        "github-copilot"
    }

    fn client_id(&self) -> &str {
        &self.record.id
    }

    async fn list_models(&self) -> AppResult<Vec<ModelInfo>> {
        match self.live_models().await {
            Ok(models) => Ok(models),
            Err(error) => {
                debug!(
                    error = %error,
                    "failed to fetch live models, falling back to static models"
                );
                Ok(self.static_models())
            }
        }
    }

    async fn chat_completion(
        &self,
        request: &ChatCompletionRequest,
    ) -> AppResult<ChatCompletionResponse> {
        let token = self.copilot_api_token().await?;
        let canonical_model = Self::normalize_model(&request.model);
        let use_responses = self.is_responses_request(&canonical_model, request);
        let include_vision = self.has_image_input(request);
        let endpoint = if use_responses {
            format!("{}/responses", self.api_base_url())
        } else {
            format!("{}/chat/completions", self.api_base_url())
        };
        let body = if use_responses {
            self.convert_chat_to_responses_body(request, &canonical_model)?
        } else {
            self.normalize_chat_request(request)
        };

        let response = self
            .client
            .post(endpoint)
            .headers(self.request_headers(&token, include_vision)?)
            .json(&body)
            .send()
            .await
            .map_err(|error| AppError::Upstream(format!("github copilot request failed: {error}")))?;

        let status = response.status();
        let body_text = response
            .text()
            .await
            .map_err(|error| AppError::Upstream(format!("failed reading github copilot response body: {error}")))?;

        if !status.is_success() {
            return Err(Self::map_upstream_error(status, body_text));
        }

        let body: Value = serde_json::from_str(&body_text)
            .map_err(|error| AppError::Upstream(format!("failed to parse github copilot response JSON: {error}")))?;

        if use_responses {
            Self::parse_responses_response(body)
        } else {
            Self::parse_chat_response(body)
        }
    }

    async fn chat_completion_stream(
        &self,
        request: &ChatCompletionRequest,
    ) -> AppResult<BoxStream> {
        let token = self.copilot_api_token().await?;
        let canonical_model = Self::normalize_model(&request.model);
        let use_responses = self.is_responses_request(&canonical_model, request);
        let endpoint = if use_responses {
            format!("{}/responses", self.api_base_url())
        } else {
            format!("{}/chat/completions", self.api_base_url())
        };
        let body = if use_responses {
            self.convert_chat_to_responses_body(request, &canonical_model)?
        } else {
            self.normalize_chat_request(request)
        };

        let response = self
            .stream_client
            .post(endpoint)
            .headers(self.request_headers(&token, self.has_image_input(request))?)
            .json(&body)
            .send()
            .await
            .map_err(|error| AppError::Upstream(format!("github copilot stream request failed: {error}")))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.map_err(|error| {
                AppError::Upstream(format!("failed reading github copilot stream body: {error}"))
            })?;
            return Err(Self::map_upstream_error(status, body));
        }

        let stream = response.bytes_stream().map(|item| {
            item.map_err(|error| AppError::Upstream(format!("github copilot stream read error: {error}")))
        });

        Ok(Box::pin(stream))
    }
}
