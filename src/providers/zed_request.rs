use anyhow::{anyhow, Result};
use serde_json::{json, Map, Value};
use uuid::Uuid;

/// Normalizes model names for Zed API
/// Maps model prefixes to Zed's expected format:
/// - claude-* -> anthropic/
/// - gpt-* -> open_ai/
/// - gemini-* -> google/
/// - grok-* -> x_ai/
pub fn normalize_model_for_zed(model: &str) -> String {
    // If already prefixed, return as-is
    if model.contains('/') {
        return model.to_string();
    }

    if model.starts_with("claude-") {
        format!("anthropic/{}", model)
    } else if model.starts_with("gpt-") {
        format!("open_ai/{}", model)
    } else if model.starts_with("gemini-") {
        format!("google/{}", model)
    } else if model.starts_with("grok-") {
        format!("x_ai/{}", model)
    } else {
        model.to_string()
    }
}

fn split_provider_model(model: &str) -> Option<(&str, &str)> {
    let (provider, model_id) = model.split_once('/')?;
    match provider {
        "anthropic" | "open_ai" | "google" | "x_ai" if !model_id.is_empty() => {
            Some((provider, model_id))
        }
        _ => None,
    }
}

fn provider_and_model_for_zed(model: &str) -> (String, String) {
    if let Some((provider, model_id)) = split_provider_model(model) {
        return (provider.to_string(), model_id.to_string());
    }

    if model.starts_with("claude-") {
        ("anthropic".into(), model.to_string())
    } else if model.starts_with("gpt-") {
        ("open_ai".into(), model.to_string())
    } else if model.starts_with("gemini-") {
        ("google".into(), model.to_string())
    } else if model.starts_with("grok-") {
        ("x_ai".into(), model.to_string())
    } else {
        ("anthropic".into(), model.to_string())
    }
}

fn normalize_text_parts(content: &Value, part_type: &str) -> Vec<Value> {
    match content {
        Value::String(text) => vec![json!({ "type": part_type, "text": text })],
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                part.as_object()
                    .and_then(|obj| obj.get("text"))
                    .and_then(|value| value.as_str())
                    .map(|text| json!({ "type": part_type, "text": text }))
            })
            .collect(),
        _ => vec![],
    }
}

fn build_anthropic_message(message: &Value) -> Option<Value> {
    let obj = message.as_object()?;
    let role = obj.get("role")?.as_str()?;
    let content = obj.get("content").unwrap_or(&Value::Null);

    Some(json!({
        "role": role,
        "content": normalize_text_parts(content, "text")
    }))
}

fn build_openai_input_item(message: &Value) -> Option<Value> {
    let obj = message.as_object()?;
    let role = match obj.get("role")?.as_str()? {
        "developer" => "system",
        other => other,
    };
    let part_type = if role == "assistant" {
        "output_text"
    } else {
        "input_text"
    };

    Some(json!({
        "type": "message",
        "role": role,
        "content": normalize_text_parts(obj.get("content").unwrap_or(&Value::Null), part_type)
    }))
}

fn copy_if_present(
    target: &mut Map<String, Value>,
    source: &Map<String, Value>,
    source_key: &str,
    target_key: &str,
) {
    if let Some(value) = source.get(source_key) {
        target.insert(target_key.to_string(), value.clone());
    }
}

