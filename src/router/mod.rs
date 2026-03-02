use axum::{
    Router,
    routing::{get, post},
};
use std::sync::Arc;
use crate::proxy::ProxyState;

/// Build the main Axum router with all API routes matching the Go CLIProxyAPI layout:
///
/// OpenAI-compatible:
///   GET  /v1/models
///   POST /v1/chat/completions
///   POST /v1/completions  (legacy)
///   POST /v1/responses    (OpenAI Responses API)
///
/// Gemini-compatible:
///   GET  /v1beta/models
///   POST /v1beta/models/:model:generateContent
///   POST /v1beta/models/:model:streamGenerateContent
///
/// Claude-compatible:
///   POST /v1/messages
///
/// Amp provider aliases:
///   GET  /api/provider/:provider/v1/models
///   POST /api/provider/:provider/v1/chat/completions
///   POST /api/provider/:provider/v1/messages
///
/// Management (localhost-only):
///   GET  /v0/management/...
///
/// Health:
///   GET  /health
pub fn build_router(state: Arc<ProxyState>) -> Router {
    Router::new()
        // ── Health ────────────────────────────────────────────────────────────
        .route("/health", get(crate::proxy::handlers::health))

        // ── OpenAI-compatible ─────────────────────────────────────────────────
        .route("/v1/models", get(crate::proxy::handlers::list_models))
        .route(
            "/v1/chat/completions",
            post(crate::proxy::handlers::chat_completions),
        )
        .route(
            "/v1/completions",
            post(crate::proxy::handlers::chat_completions),
        )
        .route(
            "/v1/responses",
            post(crate::proxy::handlers::chat_completions),
        )
        .route(
            "/v1/messages",
            post(crate::proxy::handlers::claude_messages),
        )

        // ── Gemini-compatible ─────────────────────────────────────────────────
        .route(
            "/v1beta/models",
            get(crate::proxy::handlers::list_models_gemini),
        )
        .route(
            "/v1beta/models/{model_action}",
            post(crate::proxy::handlers::gemini_generate),
        )

        // ── Amp provider aliases /api/provider/{provider}/v1/... ─────────────
        .route(
            "/api/provider/{provider}/v1/models",
            get(crate::proxy::handlers::amp_list_models),
        )
        .route(
            "/api/provider/{provider}/v1/chat/completions",
            post(crate::proxy::handlers::amp_chat_completions),
        )
        .route(
            "/api/provider/{provider}/v1/messages",
            post(crate::proxy::handlers::amp_claude_messages),
        )

        // ── Management API ────────────────────────────────────────────────────
        .nest("/v0/management", crate::proxy::management::router(state.clone()))
        // ── OAuth callbacks (top-level, no auth middleware) ──────────────────
        .route(
            "/antigravity/callback",
            get(crate::proxy::oauth::antigravity_callback),
        )
        .with_state(state)
}
