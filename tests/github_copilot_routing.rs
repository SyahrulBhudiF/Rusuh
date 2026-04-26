use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use rusuh::auth::store::{AuthRecord, AuthStatus};
use rusuh::models::{
    ChatCompletionRequest, ChatMessage, ContentPart, ImageUrl, MessageContent,
};
use rusuh::providers::Provider;

#[derive(Debug, Clone)]
struct RecordedRequest {
    path: String,
    headers: HashMap<String, String>,
    body: Value,
}

#[derive(Clone)]
struct MockGithubCopilotExecutionState {
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
}

async fn mock_runtime_token_handler() -> (StatusCode, Json<Value>) {
    (
        StatusCode::OK,
        Json(json!({
            "token": "copilot-api-token",
            "expires_at": 4_102_444_800u64
        })),
    )
}

async fn mock_chat_handler(
    State(state): State<MockGithubCopilotExecutionState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    record_request(&state, "/chat/completions", headers, body.clone()).await;
    (
        StatusCode::OK,
        Json(json!({
            "id": "chatcmpl_copilot",
            "object": "chat.completion",
            "created": 0,
            "model": body.get("model").cloned().unwrap_or(json!("unknown")),
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "chat ok"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 1,
                "completion_tokens": 1,
                "total_tokens": 2
            }
        })),
    )
}

async fn mock_responses_handler(
    State(state): State<MockGithubCopilotExecutionState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    record_request(&state, "/responses", headers, body).await;
    (
        StatusCode::OK,
        Json(json!({
            "id": "resp_copilot",
            "object": "response",
            "created_at": 0,
            "model": "gpt-5-codex",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": "responses ok"
                }]
            }],
            "usage": {
                "input_tokens": 1,
                "output_tokens": 1,
                "total_tokens": 2
            }
        })),
    )
}

async fn mock_chat_handler_missing_openai_fields(
    State(state): State<MockGithubCopilotExecutionState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    record_request(&state, "/chat/completions", headers, body.clone()).await;
    (
        StatusCode::OK,
        Json(json!({
            "id": "chatcmpl_copilot",
            "created": 0,
            "model": body.get("model").cloned().unwrap_or(json!("unknown")),
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "chat ok"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 1,
                "completion_tokens": 1,
                "total_tokens": 2
            }
        })),
    )
}

async fn record_request(
    state: &MockGithubCopilotExecutionState,
    path: &str,
    headers: HeaderMap,
    body: Value,
) {
    let headers = headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_ascii_lowercase(), value.to_string()))
        })
        .collect::<HashMap<_, _>>();
    state.requests.lock().await.push(RecordedRequest {
        path: path.to_string(),
        headers,
        body,
    });
}

async fn spawn_execution_server() -> (String, Arc<Mutex<Vec<RecordedRequest>>>) {
    spawn_execution_server_with_chat_handler(post(mock_chat_handler)).await
}

