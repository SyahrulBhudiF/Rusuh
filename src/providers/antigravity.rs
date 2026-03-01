//! Antigravity provider — translates OpenAI chat completions to/from Antigravity's Gemini-like API.


use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::RwLock;
use tracing::{debug, warn};

use crate::auth::antigravity_login;
use crate::auth::store::AuthRecord;
use crate::error::{AppError, AppResult};
use crate::models::{ChatCompletionRequest, ChatCompletionResponse, ModelInfo};
use crate::providers::{BoxStream, Provider};

// ── Constants ────────────────────────────────────────────────────────────────

const BASE_URL_DAILY: &str = "https://daily-cloudcode-pa.googleapis.com";
const BASE_URL_SANDBOX: &str = "https://daily-cloudcode-pa.sandbox.googleapis.com";
const STREAM_PATH: &str = "/v1internal:streamGenerateContent";
const GENERATE_PATH: &str = "/v1internal:generateContent";
const MODELS_PATH: &str = "/v1internal:fetchAvailableModels";
const USER_AGENT: &str = "antigravity/1.104.0 darwin/arm64";

// ── Provider ─────────────────────────────────────────────────────────────────

pub struct AntigravityProvider {
    record: AuthRecord,
    client: reqwest::Client,
    /// Cached access token (refreshed when expired)
    access_token: RwLock<String>,
}

impl AntigravityProvider {
    pub fn new(record: AuthRecord) -> Self {
        let token = record
            .access_token()
            .unwrap_or_default()
            .to_string();
        Self {
            record,
            client: reqwest::Client::new(),
            access_token: RwLock::new(token),
        }
    }

    /// Get a valid access token, refreshing if needed.
    async fn get_token(&self) -> AppResult<String> {
        // Check if current token is still valid
        let token = self.access_token.read().await.clone();
        if !token.is_empty() && !self.is_token_expired() {
            return Ok(token);
        }

        // Try refresh
        let refresh_token = self
            .record
            .metadata
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::Auth("missing refresh_token".into()))?;

        debug!("refreshing antigravity access token");
        let resp = antigravity_login::refresh_access_token(&self.client, refresh_token)
            .await
            .map_err(|e| AppError::Auth(format!("token refresh failed: {e}")))?;

