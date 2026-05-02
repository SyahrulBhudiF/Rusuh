use rusuh::providers::zed_anthropic::{convert_anthropic_to_openai, convert_openai_to_anthropic};
use serde_json::json;

#[test]
fn test_convert_anthropic_to_openai_basic() {
    let anthropic_response = json!({
        "id": "msg_123",
        "type": "message",
        "role": "assistant",
        "content": [
            {
                "type": "text",
                "text": "Hello! How can I help you?"
            }
        ],
        "model": "claude-3-5-sonnet-20241022",
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 10,
            "output_tokens": 20
        }
    });

    let result = convert_anthropic_to_openai(&anthropic_response).unwrap();

    assert_eq!(result["id"], "msg_123");
    assert_eq!(result["object"], "chat.completion");
    assert_eq!(result["model"], "claude-3-5-sonnet-20241022");
    assert_eq!(result["choices"][0]["message"]["role"], "assistant");
    assert_eq!(
        result["choices"][0]["message"]["content"],
        "Hello! How can I help you?"
    );
    assert_eq!(result["choices"][0]["finish_reason"], "stop");
    assert_eq!(result["usage"]["prompt_tokens"], 10);
    assert_eq!(result["usage"]["completion_tokens"], 20);
    assert_eq!(result["usage"]["total_tokens"], 30);
}

#[test]
fn test_convert_anthropic_to_openai_with_thinking() {
    let anthropic_response = json!({
        "id": "msg_123",
        "type": "message",
        "role": "assistant",
        "content": [
            {
                "type": "thinking",
                "thinking": "Let me think about this..."
            },
            {
                "type": "text",
                "text": "Here's my answer."
            }
        ],
        "model": "claude-3-5-sonnet-20241022",
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 10,
            "output_tokens": 20
        }
    });

    let result = convert_anthropic_to_openai(&anthropic_response).unwrap();

    assert_eq!(
        result["choices"][0]["message"]["content"],
        "Here's my answer."
    );
    // Thinking block should be preserved in metadata
    assert!(result["choices"][0]["message"]["thinking"].is_string());
    assert_eq!(
        result["choices"][0]["message"]["thinking"],
        "Let me think about this..."
    );
}

#[test]
fn test_convert_anthropic_to_openai_multiple_text_blocks() {
    let anthropic_response = json!({
        "id": "msg_123",
        "type": "message",
        "role": "assistant",
        "content": [
            {
                "type": "text",
                "text": "First part. "
            },
            {
                "type": "text",
                "text": "Second part."
            }
        ],
        "model": "claude-3-5-sonnet-20241022",
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 10,
            "output_tokens": 20
        }
    });

    let result = convert_anthropic_to_openai(&anthropic_response).unwrap();

    assert_eq!(
        result["choices"][0]["message"]["content"],
        "First part. Second part."
    );
}

#[test]
fn test_convert_openai_to_anthropic_basic() {
    let openai_request = json!({
        "model": "claude-3-5-sonnet-20241022",
        "messages": [
            {
                "role": "user",
                "content": "Hello!"
            }
        ],
        "max_tokens": 1000,
        "temperature": 0.7
    });

    let result = convert_openai_to_anthropic(&openai_request).unwrap();

    assert_eq!(result["model"], "claude-3-5-sonnet-20241022");
    assert_eq!(result["max_tokens"], 1000);
    assert_eq!(result["temperature"], 0.7);
    assert_eq!(result["messages"][0]["role"], "user");
    assert_eq!(result["messages"][0]["content"], "Hello!");
}

#[test]
fn test_convert_openai_to_anthropic_with_system() {
    let openai_request = json!({
        "model": "claude-3-5-sonnet-20241022",
        "messages": [
            {
                "role": "system",
                "content": "You are a helpful assistant."
            },
            {
                "role": "user",
                "content": "Hello!"
            }
        ],
        "max_tokens": 1000
    });

    let result = convert_openai_to_anthropic(&openai_request).unwrap();

    assert_eq!(result["system"], "You are a helpful assistant.");
    assert_eq!(result["messages"].as_array().unwrap().len(), 1);
    assert_eq!(result["messages"][0]["role"], "user");
}

#[test]
fn test_convert_openai_to_anthropic_usage_mapping() {
    // This test verifies the response conversion maps usage correctly
    let anthropic_response = json!({
        "id": "msg_123",
        "type": "message",
        "role": "assistant",
        "content": [
            {
                "type": "text",
                "text": "Response"
            }
        ],
        "model": "claude-3-5-sonnet-20241022",
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 15,
            "output_tokens": 25
        }
    });

    let result = convert_anthropic_to_openai(&anthropic_response).unwrap();

    assert_eq!(result["usage"]["prompt_tokens"], 15);
    assert_eq!(result["usage"]["completion_tokens"], 25);
    assert_eq!(result["usage"]["total_tokens"], 40);
}

#[test]
fn test_convert_openai_to_anthropic_missing_model() {
    let openai_request = json!({
        "messages": [
            {
                "role": "user",
                "content": "Hello!"
            }
        ]
    });

    let result = convert_openai_to_anthropic(&openai_request);
    assert!(result.is_err());
}

#[test]
fn test_convert_openai_to_anthropic_missing_messages() {
    let openai_request = json!({
        "model": "claude-3-5-sonnet-20241022"
    });

    let result = convert_openai_to_anthropic(&openai_request);
    assert!(result.is_err());
}