async fn spawn_execution_server_with_chat_handler(
    chat_handler: axum::routing::MethodRouter<MockGithubCopilotExecutionState>,
) -> (String, Arc<Mutex<Vec<RecordedRequest>>>) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let state = MockGithubCopilotExecutionState {
        requests: requests.clone(),
    };

    let app = Router::new()
        .route("/copilot_internal/v2/token", get(mock_runtime_token_handler))
        .route("/chat/completions", chat_handler)
        .route("/responses", post(mock_responses_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind execution server");
    let addr = listener.local_addr().expect("execution addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    (format!("http://{addr}"), requests)
}

fn github_copilot_record(base_url: &str) -> AuthRecord {
    let now = chrono::Utc::now();
    let mut metadata = HashMap::new();
    metadata.insert("type".to_string(), json!("github-copilot"));
    metadata.insert("provider_key".to_string(), json!("github-copilot"));
    metadata.insert("access_token".to_string(), json!("gho_test_token"));
    metadata.insert("token_type".to_string(), json!("bearer"));
    metadata.insert("scope".to_string(), json!("read:user"));
    metadata.insert("user_id".to_string(), json!(42));
    metadata.insert("username".to_string(), json!("octocat"));
    metadata.insert("email".to_string(), json!("octocat@example.com"));
    metadata.insert("copilot_api_url".to_string(), json!(base_url));
    metadata.insert(
        "copilot_token_url".to_string(),
        json!(format!("{}/copilot_internal/v2/token", base_url.trim_end_matches('/'))),
    );

    AuthRecord {
        id: "github-copilot-octocat.json".to_string(),
        provider: "github-copilot".to_string(),
        provider_key: "github-copilot".to_string(),
        label: "octocat@example.com".to_string(),
        disabled: false,
        status: AuthStatus::Active,
        status_message: None,
        last_refreshed_at: Some(now),
        path: PathBuf::from("github-copilot-octocat.json"),
        metadata,
        updated_at: now,
    }
}

fn chat_request_with_assistant_parts_and_tools() -> ChatCompletionRequest {
    ChatCompletionRequest {
        model: "claude-sonnet-4-5-thinking".to_string(),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: MessageContent::Text("be helpful".to_string()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: MessageContent::Parts(vec![
                    ContentPart {
                        part_type: "text".to_string(),
                        text: Some("first".to_string()),
                        image_url: None,
                    },
                    ContentPart {
                        part_type: "text".to_string(),
                        text: Some("second".to_string()),
                        image_url: None,
                    },
                ]),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: MessageContent::Text("hello".to_string()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
        ],
        stream: Some(false),
        max_tokens: None,
        temperature: None,
        top_p: None,
        tools: Some(vec![
            json!({
                "type": "function",
                "function": {
                    "name": "keep_me",
                    "description": "keep me"
                }
            }),
            json!({
                "type": "web_search_preview",
                "name": "drop_me"
            }),
        ]),
        tool_choice: Some(json!("required")),
        stop: None,
        extra: HashMap::new(),
    }
}

fn responses_style_request_with_image() -> ChatCompletionRequest {
    let mut extra = HashMap::new();
    extra.insert("input".to_string(), json!([
        {
            "role": "user",
            "content": [
                {"type": "input_text", "text": "what is in this image?"},
                {"type": "input_image", "image_url": "https://example.com/cat.png"}
            ]
        }
    ]));

    ChatCompletionRequest {
        model: "gpt-5-codex".to_string(),
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: MessageContent::Parts(vec![
                ContentPart {
                    part_type: "text".to_string(),
                    text: Some("what is in this image?".to_string()),
                    image_url: None,
                },
                ContentPart {
                    part_type: "image_url".to_string(),
                    text: None,
                    image_url: Some(ImageUrl {
                        url: "https://example.com/cat.png".to_string(),
                        detail: None,
                    }),
                },
            ]),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }],
        stream: Some(false),
        max_tokens: None,
        temperature: None,
        top_p: None,
        tools: None,
        tool_choice: None,
        stop: None,
        extra,
    }
}

#[tokio::test]
async fn github_copilot_chat_requests_use_chat_completions_and_normalize_payload() {
    let (base_url, requests) = spawn_execution_server().await;
    let provider = rusuh::providers::github_copilot::GithubCopilotProvider::new(
        github_copilot_record(&base_url),
    )
    .expect("provider should construct");

    let response = provider
        .chat_completion(&chat_request_with_assistant_parts_and_tools())
        .await
        .expect("chat request should succeed");

    assert_eq!(response.choices.len(), 1);
    let captured = requests.lock().await;
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].path, "/chat/completions");
    assert_eq!(captured[0].body["model"], "claude-sonnet-4-5");
    assert_eq!(
        captured[0].body["messages"][1]["content"],
        json!("first\nsecond")
    );
    assert_eq!(captured[0].body["tools"].as_array().unwrap().len(), 1);
    assert_eq!(captured[0].body["tools"][0]["type"], "function");
    assert_eq!(captured[0].body["tool_choice"], json!("auto"));
    assert_eq!(
        captured[0].headers.get("authorization").map(String::as_str),
        Some("Bearer copilot-api-token")
    );
    assert!(captured[0].headers.contains_key("user-agent"));
    assert!(captured[0].headers.contains_key("editor-version"));
    assert!(captured[0].headers.contains_key("editor-plugin-version"));
    assert!(captured[0].headers.contains_key("openai-intent"));
    assert!(captured[0].headers.contains_key("copilot-integration-id"));
    assert_eq!(
        captured[0]
            .headers
            .get("x-github-api-version")
            .map(String::as_str),
        Some("2025-04-01")
    );
    assert!(captured[0].headers.contains_key("x-request-id"));
    assert!(captured[0].headers.contains_key("x-initiator"));
}

#[tokio::test]
async fn github_copilot_responses_only_models_use_responses_endpoint_and_vision_header() {
    let (base_url, requests) = spawn_execution_server().await;
    let provider = rusuh::providers::github_copilot::GithubCopilotProvider::new(
        github_copilot_record(&base_url),
    )
    .expect("provider should construct");

    let response = provider
        .chat_completion(&responses_style_request_with_image())
        .await
        .expect("responses request should succeed");

    let first_choice = response.choices.first().expect("assistant choice");
    let message = first_choice.message.as_ref().expect("assistant message");
    match &message.content {
        rusuh::models::MessageContent::Text(text) => assert_eq!(text, "responses ok"),
        other => panic!("expected text content, got {other:?}"),
    }

    let captured = requests.lock().await;
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].path, "/responses");
    assert_eq!(captured[0].body["model"], "gpt-5-codex");
    assert_eq!(
        captured[0]
            .headers
            .get("copilot-vision-request")
            .map(String::as_str),
        Some("true")
    );
}

#[tokio::test]
async fn github_copilot_chat_requests_accept_live_payload_without_object_or_choice_index() {
    let (base_url, _requests) =
        spawn_execution_server_with_chat_handler(post(mock_chat_handler_missing_openai_fields)).await;
    let provider = rusuh::providers::github_copilot::GithubCopilotProvider::new(
        github_copilot_record(&base_url),
    )
    .expect("provider should construct");

    let response = provider
        .chat_completion(&chat_request_with_assistant_parts_and_tools())
        .await
        .expect("chat request should succeed");

    assert_eq!(response.object, "chat.completion");
    assert_eq!(response.choices.len(), 1);
    assert_eq!(response.choices[0].index, 0);
    let message = response.choices[0].message.as_ref().expect("assistant message");
    match &message.content {
        rusuh::models::MessageContent::Text(text) => assert_eq!(text, "chat ok"),
        other => panic!("expected text content, got {other:?}"),
    }
}
