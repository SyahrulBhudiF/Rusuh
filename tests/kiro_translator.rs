use bytes::Bytes;
use rusuh::models::{ChatCompletionRequest, ChatMessage, MessageContent};
use rusuh::providers::kiro_stream::EventStreamMessage;
use rusuh::providers::kiro_translator::{
    aggregate_kiro_messages, build_native_kiro_request, build_openai_chat_completion_response,
    translate_kiro_event_to_openai_sse, HistoryMessage, KiroAggregatedResponse, KiroEventType,
};
use serde_json::json;

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
    assert_eq!(
        kiro_req
            .conversation_state
            .current_message
            .user_input_message
            .model_id,
        "claude-3-5-sonnet"
    );
    assert!(kiro_req
        .conversation_state
        .current_message
        .user_input_message
        .content
        .contains("You are helpful"));
    assert!(kiro_req
        .conversation_state
        .current_message
        .user_input_message
        .content
        .contains("Hello"));
    assert_eq!(
        kiro_req.conversation_state.agent_task_type,
        Some("vibe".to_string())
    );
    assert_eq!(
        kiro_req.conversation_state.chat_trigger_type,
        Some("MANUAL".to_string())
    );
    assert_eq!(
        kiro_req
            .conversation_state
            .current_message
            .user_input_message
            .origin,
        Some("AI_EDITOR".to_string())
    );
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
    assert!(text.contains("event: message\ndata: {"));
}

#[test]
fn assistant_response_event_emits_parseable_json_data_line() {
    let sse = translate_kiro_event_to_openai_sse(
        "assistantResponseEvent",
        br#"{"content":"Hello"}"#,
        "chat_1",
        "kiro-model",
        123,
    )
    .expect("sse output");

    let text = String::from_utf8(sse.to_vec()).expect("utf8 sse");
    let json_line = text
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .expect("SSE data line");
    let payload: serde_json::Value = serde_json::from_str(json_line).expect("valid json data line");

    assert_eq!(payload["choices"][0]["delta"]["content"], "Hello");
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
        kiro_req
            .conversation_state
            .current_message
            .user_input_message
            .model_id,
        "claude-sonnet-4.5"
    );
    let [HistoryMessage::User(history_user), HistoryMessage::Assistant(_)] =
        kiro_req.conversation_state.history.as_slice()
    else {
        panic!("expected user and assistant history messages");
    };
    assert_eq!(
        history_user.user_input_message.model_id,
        "claude-sonnet-4.5"
    );
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
        kiro_req
            .conversation_state
            .current_message
            .user_input_message
            .model_id,
        "claude-sonnet-4.5"
    );
}

// ── Aggregation Tests ───────────────────────────────────────────────────────