        let new_token = resp.access_token.clone();
        *self.access_token.write().await = new_token.clone();
        Ok(new_token)
    }

    fn is_token_expired(&self) -> bool {
        if let Some(Value::Number(n)) = self.record.metadata.get("expires_at") {
            if let Some(exp) = n.as_i64() {
                return chrono::Utc::now().timestamp() >= exp;
            }
        }
        false
    }

    fn project_id(&self) -> Option<&str> {
        self.record.project_id()
    }

    fn base_urls(&self) -> Vec<&str> {
        // Check for custom base_url in metadata
        if let Some(Value::String(url)) = self.record.metadata.get("base_url") {
            let url = url.trim();
            if !url.is_empty() {
                // Leak is fine — this lives for the process lifetime
                return vec![Box::leak(url.to_string().into_boxed_str())];
            }
        }
        vec![BASE_URL_DAILY, BASE_URL_SANDBOX]
    }

    /// Translate OpenAI ChatCompletionRequest → Antigravity JSON payload.
    fn translate_request(&self, req: &ChatCompletionRequest, _stream: bool) -> Value {
        let mut contents = Vec::new();

        // Separate system messages
        let mut system_parts = Vec::new();

        for msg in &req.messages {
            let role = match msg.role.as_str() {
                "assistant" => "model",
                "system" => {
                    let text = msg.content.as_text();
                    if !text.is_empty() {
                        system_parts.push(json!({"text": text}));
                    }
                    continue;
                }
                "tool" => "function",
                other => other,
            };

            let parts = msg.content.as_parts();
            if !parts.is_empty() {
                contents.push(json!({
                    "role": role,
                    "parts": parts,
                }));
            }
        }

        let model = &req.model;
        let project = self
            .project_id()
            .unwrap_or("unknown-project")
            .to_string();

        let request_id = format!("agent-{}", uuid::Uuid::new_v4());
        let session_id = format!("-{}", rand::random::<u64>() & 0x7FFFFFFFFFFFFFFF);

        let mut request_body = json!({
            "contents": contents,
        });

        // Add system instruction
        if !system_parts.is_empty() {
            request_body["systemInstruction"] = json!({
                "role": "user",
                "parts": system_parts,
            });
        }

        // Generation config
        let mut gen_config = json!({});
        if let Some(temp) = req.temperature {
            gen_config["temperature"] = json!(temp);
        }
        if let Some(top_p) = req.top_p {
            gen_config["topP"] = json!(top_p);
        }
        if let Some(max) = req.max_tokens {
            gen_config["maxOutputTokens"] = json!(max);
        }
        if gen_config.as_object().is_some_and(|o| !o.is_empty()) {
            request_body["generationConfig"] = gen_config;
        }

        // Tools
        if let Some(tools) = &req.tools {
            let gemini_tools = translate_tools_to_gemini(tools);
            if !gemini_tools.is_empty() {
                request_body["tools"] = json!(gemini_tools);
            }
        }

        let payload = json!({
            "model": model,
            "userAgent": "antigravity",
            "requestType": "agent",
            "project": project,
            "requestId": request_id,
            "request": request_body,
        });

        // Set session ID
        let mut payload = payload;
        payload["request"]["sessionId"] = json!(session_id);

        payload
    }

    /// Translate Antigravity response → OpenAI ChatCompletionResponse.
    fn translate_response(&self, data: &Value, model: &str) -> ChatCompletionResponse {
        let response = data.get("response").unwrap_or(data);
        let candidate = &response["candidates"][0];
        let parts = candidate["content"]["parts"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        // Concatenate text parts
        let mut text = String::new();
        for part in &parts {
            if let Some(t) = part["text"].as_str() {
                text.push_str(t);
            }
        }

        let finish_reason = candidate["finishReason"]
            .as_str()
            .map(|r| match r {
                "STOP" => "stop",
                "MAX_TOKENS" | "MAX_OUTPUT_TOKENS" => "length",
                "TOOL_CALL" => "tool_calls",
                _ => "stop",
            })
            .unwrap_or("stop")
            .to_string();

        // Parse usage
        let usage_meta = response
            .get("usageMetadata")
            .or_else(|| data.get("usageMetadata"));

        let usage = usage_meta.map(|u| crate::models::Usage {
            prompt_tokens: u["promptTokenCount"].as_u64().unwrap_or(0) as u32,
            completion_tokens: u["candidatesTokenCount"].as_u64().unwrap_or(0) as u32,
            total_tokens: u["totalTokenCount"].as_u64().unwrap_or(0) as u32,
        });

        ChatCompletionResponse {
            id: format!("chatcmpl-{}", uuid::Uuid::new_v4()),
            object: "chat.completion".into(),
            created: chrono::Utc::now().timestamp(),
            model: model.to_string(),
            choices: vec![crate::models::Choice {
                index: 0,
                message: Some(crate::models::ChatMessage {
                    role: "assistant".into(),
                    content: crate::models::MessageContent::Text(text),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                }),
                delta: None,
                finish_reason: Some(finish_reason),
            }],
            usage,
        }
    }
}

#[async_trait]
impl Provider for AntigravityProvider {
    fn name(&self) -> &str {
        "antigravity"
    }

    async fn list_models(&self) -> AppResult<Vec<ModelInfo>> {
        let token = self.get_token().await?;
        let now = chrono::Utc::now().timestamp();

        for base_url in self.base_urls() {
            let url = format!("{}{}", base_url, MODELS_PATH);

            let resp = self
                .client
                .post(&url)
                .bearer_auth(&token)
                .header("Content-Type", "application/json")
                .header("User-Agent", USER_AGENT)
                .json(&json!({}))
                .send()
                .await;

            let resp = match resp {
                Ok(r) => r,
                Err(e) => {
                    warn!("antigravity models request failed on {}: {e}", base_url);
                    continue;
                }
            };

            if !resp.status().is_success() {
                warn!(
                    "antigravity models returned {} from {}",
                    resp.status(),
                    base_url
                );
                continue;
            }

            let data: Value = resp
                .json()
                .await
                .map_err(|e| AppError::Upstream(format!("parse models: {e}")))?;

            let models_obj = match data.get("models").and_then(|m| m.as_object()) {
                Some(m) => m,
                None => continue,
            };

            let mut models = Vec::new();
            for (id, _info) in models_obj {
                let id = id.trim();
                if id.is_empty() {
                    continue;
                }
                // Skip known internal/duplicate models
                match id {
                    "chat_20706" | "chat_23310" | "gemini-2.5-flash-thinking"
                    | "gemini-3-pro-low" | "gemini-2.5-pro" => continue,
                    _ => {}
                }
                models.push(ModelInfo {
                    id: id.to_string(),
                    object: "model".into(),
                    created: now,
                    owned_by: "antigravity".into(),
                });
            }

            if !models.is_empty() {
                return Ok(models);
            }
        }

        Ok(Vec::new())
    }

