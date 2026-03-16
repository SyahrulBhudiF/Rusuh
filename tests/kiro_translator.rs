use rusuh::models::{ChatCompletionRequest, ChatMessage, MessageContent};
use rusuh::providers::kiro_translator::{
    translate_kiro_event_to_openai_sse, translate_request_to_kiro, KiroEventType,
};

#[test]
fn translate_simple_request() {
    let req = ChatCompletionRequest {
        model: "claude-3-5-sonnet".to_string(),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: MessageContent::Text("You are helpful".to_string()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: MessageContent::Text("Hello".to_string()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
        ],
        temperature: Some(0.7),
        max_tokens: Some(1024),
        top_p: None,
        stop: None,
        stream: None,
        tools: None,
        tool_choice: None,
        extra: std::collections::HashMap::new(),
    };

    let kiro_req = translate_request_to_kiro(&req);

    assert_eq!(kiro_req["system"], "You are helpful");
    assert_eq!(kiro_req["max_tokens"], 1024);
    assert_eq!(kiro_req["messages"].as_array().map(Vec::len), Some(1));
    assert_eq!(kiro_req["messages"][0]["role"], "user");
    assert_eq!(kiro_req["messages"][0]["content"], "Hello");
    let temp = kiro_req["temperature"]
        .as_f64()
        .expect("temperature should be f64");
    assert!((temp - 0.7).abs() < 0.01);
}

#[test]
fn event_type_parsing() {
    assert_eq!(
        KiroEventType::from_str("message_start"),
        KiroEventType::MessageStart
    );
    assert_eq!(
        KiroEventType::from_str("messageStart"),
        KiroEventType::MessageStart
    );
    assert_eq!(
        KiroEventType::from_str("assistantResponseEvent"),
        KiroEventType::AssistantResponseEvent
    );
}

#[test]
fn event_filtering() {
    assert!(KiroEventType::FollowupPromptEvent.should_filter());
    assert!(KiroEventType::MeteringEvent.should_filter());
    assert!(!KiroEventType::MessageStart.should_filter());
    assert!(!KiroEventType::ContentBlockDelta.should_filter());
}

#[test]
fn assistant_response_event_streams_text() {
    let sse = translate_kiro_event_to_openai_sse(
        "assistantResponseEvent",
        br#"{"content":"Hello"}"#,
        "chat_1",
        "kiro-model",
        123,
    )
    .expect("sse output");

    let text = String::from_utf8(sse.to_vec()).expect("utf8 sse");
    assert!(text.contains("event: message"));
    assert!(text.contains("\"content\":\"Hello\""));
}

#[test]
fn usage_event_streams_usage_payload() {
    let sse = translate_kiro_event_to_openai_sse(
        "usageEvent",
        br#"{"inputTokens":100,"outputTokens":50}"#,
        "chat_1",
        "kiro-model",
        123,
    )
    .expect("usage sse output");

    let text = String::from_utf8(sse.to_vec()).expect("utf8 sse");
    assert!(text.contains("\"usage\""));
    assert!(text.contains("\"prompt_tokens\":100"));
    assert!(text.contains("\"completion_tokens\":50"));
    assert!(text.contains("\"total_tokens\":150"));
}

#[test]
fn filtered_event_returns_none() {
    let result = translate_kiro_event_to_openai_sse(
        "followupPromptEvent",
        br#"{"content":"ignore"}"#,
        "chat_1",
        "kiro-model",
        123,
    );
    assert!(result.is_none());
}
