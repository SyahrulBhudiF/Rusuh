use rusuh::providers::zed_response::parse_zed_response;
use serde_json::json;

#[test]
fn test_parse_zed_response_valid() {
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
                    "content": "Hello! How can I help you?"
                },
                "finish_reason": "stop"
            }
        ],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 20,
            "total_tokens": 30
        }
    });

    let result = parse_zed_response(&response);
    assert!(result.is_ok());
    let parsed = result.unwrap();
    assert_eq!(parsed["id"], "chatcmpl-123");
    assert_eq!(
        parsed["choices"][0]["message"]["content"],
        "Hello! How can I help you?"
    );
}

#[test]
fn test_parse_zed_response_missing_id() {
    let response = json!({
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
    assert!(result.is_err());
}

#[test]
fn test_parse_zed_response_missing_choices() {
    let response = json!({
        "id": "chatcmpl-123",
        "object": "chat.completion",
        "created": 1234567890,
        "model": "anthropic/claude-3-5-sonnet-20241022"
    });

    let result = parse_zed_response(&response);
    assert!(result.is_err());
}

#[test]
fn test_parse_zed_response_empty_choices() {
    let response = json!({
        "id": "chatcmpl-123",
        "object": "chat.completion",
        "created": 1234567890,
        "model": "anthropic/claude-3-5-sonnet-20241022",
        "choices": []
    });

    let result = parse_zed_response(&response);
    assert!(result.is_err());
}

#[test]
fn test_parse_zed_response_with_usage() {
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
        ],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 20,
            "total_tokens": 30
        }
    });

    let result = parse_zed_response(&response);
    assert!(result.is_ok());
    let parsed = result.unwrap();
    assert_eq!(parsed["usage"]["prompt_tokens"], 10);
    assert_eq!(parsed["usage"]["completion_tokens"], 20);
    assert_eq!(parsed["usage"]["total_tokens"], 30);
}

#[test]
fn test_parse_zed_response_without_usage() {
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
fn test_parse_zed_response_from_anthropic_event_stream_lines() {
    let response = json!({
        "model": "claude-sonnet-4-5",
        "events": [
            {
                "type": "message_start",
                "message": {
                    "id": "msg_123",
                    "usage": {
                        "input_tokens": 10,
                        "output_tokens": 0
                    }
                }
            },
            {
                "type": "content_block_delta",
                "delta": {
                    "type": "text_delta",
                    "text": "hi"
                }
            },
            {
                "type": "message_stop"
            }
        ]
    });

    let result = parse_zed_response(&response).unwrap();
    assert_eq!(result["id"], "msg_123");
    assert_eq!(result["object"], "chat.completion");
    assert_eq!(result["model"], "claude-sonnet-4-5");
    assert_eq!(result["choices"][0]["message"]["role"], "assistant");
    assert_eq!(result["choices"][0]["message"]["content"], "hi");
    assert_eq!(result["choices"][0]["finish_reason"], "stop");
    assert_eq!(result["usage"]["prompt_tokens"], 10);
}

#[test]
fn test_parse_zed_response_from_raw_jsonlines_body() {
    let response = json!(
        "{\"type\":\"message_start\",\"message\":{\"id\":\"msg_123\",\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n{\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n{\"type\":\"message_stop\"}"
    );

    let error = parse_zed_response(&response).unwrap_err();
    assert!(error
        .to_string()
        .contains("Response missing required field: model"));
}
