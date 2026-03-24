use rusuh::models::{ChatCompletionRequest, ChatMessage, MessageContent};
use rusuh::providers::kiro_translator::{
    build_native_kiro_request, translate_kiro_event_to_openai_sse, HistoryMessage, KiroEventType,
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

    let kiro_req = build_native_kiro_request(&req, None);

    // Verify native Kiro structure
    assert_eq!(kiro_req.conversation_state.current_message.user_input_message.model_id, "claude-3-5-sonnet");
    assert!(kiro_req.conversation_state.current_message.user_input_message.content.contains("You are helpful"));
    assert!(kiro_req.conversation_state.current_message.user_input_message.content.contains("Hello"));
    assert_eq!(kiro_req.conversation_state.agent_task_type, Some("vibe".to_string()));
    assert_eq!(kiro_req.conversation_state.chat_trigger_type, Some("MANUAL".to_string()));
    assert_eq!(kiro_req.conversation_state.current_message.user_input_message.origin, Some("AI_EDITOR".to_string()));
}

#[test]
fn event_type_parsing() {
    assert_eq!(
        KiroEventType::parse("message_start"),
        KiroEventType::MessageStart
    );
    assert_eq!(
        KiroEventType::parse("messageStart"),
        KiroEventType::MessageStart
    );
    assert_eq!(
        KiroEventType::parse("assistantResponseEvent"),
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

#[test]
fn provider_pinned_kiro_model_ids_are_mapped_to_upstream_ids() {
    let req = ChatCompletionRequest {
        model: "kiro-claude-sonnet-4-5".to_string(),
        messages: vec![
            ChatMessage {
                role: "user".to_string(),
                content: MessageContent::Text("Earlier".to_string()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: MessageContent::Text("Reply".to_string()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: MessageContent::Text("Latest".to_string()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
        ],
        temperature: None,
        max_tokens: None,
        top_p: None,
        stop: None,
        stream: Some(true),
        tools: None,
        tool_choice: None,
        extra: std::collections::HashMap::new(),
    };

    let kiro_req = build_native_kiro_request(&req, None);

    assert_eq!(
        kiro_req.conversation_state.current_message.user_input_message.model_id,
        "claude-sonnet-4.5"
    );
    let [HistoryMessage::User(history_user), HistoryMessage::Assistant(_)] =
        kiro_req.conversation_state.history.as_slice()
    else {
        panic!("expected user and assistant history messages");
    };
    assert_eq!(history_user.user_input_message.model_id, "claude-sonnet-4.5");
}

#[test]
fn agentic_kiro_model_ids_use_base_upstream_model_id() {
    let req = ChatCompletionRequest {
        model: "kiro-claude-sonnet-4-5-agentic".to_string(),
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: MessageContent::Text("Hello".to_string()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }],
        temperature: None,
        max_tokens: None,
        top_p: None,
        stop: None,
        stream: Some(true),
        tools: None,
        tool_choice: None,
        extra: std::collections::HashMap::new(),
    };

    let kiro_req = build_native_kiro_request(&req, None);

    assert_eq!(
        kiro_req.conversation_state.current_message.user_input_message.model_id,
        "claude-sonnet-4.5"
    );
}
