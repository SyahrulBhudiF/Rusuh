//! KIRO request/response translator — OpenAI ↔ KIRO (Claude format) conversion.
//!
//! KIRO uses Claude-compatible request/response format internally.
//! This module translates between OpenAI and KIRO formats.

use bytes::Bytes;
use serde_json::{json, Value};

use crate::models::{ChatCompletionRequest, ChatMessage, MessageContent};

// ── Request Translation ──────────────────────────────────────────────────────

/// Translate OpenAI ChatCompletionRequest to KIRO (Claude) format.
///
/// KIRO expects:
/// ```json
/// {
///   "messages": [
///     {"role": "user", "content": "..."},
///     {"role": "assistant", "content": "..."}
///   ],
///   "system": "system prompt",
///   "max_tokens": 4096,
///   "temperature": 0.7,
///   "anthropic_version": "bedrock-2023-05-31"
/// }
/// ```
pub fn translate_request_to_kiro(req: &ChatCompletionRequest) -> Value {
    let mut messages = Vec::new();
    let mut system_prompt = String::new();

    // Separate system messages from conversation
    for msg in &req.messages {
        match msg.role.as_str() {
            "system" => {
                let text = extract_text_content(&msg.content);
                if !text.is_empty() {
                    if !system_prompt.is_empty() {
                        system_prompt.push_str("\n\n");
                    }
                    system_prompt.push_str(&text);
                }
            }
            "user" | "assistant" => {
                messages.push(translate_message_to_kiro(msg));
            }
            _ => {
                // Skip unknown roles
            }
        }
    }

    let mut request = json!({
        "messages": messages,
        "anthropic_version": "bedrock-2023-05-31",
    });

    // Add system prompt if present
    if !system_prompt.is_empty() {
        request["system"] = json!(system_prompt);
    }

    // Add generation parameters
    if let Some(max_tokens) = req.max_tokens {
        request["max_tokens"] = json!(max_tokens);
    } else {
        // KIRO requires max_tokens
        request["max_tokens"] = json!(4096);
    }

    if let Some(temp) = req.temperature {
        request["temperature"] = json!(temp);
    }

    if let Some(top_p) = req.top_p {
        request["top_p"] = json!(top_p);
    }

    if let Some(stop) = &req.stop {
        request["stop_sequences"] = json!(stop);
    }

    // Add tools if present
    if let Some(tools) = &req.tools {
        let kiro_tools = translate_tools_to_kiro(tools);
        if !kiro_tools.is_empty() {
            request["tools"] = json!(kiro_tools);
        }
    }

    request
}

/// Translate a single message to KIRO format
fn translate_message_to_kiro(msg: &ChatMessage) -> Value {
    let content = match &msg.content {
        MessageContent::Text(text) => json!(text),
        MessageContent::Parts(parts) => {
            let kiro_parts: Vec<Value> = parts
                .iter()
                .filter_map(|part| {
                    if let Some(text) = &part.text {
                        Some(json!({
                            "type": "text",
                            "text": text
                        }))
                    } else if let Some(img) = &part.image_url {
                        // KIRO supports image content
                        Some(json!({
                            "type": "image",
                            "source": {
                                "type": "url",
                                "url": img.url
                            }
                        }))
                    } else {
                        None
                    }
                })
                .collect();

            if kiro_parts.len() == 1 {
                // Single part - unwrap
                kiro_parts[0]["text"].clone()
            } else {
                json!(kiro_parts)
            }
        }
    };

    json!({
        "role": msg.role,
        "content": content
    })
}

/// Translate OpenAI tools to KIRO format
fn translate_tools_to_kiro(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|tool| {
            if tool["type"].as_str() == Some("function") {
                let func = &tool["function"];
                Some(json!({
                    "name": func["name"],
                    "description": func.get("description").unwrap_or(&json!("")),
                    "input_schema": func.get("parameters").unwrap_or(&json!({}))
                }))
            } else {
                None
            }
        })
        .collect()
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
    pub fn from_str(s: &str) -> Self {
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
    let event = KiroEventType::from_str(event_type);

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
