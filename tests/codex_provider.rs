//! Integration tests for Codex provider registration and model exposure.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use futures::StreamExt;
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::sync::Mutex;

use rusuh::auth::manager::AccountManager;
use rusuh::auth::store::{AuthRecord, AuthStatus};
use rusuh::config::Config;
use rusuh::models::{ChatCompletionRequest, ChatMessage, MessageContent};
use rusuh::providers::Provider;

#[derive(Clone)]
struct MockCodexState {
    non_stream_body: Value,
    stream_chunks: Vec<String>,
    last_request_body: Arc<Mutex<Option<Value>>>,
    non_stream_delay: Option<Duration>,
}

async fn mock_non_stream_handler(
    State(state): State<MockCodexState>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    *state.last_request_body.lock().await = Some(body);
    if let Some(delay) = state.non_stream_delay {
        tokio::time::sleep(delay).await;
    }
    (StatusCode::OK, Json(state.non_stream_body))
}

async fn mock_stream_handler(
    State(state): State<MockCodexState>,
    Json(body): Json<Value>,
) -> (StatusCode, String) {
    *state.last_request_body.lock().await = Some(body);
    (StatusCode::OK, state.stream_chunks.join(""))
}

async fn spawn_codex_mock_server(
    non_stream_body: Value,
    stream_chunks: Vec<String>,
) -> (String, Arc<Mutex<Option<Value>>>) {
    spawn_codex_mock_server_with_delay(non_stream_body, stream_chunks, None).await
}

