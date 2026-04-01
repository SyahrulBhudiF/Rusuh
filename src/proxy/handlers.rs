use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::{collections::HashSet, sync::Arc};

use crate::{
    error::AppError,
    models::{ChatCompletionRequest, ChatMessage, MessageContent, ModelInfo, ModelsResponse},
    providers::Provider,
    proxy::ProxyState,
};

const PUBLIC_MODEL_OPUS_46: &str = "claude-opus-4-6";
const PUBLIC_MODEL_OPUS_45: &str = "claude-opus-4-5";
const PUBLIC_MODEL_SONNET_46: &str = "claude-sonnet-4-6";
const PUBLIC_MODEL_SONNET_45: &str = "claude-sonnet-4-5";
const PUBLIC_MODEL_HAIKU_45: &str = "claude-haiku-4-5";
const PUBLIC_MODEL_GPT_54: &str = "gpt-5.4";
const PUBLIC_MODEL_GPT_53_CODEX: &str = "gpt-5.3-codex";

#[derive(Clone, Copy)]
struct RouteTarget {
    provider: &'static str,
    model: &'static str,
}

// ── Health ────────────────────────────────────────────────────────────────────

pub async fn health() -> Response {
    Json(json!({ "status": "ok", "service": "rusuh" })).into_response()
}

// ── OpenAI-compatible ─────────────────────────────────────────────────────────

/// GET /v1/models
pub async fn list_models(State(state): State<Arc<ProxyState>>) -> Response {
    let models = public_catalog_models(&state).await;
    Json(ModelsResponse {
        object: "list".to_string(),
        data: models,
    })
    .into_response()
}