fn build_anthropic_provider_request(obj: &Map<String, Value>, model_id: &str) -> Value {
    let messages = obj
        .get("messages")
        .and_then(|value| value.as_array())
        .map(|messages| {
            messages
                .iter()
                .filter_map(build_anthropic_message)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut provider_request = Map::new();
    provider_request.insert("model".to_string(), Value::String(model_id.to_string()));
    provider_request.insert("messages".to_string(), Value::Array(messages));
    provider_request.insert(
        "max_tokens".to_string(),
        obj.get("max_tokens")
            .cloned()
            .unwrap_or_else(|| json!(8192)),
    );
    copy_if_present(&mut provider_request, obj, "stream", "stream");
    copy_if_present(&mut provider_request, obj, "temperature", "temperature");
    copy_if_present(&mut provider_request, obj, "tools", "tools");
    copy_if_present(&mut provider_request, obj, "tool_choice", "tool_choice");

    Value::Object(provider_request)
}

fn build_openai_provider_request(obj: &Map<String, Value>, model_id: &str) -> Value {
    let input = obj
        .get("messages")
        .and_then(|value| value.as_array())
        .map(|messages| {
            messages
                .iter()
                .filter_map(build_openai_input_item)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut provider_request = Map::new();
    provider_request.insert("model".to_string(), Value::String(model_id.to_string()));
    provider_request.insert("input".to_string(), Value::Array(input));
    copy_if_present(&mut provider_request, obj, "stream", "stream");
    copy_if_present(&mut provider_request, obj, "temperature", "temperature");
    copy_if_present(&mut provider_request, obj, "top_p", "top_p");

    if let Some(value) = obj
        .get("max_output_tokens")
        .or_else(|| obj.get("max_tokens"))
    {
        provider_request.insert("max_output_tokens".to_string(), value.clone());
    }

    Value::Object(provider_request)
}

fn build_generic_provider_request(
    obj: &Map<String, Value>,
    provider: &str,
    model_id: &str,
) -> Value {
    let supported_fields = [
        "messages",
        "temperature",
        "max_tokens",
        "top_p",
        "stream",
        "stop",
        "presence_penalty",
        "frequency_penalty",
    ];

    let mut provider_request = Map::new();
    let provider_model = if provider == "google" {
        format!("models/{model_id}")
    } else {
        model_id.to_string()
    };
    provider_request.insert("model".to_string(), Value::String(provider_model));

    for field in supported_fields {
        if let Some(value) = obj.get(field) {
            provider_request.insert(field.to_string(), value.clone());
        }
    }

    Value::Object(provider_request)
}

/// Translates OpenAI-format request to Zed completions format.
pub fn translate_to_zed_request(openai_request: &Value) -> Result<Value> {
    let obj = openai_request
        .as_object()
        .ok_or_else(|| anyhow!("Request must be an object"))?;

    let model = obj
        .get("model")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing required field: model"))?;

    let messages = obj
        .get("messages")
        .ok_or_else(|| anyhow!("Missing required field: messages"))?;

    if !messages.is_array() {
        return Err(anyhow!("messages must be an array"));
    }

    let (provider, model_id) = provider_and_model_for_zed(model);
    let provider_request = match provider.as_str() {
        "anthropic" => build_anthropic_provider_request(obj, &model_id),
        "open_ai" => build_openai_provider_request(obj, &model_id),
        _ => build_generic_provider_request(obj, &provider, &model_id),
    };

    Ok(json!({
        "thread_id": Uuid::new_v4().to_string(),
        "prompt_id": Uuid::new_v4().to_string(),
        "intent": "user_prompt",
        "provider": provider,
        "model": model_id,
        "provider_request": provider_request,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_normalize_model() {
        assert_eq!(
            normalize_model_for_zed("claude-3-5-sonnet-20241022"),
            "anthropic/claude-3-5-sonnet-20241022"
        );
        assert_eq!(normalize_model_for_zed("gpt-4o"), "open_ai/gpt-4o");
        assert_eq!(
            normalize_model_for_zed("gemini-1.5-pro"),
            "google/gemini-1.5-pro"
        );
        assert_eq!(normalize_model_for_zed("grok-2"), "x_ai/grok-2");
        assert_eq!(
            normalize_model_for_zed("anthropic/claude-3-5-sonnet-20241022"),
            "anthropic/claude-3-5-sonnet-20241022"
        );
        assert_eq!(normalize_model_for_zed("unknown"), "unknown");
    }

    #[test]
    fn test_translate_request() {
        let request = json!({
            "model": "claude-3-5-sonnet-20241022",
            "messages": [{"role": "user", "content": "Hello"}],
            "temperature": 0.7,
            "max_tokens": 1000,
            "unsupported_field": "removed"
        });

        let result = translate_to_zed_request(&request).unwrap();
        assert_eq!(result["provider"], "anthropic");
        assert_eq!(result["model"], "claude-3-5-sonnet-20241022");
        assert_eq!(
            result["provider_request"]["model"],
            "claude-3-5-sonnet-20241022"
        );
        assert_eq!(
            result["provider_request"]["messages"][0]["content"][0]["text"],
            "Hello"
        );
        assert_eq!(result["provider_request"]["temperature"], 0.7);
        assert!(!result["provider_request"]
            .as_object()
            .unwrap()
            .contains_key("unsupported_field"));
    }
}
