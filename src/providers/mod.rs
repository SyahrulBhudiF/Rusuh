pub mod antigravity;
pub mod model_info;
pub mod model_registry;
pub mod registry;
pub mod static_models;

use crate::{
    error::AppResult,
    models::{ChatCompletionRequest, ChatCompletionResponse, ModelInfo},
};
use async_trait::async_trait;
use bytes::Bytes;
use futures::Stream;
use std::pin::Pin;

pub type BoxStream = Pin<Box<dyn Stream<Item = AppResult<Bytes>> + Send>>;

/// Implemented by every upstream provider (Gemini, Claude, Codex, Qwen, iFlow, Antigravity…)
#[async_trait]
pub trait Provider: Send + Sync {
    /// Human-readable name for logging
    fn name(&self) -> &str;

    /// List models this provider exposes
    async fn list_models(&self) -> AppResult<Vec<ModelInfo>>;

    /// Non-streaming chat completion
    async fn chat_completion(
        &self,
        req: &ChatCompletionRequest,
    ) -> AppResult<ChatCompletionResponse>;

    /// Streaming chat completion — returns SSE byte stream
    async fn chat_completion_stream(
        &self,
        req: &ChatCompletionRequest,
    ) -> AppResult<BoxStream>;
}
