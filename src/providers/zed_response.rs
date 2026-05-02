use anyhow::{anyhow, Result};
use serde_json::{json, Value};

/// Parses JSON Lines format chunks into individual JSON strings.
/// Skips empty lines and invalid JSON.
pub fn parse_jsonlines_chunk(chunk: &str) -> Vec<String> {
    chunk
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter(|line| serde_json::from_str::<Value>(line).is_ok())
        .map(|line| line.to_string())
        .collect()
}

/// Formats a JSON string as an SSE event.
/// Returns "data: {json}\n\n" format.
pub fn format_sse_event(json: &str) -> String {
    format!("data: {}\n\n", json)
}

fn parse_openai_like_response(response: &Value) -> Result<Value> {
    let obj = response
        .as_object()
        .ok_or_else(|| anyhow!("Response must be an object"))?;

    if !obj.contains_key("id") {
        return Err(anyhow!("Response missing required field: id"));
    }

    let choices = obj
        .get("choices")
        .ok_or_else(|| anyhow!("Response missing required field: choices"))?;

    let choices_array = choices
        .as_array()
        .ok_or_else(|| anyhow!("choices must be an array"))?;

    if choices_array.is_empty() {
        return Err(anyhow!("choices array must not be empty"));
    }

    Ok(response.clone())
}

fn convert_events_to_openai(response: &Value, fallback_model: Option<&str>) -> Result<Value> {
    let obj = response
        .as_object()
        .ok_or_else(|| anyhow!("Response must be an object"))?;
    let events = obj
        .get("events")
        .and_then(|value| value.as_array())
        .ok_or_else(|| anyhow!("Response missing required field: events"))?;
    if events.is_empty() {
        return Err(anyhow!("Response missing required field: events"));
    }

    let mut model = obj
        .get("model")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .or_else(|| fallback_model.map(str::to_string))
        .unwrap_or_default();
    if model.is_empty() {
        return Err(anyhow!("Response missing required field: model"));
    }

    let mut id = String::from("chatcmpl-zed");
    let mut text = String::new();
    let mut prompt_tokens = 0u64;
    let mut completion_tokens = 0u64;

    for event in events {
        let event = event
            .as_object()
            .ok_or_else(|| anyhow!("event must be an object"))?;
        let event_type = event
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or_default();

        match event_type {
            "message_start" => {
                if let Some(message) = event.get("message") {
                    if let Some(message_obj) = message.as_object() {
                        if let Some(event_id) =
                            message_obj.get("id").and_then(|value| value.as_str())
                        {
                            id = event_id.to_string();
                        }
                        if let Some(event_model) =
                            message_obj.get("model").and_then(|value| value.as_str())
                        {
                            model = event_model.to_string();
                        }
                        if let Some(usage) =
                            message_obj.get("usage").and_then(|value| value.as_object())
                        {
                            prompt_tokens = usage
                                .get("input_tokens")
                                .and_then(|value| value.as_u64())
                                .unwrap_or(prompt_tokens);
                            completion_tokens = usage
                                .get("output_tokens")
                                .and_then(|value| value.as_u64())
                                .unwrap_or(completion_tokens);
                        }
                    }
                }
            }
            "content_block_delta" => {
                if let Some(delta) = event.get("delta").and_then(|value| value.as_object()) {
                    match delta.get("type").and_then(|value| value.as_str()) {
                        Some("text_delta") => {
                            if let Some(chunk) = delta.get("text").and_then(|value| value.as_str())
                            {
                                text.push_str(chunk);
                            }
                        }
                        Some("thinking_delta") => {}
                        _ => {}
                    }
                }
            }
            "response.output_text.delta" => {
                if let Some(chunk) = event.get("delta").and_then(|value| value.as_str()) {
                    text.push_str(chunk);
                }
            }
            "response.completed" => {
                if let Some(response_obj) = event.get("response") {
                    if let Some(event_model) =
                        response_obj.get("model").and_then(|value| value.as_str())
                    {
                        model = event_model.to_string();
                    }
                    if let Some(usage) = response_obj.get("usage") {
                        prompt_tokens = usage
                            .get("input_tokens")
                            .and_then(|value| value.as_u64())
                            .unwrap_or(prompt_tokens);
                        completion_tokens = usage
                            .get("output_tokens")
                            .and_then(|value| value.as_u64())
                            .unwrap_or(completion_tokens);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(json!({
        "id": id,
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [
            {
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": text,
                },
                "finish_reason": "stop"
            }
        ],
        "usage": {
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "total_tokens": prompt_tokens + completion_tokens,
        }
    }))
}

fn parse_jsonlines_to_events(body: &str, fallback_model: Option<&str>) -> Result<Value> {
    let events = body
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(serde_json::from_str::<Value>(trimmed))
            }
        })
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| anyhow!("parse jsonlines response: {e}"))?;

    convert_events_to_openai(
        &json!({
            "model": fallback_model,
            "events": events,
        }),
        fallback_model,
    )
}

/// Parses and validates Zed API response.
pub fn parse_zed_response(response: &Value) -> Result<Value> {
    parse_zed_response_with_model(response, None)
}

