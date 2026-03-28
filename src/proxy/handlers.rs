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
    proxy::ProxyState,
};

const PUBLIC_MODEL_SONNET_46: &str = "claude-sonnet-4.6";
const PUBLIC_MODEL_SONNET_45: &str = "claude-sonnet-4.5";
const PUBLIC_MODEL_SONNET_45_THINKING: &str = "claude-sonnet-4.5-thinking";

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
    let providers = {
        let providers = state.providers.read().await;
        providers.iter().cloned().collect::<Vec<_>>()
    };

    let mut models = Vec::new();
    for provider in providers {
        if let Ok(mut pm) = provider.list_models().await {
            models.append(&mut pm);
        }
    }
    models
}

async fn collect_provider_models(state: &ProxyState, provider_name: &str) -> Vec<ModelInfo> {
    let providers = {
        let providers = state.providers.read().await;
        providers
            .iter()
            .filter(|provider| provider.name().eq_ignore_ascii_case(provider_name))
            .cloned()
            .collect::<Vec<_>>()
    };

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

    for public_model in [
        PUBLIC_MODEL_SONNET_46,
        PUBLIC_MODEL_SONNET_45,
        PUBLIC_MODEL_SONNET_45_THINKING,
    ] {
        let targets = public_route_targets(public_model);
        let mut available = false;
        for target in targets {
            let clients = state
                .model_registry
                .available_clients_for_model(target.model)
                .await;
            if clients.iter().any(|client_id| {
                client_provider_name(client_id)
                    .map(|name| name.eq_ignore_ascii_case(target.provider))
                    .unwrap_or(false)
            }) {
                available = true;
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
        PUBLIC_MODEL_SONNET_46 => &[RouteTarget {
            provider: "zed",
            model: "claude-sonnet-4-6",
        }],
        PUBLIC_MODEL_SONNET_45 => &[
            RouteTarget {
                provider: "kiro",
                model: "kiro-claude-sonnet-4-5",
            },
            RouteTarget {
                provider: "zed",
                model: "claude-sonnet-4-5",
            },
        ],
        PUBLIC_MODEL_SONNET_45_THINKING => &[RouteTarget {
            provider: "kiro",
            model: "kiro-claude-sonnet-4-5-agentic",
        }],
        _ => &[],
    }
}

fn client_provider_name(client_id: &str) -> Option<&str> {
    client_id.split_once('_').map(|(provider, _)| provider)
}

fn is_public_model(model: &str) -> bool {
    !public_route_targets(model).is_empty()
}

/// Resolve model name through configured OAuth aliases only.
fn resolve_oauth_model_alias(config: &crate::config::Config, model: &str) -> String {
    for aliases in config.oauth_model_alias.values() {
        for entry in aliases {
            if entry.alias.eq_ignore_ascii_case(model) {
                return entry.name.clone();
            }
        }
    }

    model.to_string()
}

fn responses_body_to_chat_request(body: Value) -> Result<ChatCompletionRequest, AppError> {
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::BadRequest("responses request missing model".to_string()))?
        .to_string();

    let input = body.get("input").cloned().unwrap_or(Value::Null);
    let text = match input {
        Value::String(s) => s,
        Value::Null => String::new(),
        Value::Array(items) => {
            let mut segments = Vec::new();
            for (idx, item) in items.into_iter().enumerate() {
                match item {
                    Value::String(s) => segments.push(s),
                    Value::Object(map) => {
                        let text = map.get("text").and_then(Value::as_str).ok_or_else(|| {
                            AppError::BadRequest(format!(
                                "responses input array item {idx} must be a string or object with a text string"
                            ))
                        })?;
                        segments.push(text.to_string());
                    }
                    _ => {
                        return Err(AppError::BadRequest(format!(
                            "responses input array item {idx} must be a string or object with a text string"
                        )));
                    }
                }
            }
            segments.join("\n")
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
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: MessageContent::Text(text),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }],
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::responses_body_to_chat_request;
    use crate::error::AppError;

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
}

async fn route_chat(
    state: Arc<ProxyState>,
    req: ChatCompletionRequest,
    provider_hint: Option<String>,
) -> Result<Response, AppError> {
    let is_stream = req.stream.unwrap_or(false);

    if provider_hint.is_none() {
        let public_targets = public_route_targets(&req.model);
        if !public_targets.is_empty() {
            return try_route_with_targets(state, &req, public_targets, is_stream).await;
        }

        let config = state.config.read().await;
        let resolved_model = resolve_oauth_model_alias(&config, &req.model);
        if resolved_model != req.model || !is_public_model(&req.model) {
            return Err(AppError::BadRequest(format!(
                "Model '{}' is not available on the public endpoint. Use one of the curated public models or a provider-pinned route.",
                req.model
            )));
        }
    }

    let mut req = req;
    let resolved_model = {
        let config = state.config.read().await;
        resolve_oauth_model_alias(&config, &req.model)
    };
    req.model = resolved_model;

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
        if let Some(session_id) = execution_session_id.as_ref() {
            state
                .execution_sessions
                .set_selected_auth(session_id.clone(), selected_auth_id.clone())
                .await;
        }
        Some(selected_auth_id)
    } else if let Some(session_id) = execution_session_id.as_ref() {
        state.execution_sessions.get_selected_auth(session_id).await
    } else {
        None
    };

    let candidates = resolve_candidates_for_model(
        &state,
        &req.model,
        provider_hint,
        effective_selected_auth_id.as_deref(),
    )
    .await;
    execute_candidates(
        state,
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
            if let Some(session_id) = execution_session_id.as_ref() {
                state
                    .execution_sessions
                    .set_selected_auth(session_id.clone(), selected_auth_id.clone())
                    .await;
            }
            Some(selected_auth_id)
        } else if let Some(session_id) = execution_session_id.as_ref() {
            state.execution_sessions.get_selected_auth(session_id).await
        } else {
            None
        };

        let candidates = resolve_candidates_for_model(
            &state,
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
    model_id: &str,
    provider_hint: &Option<String>,
    selected_auth_id: Option<&str>,
) -> Vec<usize> {
    let model_providers = state.model_registry.get_model_providers(model_id).await;
    let provider_names = {
        let providers = state.providers.read().await;
        providers
            .iter()
            .map(|provider| provider.name().to_string())
            .collect::<Vec<_>>()
    };

    let candidates: Vec<usize> = if let Some(hint) = provider_hint {
        provider_names
            .iter()
            .enumerate()
            .filter(|(_, name)| name.eq_ignore_ascii_case(hint))
            .map(|(i, _)| i)
            .collect()
    } else if !model_providers.is_empty() {
        provider_names
            .iter()
            .enumerate()
            .filter(|(_, name)| {
                model_providers
                    .iter()
                    .any(|mp| mp.eq_ignore_ascii_case(name))
            })
            .map(|(i, _)| i)
            .collect()
    } else {
        (0..provider_names.len()).collect()
    };

    let mut available_candidates = Vec::new();
    for idx in candidates {
        let client_id = format!("{}_{}", provider_names[idx], idx);

        if let Some(selected) = selected_auth_id {
            if !client_id.eq_ignore_ascii_case(selected) {
                continue;
            }
        }

        if state
            .model_registry
            .client_is_effectively_available(&client_id, model_id)
            .await
        {
            available_candidates.push(idx);
        }
    }

    available_candidates
}

async fn execute_candidates(
    state: Arc<ProxyState>,
    req: &ChatCompletionRequest,
    candidates: Vec<usize>,
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
    let start_idx = {
        let balancer = state.balancer.read().await;
        balancer.pick(&candidates)
    };
    let start_pos = candidates.iter().position(|&c| c == start_idx).unwrap_or(0);
    let ordered: Vec<usize> = (0..candidates.len())
        .map(|i| candidates[(start_pos + i) % candidates.len()])
        .collect();
    let mut last_error = None;

    let providers = {
        let providers = state.providers.read().await;
        ordered
            .iter()
            .filter_map(|&idx| providers.get(idx).cloned().map(|provider| (idx, provider)))
            .collect::<Vec<_>>()
    };

    for (idx, provider) in providers {
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
                        let selected_auth_id = format!("{}_{}", provider.name(), idx);
                        state
                            .execution_sessions
                            .set_selected_auth(session_id.to_string(), selected_auth_id)
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