async fn spawn_codex_mock_server_with_delay(
    non_stream_body: Value,
    stream_chunks: Vec<String>,
    non_stream_delay: Option<Duration>,
) -> (String, Arc<Mutex<Option<Value>>>) {
    let last_request_body = Arc::new(Mutex::new(None));
    let state = MockCodexState {
        non_stream_body,
        stream_chunks,
        last_request_body: last_request_body.clone(),
        non_stream_delay,
    };

    let app = Router::new()
        .route("/chat/completions", post(mock_non_stream_handler))
        .route("/chat/completions/stream", post(mock_stream_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock listener");
    let addr = listener.local_addr().expect("read listener addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    (format!("http://{addr}"), last_request_body)
}

fn codex_auth_json(email: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "codex",
        "provider_key": "codex",
        "access_token": "test-access-token",
        "refresh_token": "test-refresh-token",
        "id_token": "test-id-token",
        "account_id": "acct_test",
        "email": email,
        "expired": "2030-01-01T00:00:00Z",
        "last_refresh": "2026-03-26T00:00:00Z"
    })
}

fn codex_provider_record(base_url: &str) -> AuthRecord {
    let now = chrono::Utc::now();
    let mut metadata = HashMap::new();
    metadata.insert("type".to_string(), json!("codex"));
    metadata.insert("provider_key".to_string(), json!("codex"));
    metadata.insert("access_token".to_string(), json!("test-access-token"));
    metadata.insert("refresh_token".to_string(), json!("test-refresh-token"));
    metadata.insert("id_token".to_string(), json!("test-id-token"));
    metadata.insert("account_id".to_string(), json!("acct_test"));
    metadata.insert("email".to_string(), json!("test@example.com"));
    metadata.insert("expired".to_string(), json!("2030-01-01T00:00:00Z"));
    metadata.insert("last_refresh".to_string(), json!("2026-03-26T00:00:00Z"));
    metadata.insert("base_url".to_string(), json!(base_url));

    AuthRecord {
        id: "codex-test.json".to_string(),
        provider: "codex".to_string(),
        provider_key: "codex".to_string(),
        label: "test@example.com".to_string(),
        disabled: false,
        status: AuthStatus::Active,
        status_message: None,
        last_refreshed_at: Some(now),
        path: PathBuf::from("codex-test.json"),
        metadata,
        updated_at: now,
    }
}

fn codex_chat_request(stream: Option<bool>) -> ChatCompletionRequest {
    let mut extra = HashMap::new();
    extra.insert("previous_response_id".to_string(), json!("resp_123"));
    extra.insert("prompt_cache_retention".to_string(), json!("ephemeral"));
    extra.insert("safety_identifier".to_string(), json!("safe_1"));
    extra.insert("selected_auth_id".to_string(), json!("codex_1"));
    extra.insert("execution_session_id".to_string(), json!("session-123"));

    ChatCompletionRequest {
        model: "gpt-5-codex-thinking".to_string(),
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: MessageContent::Text("hello".to_string()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }],
        stream,
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
async fn codex_auth_file_produces_registered_provider() {
    let dir = TempDir::new().unwrap();

    std::fs::write(
        dir.path().join("codex-one.json"),
        serde_json::to_string_pretty(&codex_auth_json("first@example.com")).unwrap(),
    )
    .unwrap();

    let accounts = AccountManager::with_dir(dir.path());
    accounts.reload().await.unwrap();

    let config = Config::default();
    let registry = std::sync::Arc::new(rusuh::providers::model_registry::ModelRegistry::new());
    let providers = rusuh::providers::registry::build_providers(
        &config,
        &accounts,
        registry,
        rusuh::proxy::KiroRuntimeState::default(),
    )
    .await;

    assert!(
        providers.iter().any(|provider| provider.name() == "codex"),
        "expected at least one codex provider, got: {:?}",
        providers
            .iter()
            .map(|provider| provider.name())
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn codex_provider_lists_openai_catalog_models() {
    let dir = TempDir::new().unwrap();

    std::fs::write(
        dir.path().join("codex-models.json"),
        serde_json::to_string_pretty(&codex_auth_json("models@example.com")).unwrap(),
    )
    .unwrap();

    let accounts = AccountManager::with_dir(dir.path());
    accounts.reload().await.unwrap();

    let config = Config::default();
    let registry = std::sync::Arc::new(rusuh::providers::model_registry::ModelRegistry::new());
    let providers = rusuh::providers::registry::build_providers(
        &config,
        &accounts,
        registry,
        rusuh::proxy::KiroRuntimeState::default(),
    )
    .await;

    let codex = providers
        .iter()
        .find(|provider| provider.name() == "codex")
        .expect("codex provider should be registered");

    let models = codex.list_models().await.unwrap();
    assert!(!models.is_empty(), "codex models must not be empty");
    assert!(
        models.iter().any(|model| model.id == "gpt-5-codex"),
        "expected gpt-5-codex in codex model list"
    );
}

#[tokio::test]
async fn multiple_codex_auth_files_produce_multiple_codex_providers() {
    let dir = TempDir::new().unwrap();

    std::fs::write(
        dir.path().join("codex-a.json"),
        serde_json::to_string_pretty(&codex_auth_json("a@example.com")).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("codex-b.json"),
        serde_json::to_string_pretty(&codex_auth_json("b@example.com")).unwrap(),
    )
    .unwrap();

    let accounts = AccountManager::with_dir(dir.path());
    accounts.reload().await.unwrap();

    let config = Config::default();
    let registry = std::sync::Arc::new(rusuh::providers::model_registry::ModelRegistry::new());
    let providers = rusuh::providers::registry::build_providers(
        &config,
        &accounts,
        registry,
        rusuh::proxy::KiroRuntimeState::default(),
    )
    .await;

    let codex_count = providers
        .iter()
        .filter(|provider| provider.name() == "codex")
        .count();

    assert_eq!(codex_count, 2, "expected one codex provider per auth file");
}

#[test]
fn codex_normalize_model_strips_thinking_suffix() {
    assert_eq!(
        rusuh::providers::codex::normalize_codex_model("gpt-5-codex-thinking"),
        "gpt-5-codex"
    );
    assert_eq!(
        rusuh::providers::codex::normalize_codex_model("gpt-5.2-codex"),
        "gpt-5.2-codex"
    );
}

#[test]
fn codex_prepare_request_removes_unsupported_fields_and_sets_instructions() {
    let mut extra = HashMap::new();
    extra.insert("previous_response_id".to_string(), json!("resp_123"));
    extra.insert("prompt_cache_retention".to_string(), json!("ephemeral"));
    extra.insert("safety_identifier".to_string(), json!("safe_1"));
    extra.insert("selected_auth_id".to_string(), json!("codex_1"));
    extra.insert("execution_session_id".to_string(), json!("session-123"));

    let request = rusuh::models::ChatCompletionRequest {
        model: "gpt-5-codex-thinking".to_string(),
        messages: vec![rusuh::models::ChatMessage {
            role: "user".to_string(),
            content: rusuh::models::MessageContent::Text("Hello".to_string()),
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
    };

    let prepared = rusuh::providers::codex::prepare_codex_request(request);

    assert_eq!(prepared.model, "gpt-5-codex");
    assert!(!prepared.extra.contains_key("previous_response_id"));
    assert!(!prepared.extra.contains_key("prompt_cache_retention"));
    assert!(!prepared.extra.contains_key("safety_identifier"));
    assert!(!prepared.extra.contains_key("selected_auth_id"));
    assert!(!prepared.extra.contains_key("execution_session_id"));
    assert_eq!(
        prepared
            .extra
            .get("instructions")
            .and_then(|value| value.as_str()),
        Some("")
    );
}

#[test]
fn codex_refresh_token_reused_error_is_non_retryable() {
    assert!(rusuh::providers::codex::is_non_retryable_refresh_error(
        "oauth error: refresh_token_reused"
    ));
    assert!(!rusuh::providers::codex::is_non_retryable_refresh_error(
        "timeout while refreshing"
    ));
}

#[test]
fn codex_usage_parsing_supports_codex_and_openai_shapes() {
    let codex_usage = rusuh::providers::codex::parse_usage(json!({
        "input_tokens": 11,
        "output_tokens": 7,
        "total_tokens": 18
    }))
    .expect("codex usage should parse");
    assert_eq!(codex_usage.prompt_tokens, 11);
    assert_eq!(codex_usage.completion_tokens, 7);
    assert_eq!(codex_usage.total_tokens, 18);

    let openai_usage = rusuh::providers::codex::parse_usage(json!({
        "prompt_tokens": 5,
        "completion_tokens": 9,
        "total_tokens": 14
    }))
    .expect("openai usage should parse");
    assert_eq!(openai_usage.prompt_tokens, 5);
    assert_eq!(openai_usage.completion_tokens, 9);
    assert_eq!(openai_usage.total_tokens, 14);
}

#[tokio::test]
async fn codex_chat_completion_executes_against_upstream_and_normalizes_request() {
    let non_stream = json!({
        "id": "resp_1",
        "object": "chat.completion",
        "created": 123,
        "model": "gpt-5-codex",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "hello from codex"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 3, "completion_tokens": 4, "total_tokens": 7}
    });
    let (base_url, seen_body) =
        spawn_codex_mock_server(non_stream, vec!["data: [DONE]\n\n".to_string()]).await;

    let provider =
        match rusuh::providers::codex::CodexProvider::new(codex_provider_record(&base_url)) {
            Ok(provider) => provider,
            Err(error) => panic!("failed to construct codex provider: {error}"),
        };

    let response = provider
        .chat_completion(&codex_chat_request(Some(false)))
        .await
        .expect("chat completion should succeed");

    assert_eq!(response.model, "gpt-5-codex");
    assert_eq!(response.choices.len(), 1);

    let seen = seen_body
        .lock()
        .await
        .clone()
        .expect("request body captured");
    assert_eq!(
        seen.get("model").and_then(Value::as_str),
        Some("gpt-5-codex")
    );
    assert!(seen.get("previous_response_id").is_none());
    assert!(seen.get("prompt_cache_retention").is_none());
    assert!(seen.get("safety_identifier").is_none());
    assert!(seen.get("selected_auth_id").is_none());
    assert!(seen.get("execution_session_id").is_none());
    assert_eq!(seen.get("instructions").and_then(Value::as_str), Some(""));
}

#[tokio::test]
async fn codex_chat_completion_times_out_for_slow_upstream() {
    let non_stream = json!({
        "id": "resp_1",
        "object": "chat.completion",
        "created": 123,
        "model": "gpt-5-codex",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "hello from codex"},
            "finish_reason": "stop"
        }]
    });
    let (base_url, _seen_body) =
        spawn_codex_mock_server_with_delay(non_stream, vec!["data: [DONE]\n\n".to_string()], Some(Duration::from_secs(35))).await;

    let provider =
        match rusuh::providers::codex::CodexProvider::new(codex_provider_record(&base_url)) {
            Ok(provider) => provider,
            Err(error) => panic!("failed to construct codex provider: {error}"),
        };

    let start = Instant::now();
    let error = provider
        .chat_completion(&codex_chat_request(Some(false)))
        .await
        .expect_err("slow upstream should time out");

    assert!(start.elapsed() < Duration::from_secs(20));
    assert!(error.to_string().contains("codex request failed"));
}

#[tokio::test]
async fn codex_chat_completion_stream_executes_against_upstream() {
    let non_stream = json!({
        "id": "resp_1",
        "object": "chat.completion",
        "created": 123,
        "model": "gpt-5-codex",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "unused"},
            "finish_reason": "stop"
        }]
    });
    let (base_url, _seen_body) = spawn_codex_mock_server(
        non_stream,
        vec![
            "data: {\"id\":\"chunk-1\"}\n\n".to_string(),
            "data: [DONE]\n\n".to_string(),
        ],
    )
    .await;

    let provider =
        match rusuh::providers::codex::CodexProvider::new(codex_provider_record(&base_url)) {
            Ok(provider) => provider,
            Err(error) => panic!("failed to construct codex provider: {error}"),
        };

    let mut stream = provider
        .chat_completion_stream(&codex_chat_request(Some(true)))
        .await
        .expect("stream completion should succeed");

    let mut chunks = Vec::new();
    while let Some(item) = stream.next().await {
        let bytes = item.expect("stream item should be ok");
        chunks.push(String::from_utf8_lossy(&bytes).to_string());
    }

    let combined = chunks.join("");
    assert!(combined.contains("chunk-1"));
    assert!(combined.contains("[DONE]"));
}
