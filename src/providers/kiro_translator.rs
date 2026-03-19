//! KIRO request/response translator — OpenAI ↔ Native KIRO conversion.
//!
//! KIRO uses native conversationState protocol (not Claude format).
//! This module translates between OpenAI and native KIRO formats.

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::models::{ChatCompletionRequest, ChatMessage, MessageContent};

// ── Native Kiro Request Structures ──────────────────────────────────────────

/// Native Kiro API request wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KiroRequest {
    /// Conversation state containing the message and history
    pub conversation_state: ConversationState,
    /// Profile ARN (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_arn: Option<String>,
}

/// Conversation state - core structure for Kiro requests
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationState {
    /// Conversation ID (UUID)
    pub conversation_id: String,
    /// Current message being sent
    pub current_message: CurrentMessage,
    /// Agent task type (usually "vibe")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_task_type: Option<String>,
    /// Chat trigger type ("MANUAL" or "AUTO")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_trigger_type: Option<String>,
    /// Message history
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<HistoryMessage>,
}

/// Current message container
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentMessage {
    /// User input message
    pub user_input_message: UserInputMessage,
}

/// User input message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInputMessage {
    /// Message content
    pub content: String,
    /// Model ID
    pub model_id: String,
    /// Message origin (usually "AI_EDITOR")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// User input message context (tools, tool results)
    #[serde(default)]
    pub user_input_message_context: UserInputMessageContext,
}

/// User input message context
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInputMessageContext {
    /// Available tools
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Value>,
    /// Tool execution results
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_results: Vec<Value>,
}

/// History message (user or assistant)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HistoryMessage {
    /// User message
    User(HistoryUserMessage),
    /// Assistant message
    Assistant(HistoryAssistantMessage),
}

/// History user message wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryUserMessage {
    /// User input message
    pub user_input_message: HistoryUserContent,
}

/// History user message content
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryUserContent {
    /// Message content
    pub content: String,
    /// Model ID
    pub model_id: String,
    /// Message origin
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

/// History assistant message wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryAssistantMessage {
    /// Assistant response message
    pub assistant_response_message: HistoryAssistantContent,
}

/// History assistant message content
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryAssistantContent {
    /// Response content
    pub content: String,
}

// ── Request Builder ─────────────────────────────────────────────────────────

/// Build native Kiro request from OpenAI ChatCompletionRequest
pub fn build_native_kiro_request(
    req: &ChatCompletionRequest,
    profile_arn: Option<String>,
) -> KiroRequest {
    let conversation_id = Uuid::new_v4().to_string();
    let model_id = req.model.clone();

    // Separate current message from history
    let (current_content, history) = extract_messages_and_history(&req.messages, &model_id);

    // Build current message
    let user_input_message = UserInputMessage {
        content: current_content,
        model_id,
        origin: Some("AI_EDITOR".to_string()),
        user_input_message_context: UserInputMessageContext::default(),
    };

    let current_message = CurrentMessage { user_input_message };

    // Build conversation state
    let conversation_state = ConversationState {
        conversation_id,
        current_message,
        agent_task_type: Some("vibe".to_string()),
        chat_trigger_type: Some("MANUAL".to_string()),
        history,
    };

    KiroRequest {
        conversation_state,
        profile_arn,
    }
}

/// Extract current message and history from OpenAI messages
fn extract_messages_and_history(
    messages: &[ChatMessage],
    model_id: &str,
) -> (String, Vec<HistoryMessage>) {
    if messages.is_empty() {
        return (String::new(), Vec::new());
    }

    // Last user message becomes current message
    // Everything before becomes history
    let mut history = Vec::new();
    let mut current_content = String::new();

    for (idx, msg) in messages.iter().enumerate() {
        let is_last = idx == messages.len() - 1;
        let content = extract_text_content(&msg.content);

        match msg.role.as_str() {
            "system" => {
                // System messages go into current content as prefix
                if !content.is_empty() {
                    if !current_content.is_empty() {
                        current_content.push_str("\n\n");
                    }
                    current_content.push_str(&content);
                }
            }
            "user" => {
                if is_last {
                    // Last user message is current
                    if !current_content.is_empty() {
                        current_content.push_str("\n\n");
                    }
                    current_content.push_str(&content);
                } else {
                    // Previous user messages go to history
                    history.push(HistoryMessage::User(HistoryUserMessage {
                        user_input_message: HistoryUserContent {
                            content,
                            model_id: model_id.to_string(),
                            origin: Some("AI_EDITOR".to_string()),
                        },
                    }));
                }
            }
            "assistant" => {
                // Assistant messages go to history
                history.push(HistoryMessage::Assistant(HistoryAssistantMessage {
                    assistant_response_message: HistoryAssistantContent { content },
                }));
            }
            _ => {}
        }
    }

    (current_content, history)
}

/// Extract text content from MessageContent
fn extract_text_content(content: &MessageContent) -> String {
    match content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::Parts(parts) => parts
            .iter()
            .filter_map(|p| p.text.as_deref())
            .collect::<Vec<_>>()
            .join(""),
    }
}

// ── Response Translation ─────────────────────────────────────────────────────

/// Event types from KIRO Event Stream
#[derive(Debug, Clone, PartialEq)]
pub enum KiroEventType {
    /// Message start event
    MessageStart,
    /// Content block start
    ContentBlockStart,
    /// Content block delta (streaming text)
    ContentBlockDelta,
    /// Content block stop
    ContentBlockStop,
    /// Message delta (metadata updates)
    MessageDelta,
    /// Message stop (completion)
    MessageStop,
    /// Assistant response event (KIRO-specific)
    AssistantResponseEvent,
    /// Tool use event
    ToolUseEvent,
    /// Usage/metrics event
    UsageEvent,
    /// Metering event
    MeteringEvent,
    /// Metrics event
    MetricsEvent,
    /// Followup prompt event (UI-specific, filtered)
    FollowupPromptEvent,
    /// Unknown event type
    Unknown(String),
}

