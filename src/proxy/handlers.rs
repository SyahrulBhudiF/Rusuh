use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::{
    error::AppError,
    models::{ChatCompletionRequest, ModelInfo, ModelsResponse},
    proxy::ProxyState,
};

// ── Health ────────────────────────────────────────────────────────────────────

pub async fn health() -> Response {
    Json(json!({ "status": "ok", "service": "rusuh" })).into_response()
}

// ── OpenAI-compatible ─────────────────────────────────────────────────────────

/// GET /v1/models
pub async fn list_models(State(state): State<Arc<ProxyState>>) -> Response {
    let models = state.model_registry.get_available_models("openai").await;
    if models.is_empty() {
        // Fallback to direct provider query
        let models = collect_models(&state).await;
        return Json(ModelsResponse {
            object: "list".to_string(),
            data: models,
        })
        .into_response();
    }
    Json(json!({
        "object": "list",
        "data": models,
    }))
    .into_response()
}

/// POST /v1/chat/completions
pub async fn chat_completions(
    State(state): State<Arc<ProxyState>>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Response, AppError> {
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
    Path(_provider): Path<String>,
) -> Response {
    let models = state.model_registry.get_available_models("openai").await;
    if models.is_empty() {
        let models = collect_models(&state).await;
        return Json(ModelsResponse {
            object: "list".to_string(),
            data: models,
        })
        .into_response();
    }
    Json(json!({
        "object": "list",
        "data": models,
    }))
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
    let now = chrono::Utc::now().timestamp();
    let mut models = Vec::new();
    for provider in &state.providers {
        if let Ok(mut pm) = provider.list_models().await {
            models.append(&mut pm);
        }
    }
    if models.is_empty() {
        models.push(ModelInfo {
            id: "rusuh-placeholder".to_string(),
            object: "model".to_string(),
            created: now,
            owned_by: "rusuh".to_string(),
        });
    }
    models
}

/// Resolve model name through OAuth model alias.
/// Config format: `oauth-model-alias.<channel>: [{name, alias}]`
/// When a request asks for `alias`, we rewrite to `name` for the upstream.
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

/// Core routing: model name → provider → execute.
///
/// Flow:
/// 1. Apply OAuth model alias from config
/// 2. Query ModelRegistry for providers serving this model
/// 3. If provider_hint is set, filter to that provider
/// 4. Try providers in order (round-robin comes in PRD #9)
/// 5. On stream requests, return SSE body; otherwise JSON
async fn route_chat(
    state: Arc<ProxyState>,
    mut req: ChatCompletionRequest,
    provider_hint: Option<String>,
) -> Result<Response, AppError> {
    let is_stream = req.stream.unwrap_or(false);

    // Step 1: Apply OAuth model alias
    {
        let config = state.config.read().await;
        let resolved = resolve_oauth_model_alias(&config, &req.model);
        if resolved != req.model {
            tracing::debug!("model alias: {} → {}", req.model, resolved);
            req.model = resolved;
        }
    }

    // Step 2: Get providers for this model from registry
    let model_providers = state.model_registry.get_model_providers(&req.model).await;

    // Step 3: Build candidate provider list
    let candidates: Vec<usize> = if let Some(ref hint) = provider_hint {
        // Filter to matching provider when hint is given
        state
            .providers
            .iter()
            .enumerate()
            .filter(|(_, p)| p.name().eq_ignore_ascii_case(hint))
            .map(|(i, _)| i)
            .collect()
    } else if !model_providers.is_empty() {
        // Use registry-resolved providers
        state
            .providers
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                model_providers
                    .iter()
                    .any(|mp| mp.eq_ignore_ascii_case(p.name()))
            })
            .map(|(i, _)| i)
            .collect()
    } else {
        // Fallback: try all providers
        (0..state.providers.len()).collect()
    };

    if candidates.is_empty() {
        return Err(AppError::NoAccounts(format!(
            "No provider available for model '{}'. Add credentials to config.yaml.",
            req.model
        )));
    }

    // Step 3.5: Filter candidates by effective availability (quota/suspension)
    let mut available_candidates = Vec::new();
    for &idx in &candidates {
        let provider = &state.providers[idx];
        let client_id = format!("{}_{}", provider.name(), idx);

        if state
            .model_registry
            .client_is_effectively_available(&client_id, &req.model)
            .await
        {
            available_candidates.push(idx);
        }
    }

    // Use only available candidates - if none are available, return error
    let candidates = available_candidates;

    if candidates.is_empty() {
        return Err(AppError::QuotaExceeded(format!(
            "All providers for model '{}' are currently unavailable (quota exceeded or suspended)",
            req.model
        )));
    }

    // Step 4: Try candidates with retry logic
    // - Transient errors (5xx, timeout): retry same provider up to request-retry times
    // - Account errors (401, 429): skip to next provider immediately
    let max_retries = {
        let config = state.config.read().await;
        config.request_retry.max(1) as usize
    };
    let start_idx = state.balancer.pick(&candidates);
    let start_pos = candidates.iter().position(|&c| c == start_idx).unwrap_or(0);
    let ordered: Vec<usize> = (0..candidates.len())
        .map(|i| candidates[(start_pos + i) % candidates.len()])
        .collect();
    let mut last_error = None;
    for &idx in &ordered {
        let provider = &state.providers[idx];
        for attempt in 0..max_retries {
            if attempt > 0 {
                // Exponential backoff: 100ms, 200ms, 400ms, ...
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
                    .chat_completion_stream(&req)
                    .await
                    .map(crate::proxy::stream::sse_response)
            } else {
                provider
                    .chat_completion(&req)
                    .await
                    .map(|r| Json(r).into_response())
            };

            match result {
                Ok(resp) => return Ok(resp),
                Err(e) => {
                    if e.is_account_error() {
                        tracing::warn!(
                            "provider {} account error (skipping): {e}",
                            provider.name()
                        );
                        last_error = Some(e);
                        break; // skip to next provider
                    }
                    if e.is_transient() && attempt + 1 < max_retries {
                        tracing::warn!(
                            "provider {} transient error (attempt {}/{}): {e}",
                            provider.name(),
                            attempt + 1,
                            max_retries
                        );
                        last_error = Some(e);
                        continue; // retry same provider
                    }
                    // Non-retryable or exhausted retries
                    tracing::warn!("provider {} error: {e}", provider.name());
                    last_error = Some(e);
                    break; // try next provider
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
