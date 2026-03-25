use rusuh::providers::zed_request::{normalize_model_for_zed, translate_to_zed_request};
use serde_json::json;
use uuid::Uuid;

#[test]
fn test_normalize_model_claude() {
    assert_eq!(
        normalize_model_for_zed("claude-3-5-sonnet-20241022"),
        "anthropic/claude-3-5-sonnet-20241022"
    );
    assert_eq!(
        normalize_model_for_zed("claude-opus-4"),
        "anthropic/claude-opus-4"
    );
}

#[test]
fn test_normalize_model_gpt() {
    assert_eq!(normalize_model_for_zed("gpt-4o"), "open_ai/gpt-4o");
    assert_eq!(
        normalize_model_for_zed("gpt-4-turbo"),
        "open_ai/gpt-4-turbo"
    );
}

#[test]
fn test_normalize_model_gemini() {
    assert_eq!(
        normalize_model_for_zed("gemini-1.5-pro"),
        "google/gemini-1.5-pro"
    );
    assert_eq!(
        normalize_model_for_zed("gemini-2.0-flash"),
        "google/gemini-2.0-flash"
    );
}

#[test]
fn test_normalize_model_grok() {
    assert_eq!(normalize_model_for_zed("grok-2"), "x_ai/grok-2");
    assert_eq!(normalize_model_for_zed("grok-beta"), "x_ai/grok-beta");
}

#[test]
fn test_normalize_model_already_prefixed() {
    assert_eq!(
        normalize_model_for_zed("anthropic/claude-3-5-sonnet-20241022"),
        "anthropic/claude-3-5-sonnet-20241022"
    );
    assert_eq!(normalize_model_for_zed("open_ai/gpt-4o"), "open_ai/gpt-4o");
}

#[test]
fn test_normalize_model_unknown() {
    assert_eq!(normalize_model_for_zed("unknown-model"), "unknown-model");
}

#[test]
fn test_translate_to_zed_request_basic() {
    let openai_request = json!({
        "model": "claude-3-5-sonnet-20241022",
        "messages": [
            {"role": "user", "content": "Hello"}
        ],
        "temperature": 0.7,
        "max_tokens": 1000
    });

    let result = translate_to_zed_request(&openai_request).unwrap();

    assert_eq!(result["provider"], "anthropic");
    assert_eq!(result["model"], "claude-3-5-sonnet-20241022");
    assert_eq!(result["provider_request"]["messages"][0]["role"], "user");
    assert_eq!(
        result["provider_request"]["messages"][0]["content"][0]["text"],
        "Hello"
    );
    assert_eq!(result["provider_request"]["temperature"], 0.7);
    assert_eq!(result["provider_request"]["max_tokens"], 1000);
}

#[test]
fn test_translate_to_zed_request_strips_unsupported_fields() {
    let openai_request = json!({
        "model": "gpt-4o",
        "messages": [
            {"role": "user", "content": "Hello"}
        ],
        "temperature": 0.7,
        "max_tokens": 1000,
        "unsupported_field": "should be removed",
        "another_unsupported": 123
    });

    let result = translate_to_zed_request(&openai_request).unwrap();

    assert!(!result
        .as_object()
        .unwrap()
        .contains_key("unsupported_field"));
    assert!(!result
        .as_object()
        .unwrap()
        .contains_key("another_unsupported"));
    assert_eq!(result["provider"], "open_ai");
    assert_eq!(result["model"], "gpt-4o");
}

#[test]
fn test_translate_to_zed_request_with_stream() {
    let openai_request = json!({
        "model": "claude-3-5-sonnet-20241022",
        "messages": [
            {"role": "user", "content": "Hello"}
        ],
        "stream": true
    });

    let result = translate_to_zed_request(&openai_request).unwrap();

    assert_eq!(result["provider_request"]["stream"], true);
}

#[test]
fn test_translate_to_zed_request_wraps_provider_request_for_claude() {
    let openai_request = json!({
        "model": "claude-sonnet-4-5",
        "messages": [
            {"role": "user", "content": "Reply with exactly hi"}
        ],
        "temperature": 0.2,
        "max_tokens": 16
    });

    let result = translate_to_zed_request(&openai_request).unwrap();
    let obj = result.as_object().unwrap();

    let thread_id = obj.get("thread_id").and_then(|v| v.as_str()).unwrap();
    let prompt_id = obj.get("prompt_id").and_then(|v| v.as_str()).unwrap();
    assert!(Uuid::parse_str(thread_id).is_ok());
    assert!(Uuid::parse_str(prompt_id).is_ok());
    assert_eq!(result["intent"], "user_prompt");
    assert_eq!(result["provider"], "anthropic");
    assert_eq!(result["model"], "claude-sonnet-4-5");
    assert_eq!(result["provider_request"]["model"], "claude-sonnet-4-5");
    assert_eq!(result["provider_request"]["messages"][0]["role"], "user");
    assert_eq!(
        result["provider_request"]["messages"][0]["content"][0]["text"],
        "Reply with exactly hi"
    );
    assert_eq!(result["provider_request"]["temperature"], 0.2);
    assert_eq!(result["provider_request"]["max_tokens"], 16);
    assert!(result["provider_request"]
        .get("unsupported_field")
        .is_none());
}

#[test]
fn test_translate_to_zed_request_wraps_provider_request_for_openai() {
    let openai_request = json!({
        "model": "gpt-5.4",
        "messages": [
            {"role": "system", "content": "You are concise."},
            {"role": "user", "content": "Hello"},
            {"role": "assistant", "content": "Hi"}
        ],
        "temperature": 0.7,
        "top_p": 0.9,
        "stream": false
    });

    let result = translate_to_zed_request(&openai_request).unwrap();

    assert_eq!(result["provider"], "open_ai");
    assert_eq!(result["model"], "gpt-5.4");
    assert_eq!(result["provider_request"]["model"], "gpt-5.4");
    assert_eq!(result["provider_request"]["stream"], false);
    assert_eq!(result["provider_request"]["temperature"], 0.7);
    assert_eq!(result["provider_request"]["top_p"], 0.9);
    assert_eq!(result["provider_request"]["input"][0]["role"], "system");
    assert_eq!(result["provider_request"]["input"][0]["type"], "message");
    assert_eq!(
        result["provider_request"]["input"][0]["content"][0]["type"],
        "input_text"
    );
    assert_eq!(
        result["provider_request"]["input"][0]["content"][0]["text"],
        "You are concise."
    );
    assert_eq!(result["provider_request"]["input"][2]["role"], "assistant");
    assert_eq!(
        result["provider_request"]["input"][2]["content"][0]["type"],
        "output_text"
    );
    assert_eq!(
        result["provider_request"]["input"][2]["content"][0]["text"],
        "Hi"
    );
}

#[test]
fn test_translate_to_zed_request_missing_model() {
    let openai_request = json!({
        "messages": [
            {"role": "user", "content": "Hello"}
        ]
    });

    let result = translate_to_zed_request(&openai_request);
    assert!(result.is_err());
}

#[test]
fn test_translate_to_zed_request_missing_messages() {
    let openai_request = json!({
        "model": "claude-3-5-sonnet-20241022"
    });

    let result = translate_to_zed_request(&openai_request);
    assert!(result.is_err());
}