#[test]
fn aggregate_simple_text_only_messages() {
    let messages = vec![
        EventStreamMessage {
            event_type: "assistantResponseEvent".to_string(),
            payload: Bytes::from(r#"{"content":"Hello"}"#),
        },
        EventStreamMessage {
            event_type: "contentBlockDelta".to_string(),
            payload: Bytes::from(r#"{"delta":{"text":" world"}}"#),
        },
        EventStreamMessage {
            event_type: "assistantResponseEvent".to_string(),
            payload: Bytes::from(r#"{"content":"!"}"#),
        },
    ];

    let result = aggregate_kiro_messages(&messages);

    assert_eq!(result.content, "Hello world!");
    assert!(result.tool_calls.is_empty());
    assert!(result.usage.is_none());
    assert!(result.stop_reason.is_none());
}

#[test]
fn aggregate_usage_data() {
    let messages = vec![
        EventStreamMessage {
            event_type: "assistantResponseEvent".to_string(),
            payload: Bytes::from(r#"{"content":"Response"}"#),
        },
        EventStreamMessage {
            event_type: "usageEvent".to_string(),
            payload: Bytes::from(r#"{"inputTokens":100,"outputTokens":50}"#),
        },
    ];

    let result = aggregate_kiro_messages(&messages);

    assert_eq!(result.content, "Response");
    assert!(result.usage.is_some());
    let usage = result.usage.unwrap();
    assert_eq!(usage["prompt_tokens"], 100);
    assert_eq!(usage["completion_tokens"], 50);
    assert_eq!(usage["total_tokens"], 150);
}

#[test]
fn aggregate_stop_reason_end_turn() {
    let messages = vec![
        EventStreamMessage {
            event_type: "assistantResponseEvent".to_string(),
            payload: Bytes::from(r#"{"content":"Done"}"#),
        },
        EventStreamMessage {
            event_type: "messageStop".to_string(),
            payload: Bytes::from(r#"{"stopReason":"end_turn"}"#),
        },
    ];

    let result = aggregate_kiro_messages(&messages);

    assert_eq!(result.content, "Done");
    assert_eq!(result.stop_reason, Some("end_turn".to_string()));
}

#[test]
fn aggregate_tool_use_event_into_openai_tool_call() {
    let messages = vec![
        EventStreamMessage {
            event_type: "assistantResponseEvent".to_string(),
            payload: Bytes::from(r#"{"content":"Let me help"}"#),
        },
        EventStreamMessage {
            event_type: "toolUseEvent".to_string(),
            payload: Bytes::from(
                r#"{"tool":{"id":"call_123","name":"get_weather","input":{"location":"NYC"}}}"#,
            ),
        },
    ];

    let result = aggregate_kiro_messages(&messages);

    assert_eq!(result.content, "Let me help");
    assert_eq!(result.tool_calls.len(), 1);

    let tool_call = &result.tool_calls[0];
    assert_eq!(tool_call["id"], "call_123");
    assert_eq!(tool_call["type"], "function");
    assert_eq!(tool_call["function"]["name"], "get_weather");
    assert_eq!(tool_call["function"]["arguments"], r#"{"location":"NYC"}"#);
}

#[test]
fn aggregate_multiple_tool_calls() {
    let messages = vec![
        EventStreamMessage {
            event_type: "toolUseEvent".to_string(),
            payload: Bytes::from(r#"{"tool":{"id":"call_1","name":"tool_a","input":{"x":1}}}"#),
        },
        EventStreamMessage {
            event_type: "toolUseEvent".to_string(),
            payload: Bytes::from(r#"{"tool":{"id":"call_2","name":"tool_b","input":{"y":2}}}"#),
        },
    ];

    let result = aggregate_kiro_messages(&messages);

    assert_eq!(result.tool_calls.len(), 2);
    assert_eq!(result.tool_calls[0]["id"], "call_1");
    assert_eq!(result.tool_calls[1]["id"], "call_2");
}

#[test]
fn aggregate_tool_uses_array_in_assistant_response() {
    let messages = vec![EventStreamMessage {
        event_type: "assistantResponseEvent".to_string(),
        payload: Bytes::from(
            r#"{"content":"Using tools","toolUses":[{"id":"call_x","name":"search","input":{"q":"rust"}}]}"#,
        ),
    }];

    let result = aggregate_kiro_messages(&messages);

    assert_eq!(result.content, "Using tools");
    assert_eq!(result.tool_calls.len(), 1);
    assert_eq!(result.tool_calls[0]["id"], "call_x");
    assert_eq!(result.tool_calls[0]["function"]["name"], "search");
}

#[test]
fn aggregate_empty_messages_returns_default() {
    let messages: Vec<EventStreamMessage> = vec![];
    let result = aggregate_kiro_messages(&messages);

    assert_eq!(result, KiroAggregatedResponse::default());
}

#[test]
fn aggregate_flat_tool_use_event_payload() {
    let messages = vec![
        EventStreamMessage {
            event_type: "assistantResponseEvent".to_string(),
            payload: Bytes::from(r#"{"content":"Checking weather"}"#),
        },
        EventStreamMessage {
            event_type: "toolUseEvent".to_string(),
            payload: Bytes::from(
                r#"{"toolUseId":"tool_123","name":"get_weather","input":{"location":"NYC"}}"#,
            ),
        },
    ];

    let result = aggregate_kiro_messages(&messages);

    assert_eq!(result.content, "Checking weather");
    assert_eq!(result.tool_calls.len(), 1);

    let tool_call = &result.tool_calls[0];
    assert_eq!(tool_call["id"], "tool_123");
    assert_eq!(tool_call["type"], "function");
    assert_eq!(tool_call["function"]["name"], "get_weather");
    assert_eq!(tool_call["function"]["arguments"], r#"{"location":"NYC"}"#);
}

// ── Non-Stream Response Builder Tests ──────────────────────────────────────

#[test]
fn build_text_only_response_with_stop_finish_reason() {
    let aggregate = KiroAggregatedResponse {
        content: "Hello, how can I help you?".to_string(),
        tool_calls: vec![],
        usage: None,
        stop_reason: Some("end_turn".to_string()),
    };

    let response = build_openai_chat_completion_response(
        aggregate,
        "chatcmpl-123",
        "kiro-claude-sonnet-4-5",
        1234567890,
    );

    assert_eq!(response.id, "chatcmpl-123");
    assert_eq!(response.object, "chat.completion");
    assert_eq!(response.created, 1234567890);
    assert_eq!(response.model, "kiro-claude-sonnet-4-5");
    assert_eq!(response.choices.len(), 1);

    let choice = &response.choices[0];
    assert_eq!(choice.index, 0);
    assert_eq!(choice.finish_reason, Some("stop".to_string()));

    let message = choice.message.as_ref().expect("message should exist");
    assert_eq!(message.role, "assistant");
    match &message.content {
        MessageContent::Text(text) => assert_eq!(text, "Hello, how can I help you?"),
        _ => panic!("expected Text content"),
    }
    assert!(message.tool_calls.is_none());
    assert!(response.usage.is_none());
}

#[test]
fn build_response_with_max_tokens_maps_to_length() {
    let aggregate = KiroAggregatedResponse {
        content: "This response was cut off due to".to_string(),
        tool_calls: vec![],
        usage: None,
        stop_reason: Some("max_tokens".to_string()),
    };

    let response = build_openai_chat_completion_response(
        aggregate,
        "chatcmpl-456",
        "kiro-claude-sonnet-4-5",
        1234567890,
    );

    let choice = &response.choices[0];
    assert_eq!(choice.finish_reason, Some("length".to_string()));
}

#[test]
fn build_response_with_tool_use_maps_to_tool_calls_finish_reason() {
    let aggregate = KiroAggregatedResponse {
        content: "Let me check that for you.".to_string(),
        tool_calls: vec![json!({
            "id": "call_abc",
            "type": "function",
            "function": {
                "name": "get_weather",
                "arguments": r#"{"location":"San Francisco"}"#
            }
        })],
        usage: None,
        stop_reason: Some("tool_use".to_string()),
    };

    let response = build_openai_chat_completion_response(
        aggregate,
        "chatcmpl-789",
        "kiro-claude-sonnet-4-5",
        1234567890,
    );

    let choice = &response.choices[0];
    assert_eq!(choice.finish_reason, Some("tool_calls".to_string()));

    let message = choice.message.as_ref().expect("message should exist");
    match &message.content {
        MessageContent::Text(text) => assert_eq!(text, "Let me check that for you."),
        _ => panic!("expected Text content"),
    }

    let tool_calls = message
        .tool_calls
        .as_ref()
        .expect("tool_calls should exist");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0]["id"], "call_abc");
    assert_eq!(tool_calls[0]["function"]["name"], "get_weather");
}

#[test]
fn build_response_with_usage_populates_usage_field() {
    let aggregate = KiroAggregatedResponse {
        content: "Response with usage".to_string(),
        tool_calls: vec![],
        usage: Some(json!({
            "prompt_tokens": 150,
            "completion_tokens": 75,
            "total_tokens": 225
        })),
        stop_reason: Some("end_turn".to_string()),
    };

    let response = build_openai_chat_completion_response(
        aggregate,
        "chatcmpl-usage",
        "kiro-claude-sonnet-4-5",
        1234567890,
    );

    let usage = response.usage.expect("usage should exist");
    assert_eq!(usage.prompt_tokens, 150);
    assert_eq!(usage.completion_tokens, 75);
    assert_eq!(usage.total_tokens, 225);
}

#[test]
fn build_response_with_only_tool_calls_uses_empty_text_content() {
    let aggregate = KiroAggregatedResponse {
        content: String::new(),
        tool_calls: vec![json!({
            "id": "call_xyz",
            "type": "function",
            "function": {
                "name": "search",
                "arguments": r#"{"query":"rust"}"#
            }
        })],
        usage: None,
        stop_reason: Some("tool_use".to_string()),
    };

    let response = build_openai_chat_completion_response(
        aggregate,
        "chatcmpl-tool-only",
        "kiro-claude-sonnet-4-5",
        1234567890,
    );

    let message = response.choices[0]
        .message
        .as_ref()
        .expect("message should exist");
    match &message.content {
        MessageContent::Text(text) => assert_eq!(text, ""),
        _ => panic!("expected Text content"),
    }

    let tool_calls = message
        .tool_calls
        .as_ref()
        .expect("tool_calls should exist");
    assert_eq!(tool_calls.len(), 1);
}

#[test]
fn build_response_with_no_stop_reason_defaults_to_stop() {
    let aggregate = KiroAggregatedResponse {
        content: "Response without explicit stop reason".to_string(),
        tool_calls: vec![],
        usage: None,
        stop_reason: None,
    };

    let response = build_openai_chat_completion_response(
        aggregate,
        "chatcmpl-default",
        "kiro-claude-sonnet-4-5",
        1234567890,
    );

    let choice = &response.choices[0];
    assert_eq!(choice.finish_reason, Some("stop".to_string()));
}
