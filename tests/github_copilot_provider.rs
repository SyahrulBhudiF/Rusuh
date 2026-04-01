use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::sync::Mutex;

use rusuh::auth::manager::AccountManager;
use rusuh::auth::store::{AuthRecord, AuthStatus};
use rusuh::config::Config;
use rusuh::providers::Provider;

#[derive(Clone)]
struct MockGithubCopilotModelsState {
    status: StatusCode,
    body: Value,
    last_authorization: Arc<Mutex<Option<String>>>,
}

async fn mock_models_handler(
    State(state): State<MockGithubCopilotModelsState>,
    request: axum::extract::Request,
) -> (StatusCode, Json<Value>) {
    let authorization = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    *state.last_authorization.lock().await = authorization;
    (state.status, Json(state.body))
}

async fn spawn_models_server(
    status: StatusCode,
    body: Value,
) -> (String, Arc<Mutex<Option<String>>>) {
    let last_authorization = Arc::new(Mutex::new(None));
    let state = MockGithubCopilotModelsState {
        status,
        body,
        last_authorization: last_authorization.clone(),
    };

    let app = Router::new()
        .route("/models", get(mock_models_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind models server");
    let addr = listener.local_addr().expect("models addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    (format!("http://{addr}"), last_authorization)
}

#[tokio::test]
async fn github_copilot_provider_models_live_fetch_success_path() {
    let (base_url, last_authorization) = spawn_models_server(
        StatusCode::OK,
        json!({
            "data": [
                {
                    "id": "claude-3.7-sonnet",
                    "object": "model",
                    "owned_by": "github-copilot"
                }
            ]
        }),
    )
    .await;

    let models = rusuh::auth::github_copilot_runtime::list_models(
        &reqwest::Client::new(),
        &base_url,
        "copilot-api-token",
    )
    .await
    .expect("live models should parse");

    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "claude-3.7-sonnet");
    assert_eq!(
        last_authorization.lock().await.as_deref(),
        Some("Bearer copilot-api-token")
    );
}

#[tokio::test]
async fn github_copilot_provider_models_live_fetch_failure_path() {
    let (base_url, _last_authorization) = spawn_models_server(
        StatusCode::BAD_GATEWAY,
        json!({"error": "upstream failed"}),
    )
    .await;

    let error = rusuh::auth::github_copilot_runtime::list_models(
        &reqwest::Client::new(),
        &base_url,
        "copilot-api-token",
    )
    .await
    .expect_err("non-success status should fail");

    assert!(error.to_string().contains("live model request failed"));
}

#[tokio::test]
async fn github_copilot_provider_models_live_fetch_empty_response_path() {
    let (base_url, _last_authorization) = spawn_models_server(
        StatusCode::OK,
        json!({"data": []}),
    )
    .await;

    let error = rusuh::auth::github_copilot_runtime::list_models(
        &reqwest::Client::new(),
        &base_url,
        "copilot-api-token",
    )
    .await
    .expect_err("empty live model list should fail");

    assert!(error.to_string().contains("returned no usable models"));
}

#[derive(Clone)]
struct MockGithubCopilotRuntimeState {
    body: Value,
}

async fn mock_runtime_token_handler(
    State(state): State<MockGithubCopilotRuntimeState>,
) -> (StatusCode, Json<Value>) {
    (StatusCode::OK, Json(state.body))
}

async fn spawn_runtime_token_server(body: Value) -> String {
    let state = MockGithubCopilotRuntimeState { body };
    let app = Router::new()
        .route("/copilot_internal/v2/token", get(mock_runtime_token_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind runtime server");
    let addr = listener.local_addr().expect("runtime addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    format!("http://{addr}")
}

fn github_copilot_auth_json(username: &str) -> Value {
    json!({
        "type": "github-copilot",
        "provider_key": "github-copilot",
        "access_token": "gho_test_token",
        "token_type": "bearer",
        "scope": "read:user",
        "user_id": 42,
        "username": username,
        "email": format!("{username}@example.com"),
        "status": "active",
        "disabled": false
    })
}

fn github_copilot_record(api_base_url: &str, token_base_url: &str) -> AuthRecord {
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
    metadata.insert("copilot_api_url".to_string(), json!(api_base_url));
    metadata.insert(
        "copilot_token_url".to_string(),
        json!(format!("{}/copilot_internal/v2/token", token_base_url.trim_end_matches('/'))),
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

#[tokio::test]
async fn github_copilot_auth_file_produces_registered_provider() {
    let dir = TempDir::new().unwrap();

    std::fs::write(
        dir.path().join("github-copilot-octocat.json"),
        serde_json::to_string_pretty(&github_copilot_auth_json("octocat")).unwrap(),
    )
    .unwrap();

    let accounts = AccountManager::with_dir(dir.path());
    accounts.reload().await.unwrap();

    let config = Config::default();
    let registry = Arc::new(rusuh::providers::model_registry::ModelRegistry::new());
    let providers = rusuh::providers::registry::build_providers(
        &config,
        &accounts,
        registry,
        rusuh::proxy::KiroRuntimeState::default(),
    )
    .await;

    assert!(
        providers.iter().any(|provider| provider.name() == "github-copilot"),
        "expected at least one github-copilot provider, got: {:?}",
        providers
            .iter()
            .map(|provider| provider.name())
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn multiple_github_copilot_auth_files_produce_multiple_providers() {
    let dir = TempDir::new().unwrap();

    std::fs::write(
        dir.path().join("github-copilot-octocat.json"),
        serde_json::to_string_pretty(&github_copilot_auth_json("octocat")).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("github-copilot-hubot.json"),
        serde_json::to_string_pretty(&github_copilot_auth_json("hubot")).unwrap(),
    )
    .unwrap();

    let accounts = AccountManager::with_dir(dir.path());
    accounts.reload().await.unwrap();

    let config = Config::default();
    let registry = Arc::new(rusuh::providers::model_registry::ModelRegistry::new());
    let providers = rusuh::providers::registry::build_providers(
        &config,
        &accounts,
        registry,
        rusuh::proxy::KiroRuntimeState::default(),
    )
    .await;

    let github_copilot_count = providers
        .iter()
        .filter(|provider| provider.name() == "github-copilot")
        .count();

    assert_eq!(
        github_copilot_count, 2,
        "expected one github-copilot provider per auth file"
    );
}

#[tokio::test]
async fn github_copilot_provider_list_models_prefers_live_visibility_ids() {
    let (api_base_url, last_authorization) = spawn_models_server(
        StatusCode::OK,
        json!({
            "data": [
                {
                    "id": "claude-sonnet-4-5",
                    "object": "model",
                    "owned_by": "github-copilot"
                }
            ]
        }),
    )
    .await;
    let token_base_url = spawn_runtime_token_server(json!({
        "token": "copilot-api-token",
        "expires_at": 4_102_444_800u64
    }))
    .await;

    let provider = rusuh::providers::github_copilot::GithubCopilotProvider::new(
        github_copilot_record(&api_base_url, &token_base_url),
    )
    .expect("provider should construct");

    let models = provider.list_models().await.expect("live fetch should succeed");

    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "claude-sonnet-4-5");
    assert_eq!(
        last_authorization.lock().await.as_deref(),
        Some("Bearer copilot-api-token")
    );
}

#[tokio::test]
async fn github_copilot_provider_list_models_falls_back_to_static_catalog_when_live_fetch_fails() {
    let (api_base_url, _last_authorization) = spawn_models_server(
        StatusCode::BAD_GATEWAY,
        json!({"error": "upstream failed"}),
    )
    .await;
    let token_base_url = spawn_runtime_token_server(json!({
        "token": "copilot-api-token",
        "expires_at": 4_102_444_800u64
    }))
    .await;

    let provider = rusuh::providers::github_copilot::GithubCopilotProvider::new(
        github_copilot_record(&api_base_url, &token_base_url),
    )
    .expect("provider should construct");

    let models = provider
        .list_models()
        .await
        .expect("static fallback should keep provider visible");

    assert!(
        models.iter().any(|model| model.id == "claude-sonnet-4-5"),
        "expected static Claude model fallback"
    );
    assert!(
        models.iter().any(|model| model.id == "gpt-5-codex"),
        "expected static Codex model fallback"
    );
}