impl KiroEventType {
    pub fn parse(s: &str) -> Self {
        match s {
            "message_start" | "messageStart" => Self::MessageStart,
            "content_block_start" | "contentBlockStart" => Self::ContentBlockStart,
            "content_block_delta" | "contentBlockDelta" => Self::ContentBlockDelta,
            "content_block_stop" | "contentBlockStop" => Self::ContentBlockStop,
            "message_delta" | "messageDelta" => Self::MessageDelta,
            "message_stop" | "messageStop" => Self::MessageStop,
            "assistantResponseEvent" => Self::AssistantResponseEvent,
            "toolUseEvent" => Self::ToolUseEvent,
            "usageEvent" | "messageMetadataEvent" => Self::UsageEvent,
            "meteringEvent" => Self::MeteringEvent,
            "metricsEvent" => Self::MetricsEvent,
            "followupPromptEvent" => Self::FollowupPromptEvent,
            other => Self::Unknown(other.to_string()),
        }
    }

    /// Check if this event should be filtered out (not sent to client)
    pub fn should_filter(&self) -> bool {
        matches!(
            self,
            Self::FollowupPromptEvent | Self::MeteringEvent | Self::MetricsEvent
        )
    }
}

/// Translate KIRO Event Stream message to OpenAI SSE format.
///
/// Returns SSE-formatted bytes ready to send to client.
/// Returns None if event should be filtered.
pub fn translate_kiro_event_to_openai_sse(
    event_type: &str,
    payload: &[u8],
    chat_id: &str,
    model: &str,
    created: i64,
) -> Option<Bytes> {
    let event = KiroEventType::parse(event_type);

    // Filter out UI-specific events
    if event.should_filter() {
        return None;
    }

    // Parse payload
    let payload_json: Value = match serde_json::from_slice(payload) {
        Ok(v) => v,
        Err(_) => return None,
    };

    match event {
        KiroEventType::MessageStart => {
            // Send initial chunk with role
            let chunk = json!({
                "id": chat_id,
                "object": "chat.completion.chunk",
                "created": created,
                "model": model,
                "choices": [{
                    "index": 0,
                    "delta": {
                        "role": "assistant",
                        "content": ""
                    },
                    "finish_reason": null
                }]
            });
            Some(format_sse_event("message", &chunk))
        }

        KiroEventType::ContentBlockDelta | KiroEventType::AssistantResponseEvent => {
            // Extract text content
            let text = extract_delta_text(&payload_json);
            if text.is_empty() {
                return None;
            }

            let chunk = json!({
                "id": chat_id,
                "object": "chat.completion.chunk",
                "created": created,
                "model": model,
                "choices": [{
                    "index": 0,
                    "delta": {
                        "content": text
                    },
                    "finish_reason": null
                }]
            });
            Some(format_sse_event("message", &chunk))
        }

        KiroEventType::MessageStop => {
            // Extract stop reason
            let stop_reason = payload_json["stopReason"]
                .as_str()
                .or_else(|| payload_json["stop_reason"].as_str())
                .unwrap_or("stop");

            let finish_reason = match stop_reason {
                "end_turn" => "stop",
                "max_tokens" => "length",
                "tool_use" => "tool_calls",
                _ => "stop",
            };

            let chunk = json!({
                "id": chat_id,
                "object": "chat.completion.chunk",
                "created": created,
                "model": model,
                "choices": [{
                    "index": 0,
                    "delta": {},
                    "finish_reason": finish_reason
                }]
            });
            Some(format_sse_event("message", &chunk))
        }

        KiroEventType::UsageEvent => {
            // Send usage information
            if let Some(usage) = extract_usage(&payload_json) {
                let chunk = json!({
                    "id": chat_id,
                    "object": "chat.completion.chunk",
                    "created": created,
                    "model": model,
                    "choices": [],
                    "usage": usage
                });
                Some(format_sse_event("message", &chunk))
            } else {
                None
            }
        }

        _ => None,
    }
}

/// Extract text from delta payload
fn extract_delta_text(payload: &Value) -> String {
    // Try different payload structures
    if let Some(text) = payload["delta"]["text"].as_str() {
        return text.to_string();
    }
    if let Some(text) = payload["content"].as_str() {
        return text.to_string();
    }
    if let Some(text) = payload["text"].as_str() {
        return text.to_string();
    }
    String::new()
}

/// Extract usage information from payload
fn extract_usage(payload: &Value) -> Option<Value> {
    let input_tokens = payload["inputTokens"]
        .as_u64()
        .or_else(|| payload["input_tokens"].as_u64())?;
    let output_tokens = payload["outputTokens"]
        .as_u64()
        .or_else(|| payload["output_tokens"].as_u64())?;
    let total_tokens = input_tokens + output_tokens;

    Some(json!({
        "prompt_tokens": input_tokens,
        "completion_tokens": output_tokens,
        "total_tokens": total_tokens
    }))
}

/// Format a JSON value as SSE event
fn format_sse_event(event_type: &str, data: &Value) -> Bytes {
    let json_str = serde_json::to_string(data).unwrap_or_default();
    let sse = format!("event: {}\ndata: {}\n\n", event_type, json_str);
    Bytes::from(sse)
}
