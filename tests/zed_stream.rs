use rusuh::providers::zed_response::{format_sse_event, parse_jsonlines_chunk};

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

    // Should skip invalid line and parse valid one
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