/// POST /v1/chat/completions
pub async fn chat_completions(
    State(state): State<Arc<ProxyState>>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Response, AppError> {
    route_chat(state, req, None).await
}

/// POST /v1/responses
pub async fn responses(
    State(state): State<Arc<ProxyState>>,
    Json(body): Json<Value>,
) -> Result<Response, AppError> {
    let req = responses_body_to_chat_request(body)?;
    route_chat(state, req, None).await
}

/// POST /v1/responses/compact
pub async fn responses_compact(
    State(state): State<Arc<ProxyState>>,
    Json(body): Json<Value>,
) -> Result<Response, AppError> {
    // TODO: /v1/responses/compact currently matches /v1/responses intentionally
    // until compact-specific response shaping is implemented in route_chat.
    let req = responses_body_to_chat_request(body)?;
    route_chat(state, req, None).await
}

/// POST /v1/messages  (Claude-compatible)
pub async fn claude_messages(
    State(_state): State<Arc<ProxyState>>,
    Json(_body): Json<Value>,
) -> Result<Response, AppError> {
    Err(AppError::BadRequest(
        "Claude messages endpoint not yet implemented".to_string(),
    ))
}

// ── Gemini-compatible ─────────────────────────────────────────────────────────

pub async fn list_models_gemini(State(state): State<Arc<ProxyState>>) -> Response {
    let models = state.model_registry.get_available_models("gemini").await;
    if models.is_empty() {
        let models = collect_models(&state).await;
        return Json(json!({
            "models": models.iter().map(|m| json!({
                "name": format!("models/{}", m.id),
                "displayName": m.id,
                "supportedGenerationMethods": ["generateContent", "streamGenerateContent"],
            })).collect::<Vec<_>>()
        }))
        .into_response();
    }
    Json(json!({ "models": models })).into_response()
}

/// POST /v1beta/models/:model_action
pub async fn gemini_generate(
    State(_state): State<Arc<ProxyState>>,
    Path(_model_action): Path<String>,
    Json(_body): Json<Value>,
) -> Result<Response, AppError> {
    Err(AppError::BadRequest(
        "Gemini generate endpoint not yet implemented".to_string(),
    ))
}

// ── Amp provider aliases ──────────────────────────────────────────────────────

pub async fn amp_list_models(
    State(state): State<Arc<ProxyState>>,
    Path(provider): Path<String>,
) -> Response {
    let models = collect_provider_models(&state, &provider).await;
    Json(ModelsResponse {
        object: "list".to_string(),
        data: models,
    })
    .into_response()
}

pub async fn amp_chat_completions(
    State(state): State<Arc<ProxyState>>,
    Path(provider): Path<String>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Response, AppError> {
    route_chat(state, req, Some(provider)).await
}

pub async fn amp_claude_messages(
    State(_state): State<Arc<ProxyState>>,
    Path(_provider): Path<String>,
    Json(_body): Json<Value>,
) -> Result<Response, AppError> {
    Err(AppError::BadRequest(
        "Amp Claude messages endpoint not yet implemented".to_string(),
    ))
}

// ── Internal helpers ──────────────────────────────────────────────────────────

async fn collect_models(state: &ProxyState) -> Vec<ModelInfo> {
    let providers = state.current_runtime_snapshot().await.providers().to_vec();

    let mut models = Vec::new();
    for provider in providers {
        if let Ok(mut pm) = provider.list_models().await {
            models.append(&mut pm);
        }
    }
    models
}

async fn collect_provider_models(state: &ProxyState, provider_name: &str) -> Vec<ModelInfo> {
    let providers = state
        .current_runtime_snapshot()
        .await
        .providers()
        .iter()
        .filter(|provider| provider.provider_type().eq_ignore_ascii_case(provider_name))
        .cloned()
        .collect::<Vec<_>>();

    let mut models = Vec::new();
    let mut seen_ids = HashSet::new();

    for provider in providers {
        if let Ok(pm) = provider.list_models().await {
            for model in pm {
                if seen_ids.insert(model.id.clone()) {
                    models.push(model);
                }
            }
        }
    }

    models
}

async fn public_catalog_models(state: &ProxyState) -> Vec<ModelInfo> {
    let now = chrono::Utc::now().timestamp();
    let mut models = Vec::new();
    let runtime_snapshot = state.current_runtime_snapshot().await;

    for public_model in [
        PUBLIC_MODEL_OPUS_46,
        PUBLIC_MODEL_OPUS_45,
        PUBLIC_MODEL_SONNET_46,
        PUBLIC_MODEL_SONNET_45,
        PUBLIC_MODEL_HAIKU_45,
        PUBLIC_MODEL_GPT_54,
        PUBLIC_MODEL_GPT_53_CODEX,
    ] {
        let targets = public_route_targets(public_model);
        let mut available = false;
        for target in targets {
            for client_id in runtime_snapshot.available_clients_for_model(target.model) {
                let Some(provider) = runtime_snapshot.providers().iter().find(|provider| {
                    provider.client_id().eq_ignore_ascii_case(client_id)
                        && provider
                            .provider_type()
                            .eq_ignore_ascii_case(target.provider)
                }) else {
                    continue;
                };

                if runtime_snapshot.client_supports_model(provider.client_id(), target.model)
                    && state
                        .model_registry
                        .client_is_effectively_available(provider.client_id(), target.model)
                        .await
                {
                    available = true;
                    break;
                }
            }
            if available {
                break;
            }
        }

        if available {
            models.push(ModelInfo {
                id: public_model.to_string(),
                object: "model".to_string(),
                created: now,
                owned_by: "rusuh".to_string(),
            });
        }
    }

    models
}

fn public_route_targets(model: &str) -> &'static [RouteTarget] {
    match model {
        PUBLIC_MODEL_OPUS_46 => &[RouteTarget {
            provider: "github-copilot",
            model: "claude-opus-4.6",
        }],
        PUBLIC_MODEL_OPUS_45 => &[RouteTarget {
            provider: "github-copilot",
            model: "claude-opus-4.5",
        }],
        PUBLIC_MODEL_SONNET_46 => &[
            RouteTarget {
                provider: "zed",
                model: "claude-sonnet-4-6",
            },
            RouteTarget {
                provider: "github-copilot",
                model: "claude-sonnet-4.6",
            },
        ],
        PUBLIC_MODEL_SONNET_45 => &[
            RouteTarget {
                provider: "kiro",
                model: "kiro-claude-sonnet-4-5-agentic",
            },
            RouteTarget {
                provider: "kiro",
                model: "kiro-claude-sonnet-4-5",
            },
            RouteTarget {
                provider: "zed",
                model: "claude-sonnet-4-5",
            },
            RouteTarget {
                provider: "github-copilot",
                model: "claude-sonnet-4.5",
            },
        ],
        PUBLIC_MODEL_HAIKU_45 => &[
            RouteTarget {
                provider: "kiro",
                model: "kiro-claude-haiku-4-5-agentic",
            },
            RouteTarget {
                provider: "kiro",
                model: "kiro-claude-haiku-4-5",
            },
            RouteTarget {
                provider: "github-copilot",
                model: "claude-haiku-4.5",
            },
        ],
        PUBLIC_MODEL_GPT_54 => &[
            RouteTarget {
                provider: "codex",
                model: "gpt-5.4",
            },
            RouteTarget {
                provider: "github-copilot",
                model: "gpt-5.4",
            },
        ],
        PUBLIC_MODEL_GPT_53_CODEX => &[
            RouteTarget {
                provider: "codex",
                model: "gpt-5.3-codex",
            },
            RouteTarget {
                provider: "github-copilot",
                model: "gpt-5.3-codex",
            },
        ],
        _ => &[],
    }
}

fn reserved_public_route_targets(model: &str) -> &'static [RouteTarget] {
    match model {
        "claude-sonnet-4-5-thinking" => &[RouteTarget {
            provider: "kiro",
            model: "kiro-claude-sonnet-4-5-agentic",
        }],
        _ => &[],
    }
}

