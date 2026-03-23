use anyhow::{anyhow, Result};
use serde_json::{json, Value};

/// Converts Anthropic Messages API response to OpenAI format
/// Preserves thinking blocks in metadata
pub fn convert_anthropic_to_openai(anthropic_response: &Value) -> Result<Value> {
    let obj = anthropic_response
        .as_object()
        .ok_or_else(|| anyhow!("Response must be an object"))?;

    let id = obj
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing id field"))?;

    let model = obj
        .get("model")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing model field"))?;

    let content = obj
        .get("content")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("Missing or invalid content field"))?;

    let stop_reason = obj
        .get("stop_reason")
        .and_then(|v| v.as_str())
        .unwrap_or("stop");

    // Extract text and thinking blocks
    let mut text_parts = Vec::new();
    let mut thinking_text = None;

    for block in content {
        if let Some(block_type) = block.get("type").and_then(|v| v.as_str()) {
            match block_type {
                "text" => {
                    if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                        text_parts.push(text);
                    }
                }
                "thinking" => {
                    if let Some(thinking) = block.get("thinking").and_then(|v| v.as_str()) {
                        thinking_text = Some(thinking);
                    }
                }
                _ => {}
            }
        }
    }

    let combined_text = text_parts.join("");

    // Build message object
    let mut message = json!({
        "role": "assistant",
        "content": combined_text
    });

    // Add thinking if present
    if let Some(thinking) = thinking_text {
        message["thinking"] = Value::String(thinking.to_string());
    }

    // Map stop_reason
    let finish_reason = match stop_reason {
        "end_turn" => "stop",
        "max_tokens" => "length",
        "stop_sequence" => "stop",
        _ => "stop",
    };

    // Build usage
    let mut usage = json!({
        "prompt_tokens": 0,
        "completion_tokens": 0,
        "total_tokens": 0
    });

    if let Some(anthropic_usage) = obj.get("usage") {
        let input_tokens = anthropic_usage
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let output_tokens = anthropic_usage
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        usage = json!({
            "prompt_tokens": input_tokens,
            "completion_tokens": output_tokens,
            "total_tokens": input_tokens + output_tokens
        });
    }

    Ok(json!({
        "id": id,
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [
            {
                "index": 0,
                "message": message,
                "finish_reason": finish_reason
            }
        ],
        "usage": usage
    }))
}

/// Converts OpenAI format request to Anthropic Messages API format
pub fn convert_openai_to_anthropic(openai_request: &Value) -> Result<Value> {
    let obj = openai_request
        .as_object()
        .ok_or_else(|| anyhow!("Request must be an object"))?;

    let model = obj
        .get("model")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing model field"))?;

    let messages = obj
        .get("messages")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("Missing or invalid messages field"))?;

    let mut anthropic_messages = Vec::new();
    let mut system_message = None;

    // Separate system messages from conversation messages
    for msg in messages {
        if let Some(role) = msg.get("role").and_then(|v| v.as_str()) {
            if role == "system" {
                if let Some(content) = msg.get("content").and_then(|v| v.as_str()) {
                    system_message = Some(content.to_string());
                }
            } else {
                anthropic_messages.push(msg.clone());
            }
        }
    }

    let mut anthropic_request = json!({
        "model": model,
        "messages": anthropic_messages
    });

    // Add system message if present
    if let Some(system) = system_message {
        anthropic_request["system"] = Value::String(system);
    }

    // Copy optional fields
    if let Some(max_tokens) = obj.get("max_tokens") {
        anthropic_request["max_tokens"] = max_tokens.clone();
    }
    if let Some(temperature) = obj.get("temperature") {
        anthropic_request["temperature"] = temperature.clone();
    }
    if let Some(top_p) = obj.get("top_p") {
        anthropic_request["top_p"] = top_p.clone();
    }
    if let Some(stop) = obj.get("stop") {
        anthropic_request["stop_sequences"] = stop.clone();
    }

    Ok(anthropic_request)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_anthropic_to_openai() {
        let response = json!({
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Hello!"}
            ],
            "model": "claude-3-5-sonnet-20241022",
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 20
            }
        });

        let result = convert_anthropic_to_openai(&response).unwrap();
        assert_eq!(result["id"], "msg_123");
        assert_eq!(result["choices"][0]["message"]["content"], "Hello!");
        assert_eq!(result["usage"]["prompt_tokens"], 10);
        assert_eq!(result["usage"]["completion_tokens"], 20);
    }

    #[test]
    fn test_convert_openai_to_anthropic() {
        let request = json!({
            "model": "claude-3-5-sonnet-20241022",
            "messages": [
                {"role": "user", "content": "Hello!"}
            ],
            "max_tokens": 1000
        });

        let result = convert_openai_to_anthropic(&request).unwrap();
        assert_eq!(result["model"], "claude-3-5-sonnet-20241022");
        assert_eq!(result["max_tokens"], 1000);
    }
}