    async fn chat_completion(
        &self,
        req: &ChatCompletionRequest,
    ) -> AppResult<ChatCompletionResponse> {
        let token = self.get_token().await?;
        let payload = self.translate_request(req, false);

        for base_url in self.base_urls() {
            let url = format!("{}{}", base_url, GENERATE_PATH);

            let resp = self
                .client
                .post(&url)
                .bearer_auth(&token)
                .header("Content-Type", "application/json")
                .header("User-Agent", USER_AGENT)
                .header("Accept", "application/json")
                .json(&payload)
                .send()
                .await;

            let resp = match resp {
                Ok(r) => r,
                Err(e) => {
                    warn!("antigravity request failed on {}: {e}", base_url);
                    continue;
                }
            };

            let status = resp.status();
            let body: Value = resp
                .json()
                .await
                .map_err(|e| AppError::Upstream(format!("parse response: {e}")))?;

            if !status.is_success() {
                return Err(AppError::Upstream(format!(
                    "antigravity error ({}): {}",
                    status, body
                )));
            }

            return Ok(self.translate_response(&body, &req.model));
        }

        Err(AppError::Upstream(
            "all antigravity base URLs failed".into(),
        ))
    }

    async fn chat_completion_stream(
        &self,
        req: &ChatCompletionRequest,
    ) -> AppResult<BoxStream> {
        let token = self.get_token().await?;
        let payload = self.translate_request(req, true);
        let model = req.model.clone();
        for base_url in self.base_urls() {
            let url = format!("{}{}?alt=sse", base_url, STREAM_PATH);
            let resp = self
                .client
                .post(&url)
                .bearer_auth(&token)
                .header("Content-Type", "application/json")
                .header("User-Agent", USER_AGENT)
                .header("Accept", "text/event-stream")
                .json(&payload)
                .send()
                .await;
            let resp = match resp {
                Ok(r) => r,
                Err(e) => {
                    warn!("antigravity stream failed on {}: {e}", base_url);
                    continue;
                }
            };
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(AppError::Upstream(format!(
                    "antigravity stream error ({}): {}",
                    status, body
                )));
            }
            let id = format!("chatcmpl-{}", uuid::Uuid::new_v4());
            let created = chrono::Utc::now().timestamp();
            let transform = crate::proxy::stream::antigravity_to_openai_transform(
                id, model, created,
            );
            return Ok(crate::proxy::stream::buffered_sse_stream(
                resp.bytes_stream(),
                transform,
            ));
        }
        Err(AppError::Upstream(
            "all antigravity stream base URLs failed".into(),
        ))
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Helper trait on MessageContent for translation.
trait MessageContentExt {
    fn as_text(&self) -> String;
    fn as_parts(&self) -> Vec<Value>;
}

impl MessageContentExt for crate::models::MessageContent {
    fn as_text(&self) -> String {
        match self {
            crate::models::MessageContent::Text(t) => t.clone(),
            crate::models::MessageContent::Parts(parts) => {
                parts
                    .iter()
                    .filter_map(|p| p.text.as_deref())
                    .collect::<Vec<_>>()
                    .join("")
            }
        }
    }

    fn as_parts(&self) -> Vec<Value> {
        match self {
            crate::models::MessageContent::Text(t) => {
                vec![json!({"text": t})]
            }
            crate::models::MessageContent::Parts(parts) => {
                parts
                    .iter()
                    .map(|p| {
                        if let Some(ref text) = p.text {
                            json!({"text": text})
                        } else if let Some(ref img) = p.image_url {
                            json!({"inlineData": {"url": img.url}})
                        } else {
                            json!({"text": ""})
                        }
                    })
                    .collect()
            }
        }
    }
}

/// Translate OpenAI tools to Gemini function declarations.
fn translate_tools_to_gemini(tools: &[Value]) -> Vec<Value> {
    let mut declarations = Vec::new();
    for tool in tools {
        if tool["type"].as_str() == Some("function") {
            if let Some(func) = tool.get("function") {
                declarations.push(json!({
                    "functionDeclarations": [func],
                }));
            }
        }
    }
    declarations
}
