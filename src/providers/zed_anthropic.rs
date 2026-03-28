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

    let max_tokens = obj
        .get("max_tokens")
        .ok_or_else(|| anyhow!("Missing required field: max_tokens"))?;
    let max_tokens = max_tokens
        .as_u64()
        .ok_or_else(|| anyhow!("max_tokens must be a positive integer"))?;
    if max_tokens == 0 {
        return Err(anyhow!("max_tokens must be greater than 0"));
    }

    let mut anthropic_messages = Vec::new();
    let mut leading_system_messages = Vec::new();
    let mut seen_non_system = false;

    // Collect leading system messages, then preserve conversation order.
    for msg in messages {
        if let Some(role) = msg.get("role").and_then(|v| v.as_str()) {
            if role == "system" {
                if seen_non_system {
                    return Err(anyhow!(
                        "System messages are only supported at the start of the conversation"
                    ));
                }

                let content = msg
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("System message content must be a string"))?;
                leading_system_messages.push(content.to_string());
                continue;
            }

            seen_non_system = true;
            anthropic_messages.push(msg.clone());
        }
    }

    let mut anthropic_request = json!({
        "model": model,
        "messages": anthropic_messages
    });

    if !leading_system_messages.is_empty() {
        anthropic_request["system"] = Value::String(leading_system_messages.join("\n\n"));
    }

    anthropic_request["max_tokens"] = Value::Number(max_tokens.into());

    // Copy optional fields
    if let Some(temperature) = obj.get("temperature") {
        anthropic_request["temperature"] = temperature.clone();
    }
    if let Some(top_p) = obj.get("top_p") {
        anthropic_request["top_p"] = top_p.clone();
    }
    if let Some(stop) = obj.get("stop") {
        let stop_sequences = match stop {
            Value::String(text) => vec![Value::String(text.clone())],
            Value::Array(items) => items
                .iter()
                .filter_map(|item| match item {
                    Value::String(text) => Some(Value::String(text.clone())),
                    Value::Number(_) | Value::Bool(_) | Value::Null => {
                        Some(Value::String(item.to_string()))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>(),
            Value::Number(_) | Value::Bool(_) | Value::Null => {
                vec![Value::String(stop.to_string())]
            }
            _ => Vec::new(),
        };
        anthropic_request["stop_sequences"] = Value::Array(stop_sequences);
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

    #[test]
    fn test_convert_openai_to_anthropic_with_multiple_leading_system_messages() {
        let request = json!({
            "model": "claude-3-5-sonnet-20241022",
            "messages": [
                {"role": "system", "content": "Instruction 1"},
                {"role": "system", "content": "Instruction 2"},
                {"role": "user", "content": "Hello!"},
                {"role": "assistant", "content": "Hi"}
            ],
            "max_tokens": 1000
        });

        let result = convert_openai_to_anthropic(&request).unwrap();
        assert_eq!(result["system"], "Instruction 1\n\nInstruction 2");
        assert_eq!(result["messages"][0]["role"], "user");
        assert_eq!(result["messages"][1]["role"], "assistant");
    }

    #[test]
    fn test_convert_openai_to_anthropic_rejects_late_system_messages() {
        let request = json!({
            "model": "claude-3-5-sonnet-20241022",
            "messages": [
                {"role": "system", "content": "Instruction 1"},
                {"role": "user", "content": "Hello!"},
                {"role": "system", "content": "Late instruction"}
            ],
            "max_tokens": 1000
        });

        let err = convert_openai_to_anthropic(&request).unwrap_err();
        assert!(err
            .to_string()
            .contains("System messages are only supported at the start of the conversation"));
    }

    #[test]
    fn test_convert_openai_to_anthropic_rejects_missing_max_tokens() {
        let request = json!({
            "model": "claude-3-5-sonnet-20241022",
            "messages": [
                {"role": "user", "content": "Hello!"}
            ]
        });

        let err = convert_openai_to_anthropic(&request).unwrap_err();
        assert!(err
            .to_string()
            .contains("Missing required field: max_tokens"));
    }

    #[test]
    fn test_convert_openai_to_anthropic_rejects_non_numeric_max_tokens() {
        let request = json!({
            "model": "claude-3-5-sonnet-20241022",
            "messages": [
                {"role": "user", "content": "Hello!"}
            ],
            "max_tokens": "1000"
        });

        let err = convert_openai_to_anthropic(&request).unwrap_err();
        assert!(err
            .to_string()
            .contains("max_tokens must be a positive integer"));
    }

    #[test]
    fn test_convert_openai_to_anthropic_rejects_zero_max_tokens() {
        let request = json!({
            "model": "claude-3-5-sonnet-20241022",
            "messages": [
                {"role": "user", "content": "Hello!"}
            ],
            "max_tokens": 0
        });

        let err = convert_openai_to_anthropic(&request).unwrap_err();
        assert!(err
            .to_string()
            .contains("max_tokens must be greater than 0"));
    }

    #[test]
    fn test_convert_openai_to_anthropic_normalizes_string_stop() {
        let request = json!({
            "model": "claude-3-5-sonnet-20241022",
            "messages": [
                {"role": "user", "content": "Hello!"}
            ],
            "max_tokens": 1000,
            "stop": "END"
        });

        let result = convert_openai_to_anthropic(&request).unwrap();
        assert_eq!(result["stop_sequences"], json!(["END"]));
    }

    #[test]
    fn test_convert_openai_to_anthropic_normalizes_array_stop_with_scalars() {
        let request = json!({
            "model": "claude-3-5-sonnet-20241022",
            "messages": [
                {"role": "user", "content": "Hello!"}
            ],
            "max_tokens": 1000,
            "stop": ["END", 42, true, null, {"ignored": true}]
        });

        let result = convert_openai_to_anthropic(&request).unwrap();
        assert_eq!(
            result["stop_sequences"],
            json!(["END", "42", "true", "null"])
        );
    }
}