fn is_reserved_public_model(model: &str) -> bool {
    !reserved_public_route_targets(model).is_empty()
}

fn is_public_model(model: &str) -> bool {
    !public_route_targets(model).is_empty() || is_reserved_public_model(model)
}

/// Resolve model name through configured OAuth aliases only.
fn resolve_oauth_model_alias(config: &crate::config::Config, model: &str) -> String {
    // Try exact match first
    for aliases in config.oauth_model_alias.values() {
        for entry in aliases {
            if entry.alias.eq_ignore_ascii_case(model) {
                return entry.name.clone();
            }
        }
    }

    // Try normalizing by stripping suffix after version number
    // Pattern: claude-{family}-{major}.{minor}[-suffix]
    // Example: "claude-sonnet-4.5-thinking" → "claude-sonnet-4.5"
    if let Some(normalized) = normalize_model_name_for_alias(model) {
        for aliases in config.oauth_model_alias.values() {
            for entry in aliases {
                if entry.alias.eq_ignore_ascii_case(&normalized) {
                    return entry.name.clone();
                }
            }
        }
    }

    model.to_string()
}

fn normalize_model_name_for_alias(model: &str) -> Option<String> {
    let parts: Vec<&str> = model.split('-').collect();
    if parts.len() < 3 {
        return None;
    }

    // Look for the last part that matches version pattern (e.g., "4.5", "4.6")
    let mut last_version_index = None;
    for (i, part) in parts.iter().enumerate() {
        if part.contains('.') {
            let version_parts: Vec<&str> = part.split('.').collect();
            if version_parts.len() == 2
                && version_parts[0].chars().all(|c| c.is_ascii_digit())
                && version_parts[1].chars().all(|c| c.is_ascii_digit())
            {
                last_version_index = Some(i);
            }
        }
    }

    // Reconstruct base model name up to and including the last version
    last_version_index.map(|i| parts[..=i].join("-"))
}

fn responses_body_to_chat_request(body: Value) -> Result<ChatCompletionRequest, AppError> {
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::BadRequest("responses request missing model".to_string()))?
        .to_string();

    let input = body.get("input").cloned().unwrap_or(Value::Null);
    let messages = match input {
        Value::String(s) => vec![ChatMessage {
            role: "user".to_string(),
            content: MessageContent::Text(s),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }],
        Value::Null => vec![ChatMessage {
            role: "user".to_string(),
            content: MessageContent::Text(String::new()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }],
        Value::Array(items) => {
            let mut messages = Vec::with_capacity(items.len());
            for (idx, item) in items.into_iter().enumerate() {
                match item {
                    Value::String(s) => messages.push(ChatMessage {
                        role: "user".to_string(),
                        content: MessageContent::Text(s),
                        name: None,
                        tool_calls: None,
                        tool_call_id: None,
                    }),
                    Value::Object(map) => {
                        let text = map
                            .get("text")
                            .and_then(Value::as_str)
                            .or_else(|| {
                                map.get("content").and_then(Value::as_array).and_then(|items| {
                                    items.iter().find_map(|content_item| {
                                        let content_map = content_item.as_object()?;
                                        let text = content_map.get("text").and_then(Value::as_str)?;
                                        match content_map.get("type").and_then(Value::as_str) {
                                            Some("input_text") | None => Some(text),
                                            _ => None,
                                        }
                                    })
                                })
                            })
                            .ok_or_else(|| {
                                AppError::BadRequest(format!(
                                    "responses input array item {idx} must be a string or object with a text string"
                                ))
                            })?;
                        let role = match map.get("role").and_then(Value::as_str) {
                            Some("system") => "system",
                            Some("developer") => "developer",
                            Some("assistant") => "assistant",
                            Some("tool") => "tool",
                            Some("user") | None => "user",
                            Some(other) => other,
                        };
                        messages.push(ChatMessage {
                            role: role.to_string(),
                            content: MessageContent::Text(text.to_string()),
                            name: map.get("name").and_then(Value::as_str).map(str::to_string),
                            tool_calls: map.get("tool_calls").and_then(Value::as_array).cloned(),
                            tool_call_id: map
                                .get("tool_call_id")
                                .and_then(Value::as_str)
                                .map(str::to_string),
                        });
                    }
                    _ => {
                        return Err(AppError::BadRequest(format!(
                            "responses input array item {idx} must be a string or object with a text string"
                        )));
                    }
                }
            }
            messages
        }
        _ => {
            return Err(AppError::BadRequest(
                "responses input must be a string, null, or array to build a chat completion request"
                    .to_string(),
            ));
        }
    };

    let stream = body.get("stream").and_then(Value::as_bool);

    let mut extra = match body {
        Value::Object(map) => map.into_iter().collect::<std::collections::HashMap<_, _>>(),
        _ => std::collections::HashMap::new(),
    };
    extra.remove("model");
    extra.remove("input");
    extra.remove("stream");

    Ok(ChatCompletionRequest {
        model,
        messages,
        stream,
        max_tokens: None,
        temperature: None,
        top_p: None,
        tools: None,
        tool_choice: None,
        stop: None,
        extra,
    })
}