pub fn parse_zed_response_with_model(
    response: &Value,
    fallback_model: Option<&str>,
) -> Result<Value> {
    if let Some(raw_body) = response.as_str() {
        return parse_jsonlines_to_events(raw_body, fallback_model);
    }

    if response
        .as_object()
        .and_then(|obj| obj.get("events"))
        .and_then(|value| value.as_array())
        .is_some()
    {
        return convert_events_to_openai(response, fallback_model);
    }

    parse_openai_like_response(response)
}

/// Extracts content from the first choice's message in a Zed API response
pub fn extract_content(response: &serde_json::Value) -> anyhow::Result<String> {
    let choices = response
        .get("choices")
        .and_then(|c| c.as_array())
        .ok_or_else(|| anyhow!("Response missing choices array"))?;

    if choices.is_empty() {
        return Err(anyhow!("choices array is empty"));
    }

    let content = choices[0]
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| anyhow!("Failed to extract content from message"))?;

    Ok(content.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_jsonlines_chunk_single_line() {
        let chunk = r#"{"id":"1","choices":[{"delta":{"content":"Hello"}}]}"#;
        let result = parse_jsonlines_chunk(chunk);

        assert_eq!(result.len(), 1);
        assert!(result[0].contains("\"id\":\"1\""));
        assert!(result[0].contains("\"content\":\"Hello\""));
    }

    #[test]
    fn test_parse_jsonlines_chunk_multiple_lines() {
        let chunk = r#"{"id":"1","choices":[{"delta":{"content":"Hello"}}]}
{"id":"2","choices":[{"delta":{"content":" World"}}]}"#;
        let result = parse_jsonlines_chunk(chunk);

        assert_eq!(result.len(), 2);
        assert!(result[0].contains("\"id\":\"1\""));
        assert!(result[1].contains("\"id\":\"2\""));
    }

    #[test]
    fn test_parse_jsonlines_chunk_with_empty_lines() {
        let chunk = r#"{"id":"1","choices":[{"delta":{"content":"Hello"}}]}

{"id":"2","choices":[{"delta":{"content":" World"}}]}"#;
        let result = parse_jsonlines_chunk(chunk);

        assert_eq!(result.len(), 2);
        assert!(result[0].contains("\"id\":\"1\""));
        assert!(result[1].contains("\"id\":\"2\""));
    }

    #[test]
    fn test_parse_jsonlines_chunk_empty_input() {
        let chunk = "";
        let result = parse_jsonlines_chunk(chunk);

        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_parse_jsonlines_chunk_invalid_json() {
        let chunk = r#"{"id":"1","invalid
{"id":"2","choices":[{"delta":{"content":"Valid"}}]}"#;
        let result = parse_jsonlines_chunk(chunk);

        assert_eq!(result.len(), 1);
        assert!(result[0].contains("\"id\":\"2\""));
    }

    #[test]
    fn test_format_sse_event_basic() {
        let json = r#"{"id":"chatcmpl-123","choices":[{"delta":{"content":"Hello"}}]}"#;

        let result = format_sse_event(json);
        assert_eq!(result, format!("data: {}\n\n", json));
    }

    #[test]
    fn test_format_sse_event_done() {
        let json = r#"{"id":"chatcmpl-123","choices":[{"delta":{},"finish_reason":"stop"}]}"#;

        let result = format_sse_event(json);
        assert!(result.starts_with("data: "));
        assert!(result.ends_with("\n\n"));
        assert!(result.contains("finish_reason"));
    }

    #[test]
    fn test_format_sse_event_with_usage() {
        let json = r#"{"id":"chatcmpl-123","choices":[{"delta":{"content":"Hello"}}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#;

        let result = format_sse_event(json);
        assert!(result.contains("usage"));
        assert!(result.contains("prompt_tokens"));
    }

    #[test]
    fn test_valid_response() {
        let response = json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1234567890,
            "model": "anthropic/claude-3-5-sonnet-20241022",
            "choices": [
                {
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "Hello!"
                    },
                    "finish_reason": "stop"
                }
            ]
        });

        let result = parse_zed_response(&response);
        assert!(result.is_ok());
    }

    #[test]
    fn test_missing_id() {
        let response = json!({
            "choices": [{"message": {"content": "Hello"}}]
        });

        let result = parse_zed_response(&response);
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_choices() {
        let response = json!({
            "id": "chatcmpl-123"
        });

        let result = parse_zed_response(&response);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_choices() {
        let response = json!({
            "id": "chatcmpl-123",
            "choices": []
        });

        let result = parse_zed_response(&response);
        assert!(result.is_err());
    }

    #[test]
    fn test_events_response_rejects_empty_events() {
        let response = json!({
            "model": "claude-sonnet-4-5",
            "events": []
        });

        let result = parse_zed_response(&response);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Response missing required field: events"));
    }

    #[test]
    fn test_events_response_rejects_missing_model_without_fallback() {
        let response = json!({
            "events": [
                {
                    "type": "response.output_text.delta",
                    "delta": "Hello"
                }
            ]
        });

        let result = parse_zed_response(&response);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Response missing required field: model"));
    }

    #[test]
    fn test_jsonlines_response_rejects_missing_model_without_fallback() {
        let response =
            Value::String(r#"{"type":"response.output_text.delta","delta":"Hello"}"#.to_string());

        let result = parse_zed_response(&response);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Response missing required field: model"));
    }
}
