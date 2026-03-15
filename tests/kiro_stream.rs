use rusuh::providers::kiro_stream::{EventStreamParser, parse_payload};
use std::io::Cursor;

/// Helper to create a minimal Event Stream message
fn create_event_stream_message(event_type: &str, payload: &[u8]) -> Vec<u8> {
    let mut message = Vec::new();

    // Build headers with :event-type
    let mut headers = Vec::new();
    headers.push(11u8); // name length for ":event-type"
    headers.extend_from_slice(b":event-type");
    headers.push(7u8); // value type: string
    headers.push(0u8); // length high byte
    headers.push(event_type.len() as u8); // length low byte
    headers.extend_from_slice(event_type.as_bytes());

    let headers_length = headers.len() as u32;
    let payload_length = payload.len() as u32;
    let total_length = 12 + headers_length + payload_length + 4; // prelude + headers + payload + crc

    // Write prelude
    message.extend_from_slice(&total_length.to_be_bytes());
    message.extend_from_slice(&headers_length.to_be_bytes());
    message.extend_from_slice(&[0u8; 4]); // prelude_crc (dummy)

    // Write headers
    message.extend_from_slice(&headers);

    // Write payload
    message.extend_from_slice(payload);

    // Write message_crc (dummy)
    message.extend_from_slice(&[0u8; 4]);

    message
}

#[test]
fn test_parse_single_message() {
    let payload = br#"{"content":"Hello, world!","type":"text"}"#;
    let message = create_event_stream_message("assistantResponseEvent", payload);

    let mut parser = EventStreamParser::new(Cursor::new(message));
    let result = parser.read_message().unwrap();

    assert!(result.is_some());
    let msg = result.unwrap();
    assert_eq!(msg.event_type, "assistantResponseEvent");

    let parsed = parse_payload(&msg.payload).unwrap();
    assert_eq!(parsed["content"], "Hello, world!");
    assert_eq!(parsed["type"], "text");
}

#[test]
fn test_parse_multiple_messages() {
    let mut stream = Vec::new();

    // Message 1
    let payload1 = br#"{"content":"First"}"#;
    stream.extend_from_slice(&create_event_stream_message("messageStart", payload1));

    // Message 2
    let payload2 = br#"{"content":"Second"}"#;
    stream.extend_from_slice(&create_event_stream_message("contentBlock", payload2));

    // Message 3
    let payload3 = br#"{"stop_reason":"end_turn"}"#;
    stream.extend_from_slice(&create_event_stream_message("messageStop", payload3));

    let parser = EventStreamParser::new(Cursor::new(stream));
    let messages = parser.parse_all().unwrap();

    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].event_type, "messageStart");
    assert_eq!(messages[1].event_type, "contentBlock");
    assert_eq!(messages[2].event_type, "messageStop");

    let parsed2 = parse_payload(&messages[1].payload).unwrap();
    assert_eq!(parsed2["content"], "Second");
}

#[test]
fn test_empty_payload() {
    let message = create_event_stream_message("ping", &[]);

    let mut parser = EventStreamParser::new(Cursor::new(message));
    let result = parser.read_message().unwrap();

    assert!(result.is_some());
    let msg = result.unwrap();
    assert_eq!(msg.event_type, "ping");
    assert!(msg.payload.is_empty());
}

#[test]
fn test_eof_returns_none() {
    let empty_stream: Vec<u8> = Vec::new();
    let mut parser = EventStreamParser::new(Cursor::new(empty_stream));
    let result = parser.read_message().unwrap();
    assert!(result.is_none());
}

#[test]
fn test_message_too_large() {
    // Create a message with total_length exceeding MAX_EVENT_STREAM_MSG_SIZE
    let mut message = Vec::new();
    let total_length = 11 * 1024 * 1024u32; // 11MB (exceeds 10MB limit)
    message.extend_from_slice(&total_length.to_be_bytes());
    message.extend_from_slice(&[0u8; 8]); // headers_length + prelude_crc

    let mut parser = EventStreamParser::new(Cursor::new(message));
    let result = parser.read_message();

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("message too large"));
}

#[test]
fn test_message_too_small() {
    // Create a message with total_length below minimum
    let mut message = Vec::new();
    let total_length = 10u32; // Less than MIN_EVENT_STREAM_FRAME_SIZE (16)
    message.extend_from_slice(&total_length.to_be_bytes());
    message.extend_from_slice(&[0u8; 8]); // headers_length + prelude_crc

    let mut parser = EventStreamParser::new(Cursor::new(message));
    let result = parser.read_message();

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("invalid message length"));
}

#[test]
fn test_headers_exceed_bounds() {
    // Create a message where headers_length exceeds total_length - 16
    let mut message = Vec::new();
    let total_length = 100u32;
    let headers_length = 90u32; // Exceeds total_length - 16 (84)
    message.extend_from_slice(&total_length.to_be_bytes());
    message.extend_from_slice(&headers_length.to_be_bytes());
    message.extend_from_slice(&[0u8; 4]); // prelude_crc

    let mut parser = EventStreamParser::new(Cursor::new(message));
    let result = parser.read_message();

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("headers length"));
    assert!(err.to_string().contains("exceeds message bounds"));
}

#[test]
fn test_usage_event() {
    let payload = br#"{"inputTokens":100,"outputTokens":50,"totalTokens":150}"#;
    let message = create_event_stream_message("usageEvent", payload);

    let mut parser = EventStreamParser::new(Cursor::new(message));
    let result = parser.read_message().unwrap();

    assert!(result.is_some());
    let msg = result.unwrap();
    assert_eq!(msg.event_type, "usageEvent");

    let parsed = parse_payload(&msg.payload).unwrap();
    assert_eq!(parsed["inputTokens"], 100);
    assert_eq!(parsed["outputTokens"], 50);
    assert_eq!(parsed["totalTokens"], 150);
}

#[test]
fn test_tool_use_event() {
    let payload = br#"{"toolUseId":"tool_123","name":"search","input":{"query":"rust"}}"#;
    let message = create_event_stream_message("toolUseEvent", payload);

    let mut parser = EventStreamParser::new(Cursor::new(message));
    let result = parser.read_message().unwrap();

    assert!(result.is_some());
    let msg = result.unwrap();
    assert_eq!(msg.event_type, "toolUseEvent");

    let parsed = parse_payload(&msg.payload).unwrap();
    assert_eq!(parsed["toolUseId"], "tool_123");
    assert_eq!(parsed["name"], "search");
    assert_eq!(parsed["input"]["query"], "rust");
}