async fn route_chat(
    state: Arc<ProxyState>,
    req: ChatCompletionRequest,
    provider_hint: Option<String>,
) -> Result<Response, AppError> {
    let is_stream = req.stream.unwrap_or(false);

    // Resolve aliases early so dotted models like "claude-sonnet-4.6" match public routes
    let mut req = req;
    let resolved_model = {
        let config = state.config.read().await;
        resolve_oauth_model_alias(&config, &req.model)
    };
    req.model = resolved_model;

    if provider_hint.is_none() {
        let public_targets = public_route_targets(&req.model);
        if !public_targets.is_empty() {
            return try_route_with_targets(state, &req, public_targets, is_stream).await;
        }
        let reserved_targets = reserved_public_route_targets(&req.model);
        if !reserved_targets.is_empty() {
            return try_route_with_targets(state, &req, reserved_targets, is_stream).await;
        }

        if !is_public_model(&req.model) {
            return Err(AppError::BadRequest(format!(
                "Model '{}' is not available on the public endpoint. Use one of the curated public models or a provider-pinned route.",
                req.model
            )));
        }
    }

    try_route_with_model(state, &req, &provider_hint, is_stream).await
}

/// Internal helper: attempt routing with a specific model name.
async fn try_route_with_model(
    state: Arc<ProxyState>,
    req: &ChatCompletionRequest,
    provider_hint: &Option<String>,
    is_stream: bool,
) -> Result<Response, AppError> {
    let request_selected_auth_id = req
        .extra
        .get("selected_auth_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);

    let execution_session_id = req
        .extra
        .get("execution_session_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);

    let effective_selected_auth_id = if let Some(selected_auth_id) = request_selected_auth_id {
        Some(selected_auth_id)
    } else if let Some(session_id) = execution_session_id.as_ref() {
        state.execution_sessions.get_selected_auth(session_id).await
    } else {
        None
    };

    let runtime_snapshot = state.current_runtime_snapshot().await;
    let candidates = resolve_candidates_for_model(
        &state,
        runtime_snapshot.clone(),
        &req.model,
        provider_hint,
        effective_selected_auth_id.as_deref(),
    )
    .await;
    execute_candidates(
        state,
        runtime_snapshot,
        req,
        candidates,
        is_stream,
        execution_session_id.as_deref(),
    )
    .await
}

async fn try_route_with_targets(
    state: Arc<ProxyState>,
    req: &ChatCompletionRequest,
    targets: &[RouteTarget],
    is_stream: bool,
) -> Result<Response, AppError> {
    let mut last_error = None;
    let execution_session_id = req
        .extra
        .get("execution_session_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);

    for target in targets {
        let mut upstream_req = req.clone();
        upstream_req.model = target.model.to_string();
        let provider_hint = Some(target.provider.to_string());
        let request_selected_auth_id = req
            .extra
            .get("selected_auth_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);

        let effective_selected_auth_id = if let Some(selected_auth_id) = request_selected_auth_id {
            Some(selected_auth_id)
        } else if let Some(session_id) = execution_session_id.as_ref() {
            state.execution_sessions.get_selected_auth(session_id).await
        } else {
            None
        };

        let runtime_snapshot = state.current_runtime_snapshot().await;
        let candidates = resolve_candidates_for_model(
            &state,
            runtime_snapshot.clone(),
            target.model,
            &provider_hint,
            effective_selected_auth_id.as_deref(),
        )
        .await;

        if candidates.is_empty() {
            last_error = Some(AppError::QuotaExceeded(format!(
                "All providers for model '{}' are currently unavailable (quota exceeded or suspended)",
                target.model
            )));
            continue;
        }

        match execute_candidates(
            state.clone(),
            runtime_snapshot,
            &upstream_req,
            candidates,
            is_stream,
            execution_session_id.as_deref(),
        )
        .await
        {
            Ok(response) => return Ok(response),
            Err(e) if e.is_quota_or_unavailable() || e.is_account_error() => {
                last_error = Some(e);
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        AppError::NoAccounts(format!(
            "No provider available for model '{}'. Add credentials to config.yaml.",
            req.model
        ))
    }))
}

async fn resolve_candidates_for_model(
    state: &ProxyState,
    runtime_snapshot: Arc<crate::proxy::RuntimeSnapshot>,
    model_id: &str,
    provider_hint: &Option<String>,
    selected_auth_id: Option<&str>,
) -> Vec<Arc<dyn Provider>> {
    let model_providers = runtime_snapshot.model_providers(model_id);
    let providers = runtime_snapshot.providers().to_vec();

    let mut available_candidates = Vec::new();
    for provider in providers {
        if !runtime_snapshot.client_supports_model(provider.client_id(), model_id) {
            continue;
        }

        if let Some(hint) = provider_hint {
            if !provider.provider_type().eq_ignore_ascii_case(hint) {
                continue;
            }
        } else if !model_providers.is_empty()
            && !model_providers
                .iter()
                .any(|provider_name| provider_name.eq_ignore_ascii_case(provider.provider_type()))
        {
            continue;
        }

        let client_id = provider.client_id();
        if let Some(selected) = selected_auth_id {
            if !client_id.eq_ignore_ascii_case(selected) {
                continue;
            }
        }

        if state
            .model_registry
            .client_is_effectively_available(client_id, model_id)
            .await
        {
            available_candidates.push(provider);
        }
    }

    available_candidates
}

async fn execute_candidates(
    state: Arc<ProxyState>,
    runtime_snapshot: Arc<crate::proxy::RuntimeSnapshot>,
    req: &ChatCompletionRequest,
    candidates: Vec<Arc<dyn Provider>>,
    is_stream: bool,
    execution_session_id: Option<&str>,
) -> Result<Response, AppError> {
    if candidates.is_empty() {
        return Err(AppError::QuotaExceeded(format!(
            "All providers for model '{}' are currently unavailable (quota exceeded or suspended)",
            req.model
        )));
    }

    let max_retries = {
        let config = state.config.read().await;
        config.request_retry.max(1) as usize
    };
    let candidate_indices: Vec<usize> = (0..candidates.len()).collect();
    let start_idx = runtime_snapshot.balancer().pick(&candidate_indices);
    let start_pos = candidate_indices
        .iter()
        .position(|&candidate_index| candidate_index == start_idx)
        .unwrap_or(0);
    let ordered: Vec<Arc<dyn Provider>> = (0..candidates.len())
        .map(|offset| candidates[(start_pos + offset) % candidates.len()].clone())
        .collect();
    let mut last_error = None;

    for provider in ordered {
        for attempt in 0..max_retries {
            if attempt > 0 {
                let delay = std::time::Duration::from_millis(100 << (attempt - 1).min(4));
                tracing::info!(
                    "retry {}/{} for provider {} (backoff {}ms)",
                    attempt + 1,
                    max_retries,
                    provider.name(),
                    delay.as_millis()
                );
                tokio::time::sleep(delay).await;
            }

            let result = if is_stream {
                provider
                    .chat_completion_stream(req)
                    .await
                    .map(crate::proxy::stream::sse_response)
            } else {
                provider
                    .chat_completion(req)
                    .await
                    .map(|r| Json(r).into_response())
            };

            match result {
                Ok(resp) => {
                    if let Some(session_id) = execution_session_id {
                        let selected_auth_id = provider.client_id().to_string();
                        let selected_auth_is_active = state
                            .current_runtime_snapshot()
                            .await
                            .providers()
                            .iter()
                            .any(|provider| {
                                provider.client_id().eq_ignore_ascii_case(&selected_auth_id)
                            });
                        state
                            .execution_sessions
                            .set_selected_auth(
                                session_id.to_string(),
                                selected_auth_id,
                                selected_auth_is_active,
                            )
                            .await;
                    }
                    return Ok(resp);
                }
                Err(e) => {
                    if e.is_account_error() {
                        tracing::warn!(
                            "provider {} account error (skipping): {e}",
                            provider.name()
                        );
                        last_error = Some(e);
                        break;
                    }
                    if e.is_transient() && attempt + 1 < max_retries {
                        tracing::warn!(
                            "provider {} transient error (attempt {}/{}): {e}",
                            provider.name(),
                            attempt + 1,
                            max_retries
                        );
                        last_error = Some(e);
                        continue;
                    }
                    tracing::warn!("provider {} error: {e}", provider.name());
                    last_error = Some(e);
                    break;
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        AppError::NoAccounts(format!(
            "All providers failed for model '{}'. Check credentials and upstream availability.",
            req.model
        ))
    }))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use async_trait::async_trait;
    use axum::body::to_bytes;
    use serde_json::{json, Value};
    use tokio::sync::Mutex;

    use super::{
        execute_candidates, resolve_candidates_for_model, responses_body_to_chat_request,
        try_route_with_model, MessageContent,
    };
    use crate::auth::manager::AccountManager;
    use crate::config::Config;
    use crate::error::{AppError, AppResult};
    use crate::models::{
        ChatCompletionRequest, ChatCompletionResponse, ChatMessage, Choice,
        MessageContent as ChatMessageContent, ModelInfo,
    };
    use crate::providers::model_info::ExtModelInfo;
    use crate::providers::{BoxStream, Provider};
    use crate::proxy::ProxyState;

    #[test]
    fn responses_body_rejects_unsupported_top_level_input_shape() {
        let error = responses_body_to_chat_request(json!({
            "model": "claude-sonnet-4.6",
            "input": {"text": "hello"}
        }))
        .expect_err("object input should be rejected");

        match error {
            AppError::BadRequest(message) => {
                assert!(message.contains("responses input must be a string, null, or array"));
            }
            other => panic!("expected bad request, got {other}"),
        }
    }

    #[test]
    fn responses_body_preserves_array_message_boundaries_and_metadata() {
        let request = responses_body_to_chat_request(json!({
            "model": "claude-sonnet-4.6",
            "input": [
                "hello",
                {
                    "role": "system",
                    "content": [
                        {"type": "input_image", "image_url": "ignored"},
                        {"type": "input_text", "text": "from nested content"}
                    ]
                },
                {
                    "role": "developer",
                    "text": "planner note",
                    "name": "planner"
                },
                {
                    "role": "assistant",
                    "text": "calling tool",
                    "tool_calls": [
                        {
                            "id": "call_1",
                            "type": "function",
                            "function": {"name": "lookup", "arguments": "{}"}
                        }
                    ]
                },
                {
                    "role": "tool",
                    "text": "tool result",
                    "tool_call_id": "call_1"
                }
            ]
        }))
        .expect("array input should preserve message boundaries and metadata");

        assert_eq!(request.messages.len(), 5);

        assert_eq!(request.messages[0].role, "user");
        match &request.messages[0].content {
            MessageContent::Text(text) => assert_eq!(text, "hello"),
            other => panic!("expected text content, got {other:?}"),
        }

        assert_eq!(request.messages[1].role, "system");
        match &request.messages[1].content {
            MessageContent::Text(text) => assert_eq!(text, "from nested content"),
            other => panic!("expected text content, got {other:?}"),
        }

        assert_eq!(request.messages[2].role, "developer");
        assert_eq!(request.messages[2].name.as_deref(), Some("planner"));
        match &request.messages[2].content {
            MessageContent::Text(text) => assert_eq!(text, "planner note"),
            other => panic!("expected text content, got {other:?}"),
        }

        assert_eq!(request.messages[3].role, "assistant");
        assert_eq!(
            request.messages[3].tool_calls.as_ref().map(Vec::len),
            Some(1)
        );
        match &request.messages[3].content {
            MessageContent::Text(text) => assert_eq!(text, "calling tool"),
            other => panic!("expected text content, got {other:?}"),
        }

        assert_eq!(request.messages[4].role, "tool");
        assert_eq!(request.messages[4].tool_call_id.as_deref(), Some("call_1"));
        match &request.messages[4].content {
            MessageContent::Text(text) => assert_eq!(text, "tool result"),
            other => panic!("expected text content, got {other:?}"),
        }
    }

    #[test]
    fn responses_body_rejects_array_items_with_invalid_shape() {
        let error = responses_body_to_chat_request(json!({
            "model": "claude-sonnet-4.6",
            "input": ["hello", {"type": "input_text", "text": 42}]
        }))
        .expect_err("invalid array item should be rejected");

        match error {
            AppError::BadRequest(message) => {
                assert!(message.contains("responses input array item"));
                assert!(message.contains("string or object with a text string"));
            }
            other => panic!("expected bad request, got {other}"),
        }
    }

    #[derive(Debug)]
    struct TestProvider {
        name: &'static str,
        client_id: &'static str,
        model_id: &'static str,
        response_label: &'static str,
        calls: Arc<Mutex<Vec<&'static str>>>,
    }

    #[async_trait]
    impl Provider for TestProvider {
        fn name(&self) -> &str {
            self.name
        }

        fn client_id(&self) -> &str {
            self.client_id
        }

        async fn list_models(&self) -> AppResult<Vec<ModelInfo>> {
            Ok(vec![ModelInfo {
                id: self.model_id.to_string(),
                object: "model".to_string(),
                created: 0,
                owned_by: self.name.to_string(),
            }])
        }

        async fn chat_completion(
            &self,
            req: &ChatCompletionRequest,
        ) -> AppResult<ChatCompletionResponse> {
            self.calls.lock().await.push(self.client_id);
            Ok(ChatCompletionResponse {
                id: format!("{}-response", self.client_id),
                object: "chat.completion".to_string(),
                created: 0,
                model: req.model.clone(),
                choices: vec![Choice {
                    index: 0,
                    message: Some(ChatMessage {
                        role: "assistant".to_string(),
                        content: ChatMessageContent::Text(self.response_label.to_string()),
                        name: None,
                        tool_calls: None,
                        tool_call_id: None,
                    }),
                    delta: None,
                    finish_reason: Some("stop".to_string()),
                }],
                usage: None,
            })
        }

        async fn chat_completion_stream(
            &self,
            _req: &ChatCompletionRequest,
        ) -> AppResult<BoxStream> {
            unreachable!("streaming not used in this test")
        }
    }

    fn ext_model(id: &str, owned_by: &str, provider_type: &str) -> ExtModelInfo {
        ExtModelInfo {
            id: id.to_string(),
            object: "model".to_string(),
            created: 0,
            owned_by: owned_by.to_string(),
            provider_type: provider_type.to_string(),
            display_name: None,
            name: Some(id.to_string()),
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
        }
    }

    async fn test_state(
        registry: Arc<crate::providers::model_registry::ModelRegistry>,
        providers: Vec<Arc<dyn Provider>>,
    ) -> Arc<ProxyState> {
        let accounts = Arc::new(AccountManager::with_dir("/tmp/rusuh_test_nonexistent"));
        let state = Arc::new(ProxyState::new(
            Config::default(),
            accounts,
            registry,
            providers.len(),
        ));
        state
            .publish_runtime_from_providers(providers)
            .await
            .expect("test providers should publish");
        state
    }

    fn test_request(model: &str) -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: model.to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: ChatMessageContent::Text("hello".to_string()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            }],
            stream: None,
            max_tokens: None,
            temperature: None,
            top_p: None,
            tools: None,
            tool_choice: None,
            stop: None,
            extra: HashMap::new(),
        }
    }

    fn test_request_with_selected_auth(
        model: &str,
        selected_auth_id: &str,
    ) -> ChatCompletionRequest {
        let mut request = test_request(model);
        request.extra.insert(
            "selected_auth_id".to_string(),
            Value::String(selected_auth_id.to_string()),
        );
        request
    }

    #[tokio::test]
    async fn execute_candidates_uses_resolved_auth_snapshot_instead_of_reloaded_indices() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let registry = Arc::new(crate::providers::model_registry::ModelRegistry::new());
        let model_id = "gpt-5-codex";

        registry
            .register_client(
                "auth-first",
                "codex",
                vec![ext_model(model_id, "codex", "codex")],
            )
            .await;
        registry
            .register_client(
                "auth-second",
                "codex",
                vec![ext_model(model_id, "codex", "codex")],
            )
            .await;

        let initial_providers: Vec<Arc<dyn Provider>> = vec![
            Arc::new(TestProvider {
                name: "codex",
                client_id: "auth-first",
                model_id,
                response_label: "first",
                calls: calls.clone(),
            }),
            Arc::new(TestProvider {
                name: "codex",
                client_id: "auth-second",
                model_id,
                response_label: "second",
                calls: calls.clone(),
            }),
        ];
        let state = test_state(registry.clone(), initial_providers).await;

        let runtime_snapshot = state.current_runtime_snapshot().await;
        let candidates = resolve_candidates_for_model(
            &state,
            runtime_snapshot.clone(),
            model_id,
            &Some("codex".to_string()),
            Some("auth-first"),
        )
        .await;
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].client_id(), "auth-first");

        {
            let mut providers = state.providers.write().await;
            *providers = vec![Arc::new(TestProvider {
                name: "codex",
                client_id: "auth-second",
                model_id,
                response_label: "second",
                calls: calls.clone(),
            })];
        }

        let response = execute_candidates(
            state.clone(),
            runtime_snapshot,
            &test_request(model_id),
            candidates,
            false,
            None,
        )
        .await
        .expect("resolved candidate should still execute against the originally selected auth");
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("response body should be readable");
        let json: serde_json::Value =
            serde_json::from_slice(&body).expect("response should be valid json");

        assert_eq!(json["choices"][0]["message"]["content"], "first");
        assert_eq!(calls.lock().await.as_slice(), &["auth-first"]);
    }

    #[tokio::test]
    async fn route_with_selected_auth_uses_published_runtime_snapshot() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let registry = Arc::new(crate::providers::model_registry::ModelRegistry::new());
        let model_id = "gpt-5-codex";

        registry
            .register_client(
                "auth-first",
                "codex",
                vec![ext_model(model_id, "codex", "codex")],
            )
            .await;

        let initial_providers: Vec<Arc<dyn Provider>> = vec![Arc::new(TestProvider {
            name: "codex",
            client_id: "auth-first",
            model_id,
            response_label: "first",
            calls: calls.clone(),
        })];
        let state = test_state(registry, initial_providers).await;

        {
            let mut providers = state.providers.write().await;
            *providers = vec![Arc::new(TestProvider {
                name: "codex",
                client_id: "auth-second",
                model_id,
                response_label: "second",
                calls: calls.clone(),
            })];
        }

        let provider_hint = Some("codex".to_string());
        let request = test_request_with_selected_auth(model_id, "auth-first");
        let response = try_route_with_model(state.clone(), &request, &provider_hint, false)
            .await
            .expect("routing should continue to use the published runtime snapshot");
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("response body should be readable");
        let json: serde_json::Value =
            serde_json::from_slice(&body).expect("response should be valid json");

        assert_eq!(json["choices"][0]["message"]["content"], "first");
        assert_eq!(calls.lock().await.as_slice(), &["auth-first"]);
    }

    #[tokio::test]
    async fn execute_candidates_keeps_balancer_order_from_resolved_runtime_snapshot() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let registry = Arc::new(crate::providers::model_registry::ModelRegistry::new());
        let model_id = "gpt-5-codex";

        registry
            .register_client(
                "auth-first",
                "codex",
                vec![ext_model(model_id, "codex", "codex")],
            )
            .await;
        registry
            .register_client(
                "auth-second",
                "codex",
                vec![ext_model(model_id, "codex", "codex")],
            )
            .await;

        let initial_providers: Vec<Arc<dyn Provider>> = vec![
            Arc::new(TestProvider {
                name: "codex",
                client_id: "auth-first",
                model_id,
                response_label: "first",
                calls: calls.clone(),
            }),
            Arc::new(TestProvider {
                name: "codex",
                client_id: "auth-second",
                model_id,
                response_label: "second",
                calls: calls.clone(),
            }),
        ];
        let state = test_state(registry.clone(), initial_providers).await;

        let resolved_snapshot = state.current_runtime_snapshot().await;
        resolved_snapshot.balancer().pick(&[0, 1]);

        let resolved_runtime_snapshot = state.current_runtime_snapshot().await;
        let candidates = resolve_candidates_for_model(
            &state,
            resolved_runtime_snapshot.clone(),
            model_id,
            &Some("codex".to_string()),
            None,
        )
        .await;
        assert_eq!(candidates.len(), 2);

        state
            .publish_runtime_from_providers(vec![
                Arc::new(TestProvider {
                    name: "codex",
                    client_id: "auth-new-first",
                    model_id,
                    response_label: "new-first",
                    calls: calls.clone(),
                }),
                Arc::new(TestProvider {
                    name: "codex",
                    client_id: "auth-new-second",
                    model_id,
                    response_label: "new-second",
                    calls: calls.clone(),
                }),
                Arc::new(TestProvider {
                    name: "codex",
                    client_id: "auth-new-third",
                    model_id,
                    response_label: "new-third",
                    calls: calls.clone(),
                }),
            ])
            .await
            .expect("replacement providers should publish");

        let response = execute_candidates(
            state.clone(),
            resolved_runtime_snapshot,
            &test_request(model_id),
            candidates,
            false,
            None,
        )
            .await
            .expect("execution should keep using the resolved candidate order");
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("response body should be readable");
        let json: serde_json::Value =
            serde_json::from_slice(&body).expect("response should be valid json");

        assert_eq!(json["choices"][0]["message"]["content"], "second");
        assert_eq!(calls.lock().await.as_slice(), &["auth-second"]);
    }

    #[tokio::test]
    async fn execute_candidates_does_not_reinsert_stale_selected_auth_after_runtime_refresh() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let registry = Arc::new(crate::providers::model_registry::ModelRegistry::new());
        let model_id = "gpt-5-codex";

        registry
            .register_client(
                "auth-first",
                "codex",
                vec![ext_model(model_id, "codex", "codex")],
            )
            .await;

        let state = test_state(
            registry.clone(),
            vec![Arc::new(TestProvider {
                name: "codex",
                client_id: "auth-first",
                model_id,
                response_label: "first",
                calls: calls.clone(),
            })],
        )
        .await;

        let resolved_runtime_snapshot = state.current_runtime_snapshot().await;
        let candidates = resolve_candidates_for_model(
            &state,
            resolved_runtime_snapshot.clone(),
            model_id,
            &Some("codex".to_string()),
            Some("auth-first"),
        )
        .await;
        assert_eq!(candidates.len(), 1);

        state
            .publish_runtime_from_providers(vec![Arc::new(TestProvider {
                name: "codex",
                client_id: "auth-second",
                model_id,
                response_label: "second",
                calls: calls.clone(),
            })])
            .await
            .expect("replacement providers should publish");

        let response = execute_candidates(
            state.clone(),
            resolved_runtime_snapshot,
            &test_request(model_id),
            candidates,
            false,
            Some("session-stale"),
        )
        .await
        .expect("execution should still complete against resolved provider");
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("response body should be readable");
        let json: serde_json::Value =
            serde_json::from_slice(&body).expect("response should be valid json");

        assert_eq!(json["choices"][0]["message"]["content"], "first");
        assert_eq!(calls.lock().await.as_slice(), &["auth-first"]);
        assert_eq!(
            state
                .execution_sessions
                .get_selected_auth("session-stale")
                .await,
            None,
            "stale auth should not be reinserted into sticky session mapping"
        );
    }
}
