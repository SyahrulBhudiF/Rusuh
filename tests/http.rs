use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use bytes::Bytes;
use tokio::sync::{Mutex, Notify};
use tower::ServiceExt;

use rusuh::auth::manager::AccountManager;
use rusuh::config::Config;
use rusuh::error::AppError;
use rusuh::models::{
    ChatCompletionRequest, ChatCompletionResponse, ChatMessage, Choice, MessageContent, ModelInfo,
};
use rusuh::providers::model_info::ExtModelInfo;
use rusuh::providers::model_registry::ModelRegistry;
use rusuh::providers::{BoxStream, Provider};
use rusuh::proxy::ProxyState;
use rusuh::router::build_router;

/// Build a test app with the given config.
fn test_app(cfg: Config) -> axum::Router {
    let accounts = Arc::new(AccountManager::with_dir("/tmp/rusuh_test_nonexistent"));
    let registry = Arc::new(ModelRegistry::new());
    let state = Arc::new(ProxyState::new(cfg, accounts, registry, 0));

    build_router(state.clone()).layer(axum::middleware::from_fn_with_state(
        state,
        rusuh::middleware::auth::api_key_auth,
    ))
}

#[derive(Debug)]
struct StubProvider {
    name: &'static str,
    provider_type: &'static str,
    client_id: String,
    models: Vec<ModelInfo>,
    observed_models: Arc<Mutex<Vec<String>>>,
    result: StubCompletionResult,
}

#[derive(Debug, Clone)]
enum StubCompletionResult {
    Success,
    SuccessWithToolCalls(Vec<serde_json::Value>),
    QuotaExceeded(String),
}

impl StubProvider {
    fn success(
        name: &'static str,
        client_id: &str,
        model_ids: &[&str],
        observed_models: Arc<Mutex<Vec<String>>>,
    ) -> Self {
        Self::success_with_type(name, name, client_id, model_ids, observed_models)
    }

    fn success_with_type(
        name: &'static str,
        provider_type: &'static str,
        client_id: &str,
        model_ids: &[&str],
        observed_models: Arc<Mutex<Vec<String>>>,
    ) -> Self {
        Self {
            name,
            provider_type,
            client_id: client_id.to_string(),
            models: model_ids
                .iter()
                .map(|id| ModelInfo {
                    id: (*id).to_string(),
                    object: "model".to_string(),
                    created: 0,
                    owned_by: name.to_string(),
                })
                .collect(),
            observed_models,
            result: StubCompletionResult::Success,
        }
    }

    fn success_with_tool_calls(
        name: &'static str,
        client_id: &str,
        model_ids: &[&str],
        observed_models: Arc<Mutex<Vec<String>>>,
        tool_calls: Vec<serde_json::Value>,
    ) -> Self {
        Self {
            name,
            provider_type: name,
            client_id: client_id.to_string(),
            models: model_ids
                .iter()
                .map(|id| ModelInfo {
                    id: (*id).to_string(),
                    object: "model".to_string(),
                    created: 0,
                    owned_by: name.to_string(),
                })
                .collect(),
            observed_models,
            result: StubCompletionResult::SuccessWithToolCalls(tool_calls),
        }
    }

    fn quota_exceeded(
        name: &'static str,
        client_id: &str,
        model_ids: &[&str],
        observed_models: Arc<Mutex<Vec<String>>>,
        message: &str,
    ) -> Self {
        Self {
            name,
            provider_type: name,
            client_id: client_id.to_string(),
            models: model_ids
                .iter()
                .map(|id| ModelInfo {
                    id: (*id).to_string(),
                    object: "model".to_string(),
                    created: 0,
                    owned_by: name.to_string(),
                })
                .collect(),
            observed_models,
            result: StubCompletionResult::QuotaExceeded(message.to_string()),
        }
    }
}

#[async_trait]
impl Provider for StubProvider {
    fn name(&self) -> &str {
        self.name
    }

    fn provider_type(&self) -> &str {
        self.provider_type
    }

    fn client_id(&self) -> &str {
        &self.client_id
    }

    async fn list_models(&self) -> rusuh::error::AppResult<Vec<ModelInfo>> {
        Ok(self.models.clone())
    }

    async fn chat_completion(
        &self,
        req: &ChatCompletionRequest,
    ) -> rusuh::error::AppResult<ChatCompletionResponse> {
        self.observed_models.lock().await.push(req.model.clone());

        match &self.result {
            StubCompletionResult::Success => Ok(ChatCompletionResponse {
                id: format!("{}-ok", self.name),
                object: "chat.completion".to_string(),
                created: 0,
                model: req.model.clone(),
                choices: vec![Choice {
                    index: 0,
                    message: Some(ChatMessage {
                        role: "assistant".to_string(),
                        content: MessageContent::Text(format!("handled by {}", self.name)),
                        name: None,
                        tool_calls: None,
                        tool_call_id: None,
                    }),
                    delta: None,
                    finish_reason: Some("stop".to_string()),
                }],
                usage: None,
            }),
            StubCompletionResult::SuccessWithToolCalls(tool_calls) => Ok(ChatCompletionResponse {
                id: format!("{}-ok", self.name),
                object: "chat.completion".to_string(),
                created: 0,
                model: req.model.clone(),
                choices: vec![Choice {
                    index: 0,
                    message: Some(ChatMessage {
                        role: "assistant".to_string(),
                        content: MessageContent::Text(String::new()),
                        name: None,
                        tool_calls: Some(tool_calls.clone()),
                        tool_call_id: None,
                    }),
                    delta: None,
                    finish_reason: Some("tool_calls".to_string()),
                }],
                usage: None,
            }),
            StubCompletionResult::QuotaExceeded(message) => {
                Err(AppError::QuotaExceeded(message.clone()))
            }
        }
    }

    async fn chat_completion_stream(
        &self,
        req: &ChatCompletionRequest,
    ) -> rusuh::error::AppResult<BoxStream> {
        self.observed_models.lock().await.push(req.model.clone());
        match &self.result {
            StubCompletionResult::Success | StubCompletionResult::SuccessWithToolCalls(_) => {
                Ok(Box::pin(futures::stream::iter(vec![Ok(
                    Bytes::from_static(b"data: [DONE]\n\n"),
                )])))
            }
            StubCompletionResult::QuotaExceeded(message) => {
                Err(AppError::QuotaExceeded(message.clone()))
            }
        }
    }
}

fn make_ext_model(id: &str, owned_by: &str, provider_type: &str) -> ExtModelInfo {
    ExtModelInfo {
        id: id.to_string(),
        object: "model".to_string(),
        created: 0,
        owned_by: owned_by.to_string(),
        provider_type: provider_type.to_string(),
        display_name: None,
        name: Some(id.to_string()),
        version: None,
        description: None,
        input_token_limit: 0,
        output_token_limit: 0,
        supported_generation_methods: vec![],
        context_length: 0,
        max_completion_tokens: 0,
        supported_parameters: vec![],
        thinking: None,
        user_defined: false,
    }
}

fn test_app_with_state(state: Arc<ProxyState>) -> axum::Router {
    build_router(state.clone()).layer(axum::middleware::from_fn_with_state(
        state,
        rusuh::middleware::auth::api_key_auth,
    ))
}

fn test_state_with_providers(
    cfg: Config,
    registry: Arc<ModelRegistry>,
    providers: Vec<Arc<dyn Provider>>,
) -> Arc<ProxyState> {
    let accounts = Arc::new(AccountManager::with_dir("/tmp/rusuh_test_nonexistent"));
    let state = Arc::new(ProxyState::new(cfg, accounts, registry, providers.len()));
    futures::executor::block_on(async {
        state
            .publish_runtime_from_providers(providers)
            .await
            .expect("test providers should publish");
    });
    state
}

fn basic_chat_request(model: &str) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "test"}]
    })
}

#[derive(Debug)]
struct BlockingProvider {
    name: &'static str,
    client_id: String,
    models: Vec<ModelInfo>,
    list_started: Arc<Notify>,
    list_release: Arc<Notify>,
    chat_started: Arc<Notify>,
    chat_release: Arc<Notify>,
}

impl BlockingProvider {
    fn new(
        name: &'static str,
        client_id: impl Into<String>,
        model_ids: &[&str],
        list_started: Arc<Notify>,
        list_release: Arc<Notify>,
        chat_started: Arc<Notify>,
        chat_release: Arc<Notify>,
    ) -> Self {
        Self {
            name,
            client_id: client_id.into(),
            models: model_ids
                .iter()
                .map(|id| ModelInfo {
                    id: (*id).to_string(),
                    object: "model".to_string(),
                    created: 0,
                    owned_by: name.to_string(),
                })
                .collect(),
            list_started,
            list_release,
            chat_started,
            chat_release,
        }
    }
}

#[async_trait]
impl Provider for BlockingProvider {
    fn name(&self) -> &str {
        self.name
    }

    fn client_id(&self) -> &str {
        self.client_id.as_str()
    }

    async fn list_models(&self) -> rusuh::error::AppResult<Vec<ModelInfo>> {
        self.list_started.notify_one();
        self.list_release.notified().await;
        Ok(self.models.clone())
    }

    async fn chat_completion(
        &self,
        req: &ChatCompletionRequest,
    ) -> rusuh::error::AppResult<ChatCompletionResponse> {
        self.chat_started.notify_one();
        self.chat_release.notified().await;

        Ok(ChatCompletionResponse {
            id: format!("{}-ok", self.name),
            object: "chat.completion".to_string(),
            created: 0,
            model: req.model.clone(),
            choices: vec![Choice {
                index: 0,
                message: Some(ChatMessage {
                    role: "assistant".to_string(),
                    content: MessageContent::Text(format!("handled by {}", self.name)),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                }),
                delta: None,
                finish_reason: Some("stop".to_string()),
            }],
            usage: None,
        })
    }

    async fn chat_completion_stream(
        &self,
        _req: &ChatCompletionRequest,
    ) -> rusuh::error::AppResult<BoxStream> {
        unreachable!("streaming is not used in this test")
    }
}

#[tokio::test]
async fn health_always_accessible() {
    let app = test_app(Config::default());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn health_accessible_with_api_keys_set() {
    let cfg = Config {
        api_keys: vec!["secret-key".into()],
        ..Default::default()
    };

    let app = test_app(cfg);

    // No auth header — should still work for /health
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn auth_disabled_when_no_keys() {
    let app = test_app(Config::default());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn auth_rejects_missing_key() {
    let cfg = Config {
        api_keys: vec!["correct-key".into()],
        ..Default::default()
    };

    let app = test_app(cfg);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_rejects_wrong_key() {
    let cfg = Config {
        api_keys: vec!["correct-key".into()],
        ..Default::default()
    };

    let app = test_app(cfg);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header("Authorization", "Bearer wrong-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_accepts_bearer_token() {
    let cfg = Config {
        api_keys: vec!["my-key".into()],
        ..Default::default()
    };

    let app = test_app(cfg);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header("Authorization", "Bearer my-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn auth_accepts_x_api_key_header() {
    let cfg = Config {
        api_keys: vec!["my-key".into()],
        ..Default::default()
    };

    let app = test_app(cfg);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header("x-api-key", "my-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn models_returns_list() {
    let app = test_app(Config::default());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["object"], "list");
    assert!(json["data"].is_array());
}

#[tokio::test]
async fn public_claude_sonnet_4_6_does_not_use_kiro_alias_routing() {
    use rusuh::providers::model_info::ExtModelInfo;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();

    let auth = serde_json::json!({
        "type": "kiro",
        "provider_key": "kiro",
        "access_token": "token1",
        "refresh_token": "refresh1",
        "expires_at": "2030-01-01T00:00:00Z",
        "auth_method": "builder-id",
        "provider": "AWS",
        "region": "us-east-1",
        "client_id": "test-client-1",
        "client_secret": "secret1"
    });

    std::fs::write(
        dir.path().join("kiro-1.json"),
        serde_json::to_string_pretty(&auth).unwrap(),
    )
    .unwrap();

    let accounts = Arc::new(AccountManager::with_dir(dir.path()));
    accounts.reload().await.unwrap();

    let config = Config::default();
    let registry = Arc::new(ModelRegistry::new());
    let runtime = rusuh::proxy::KiroRuntimeState::default();
    let providers = rusuh::providers::registry::build_providers(
        &config,
        &accounts,
        registry.clone(),
        runtime.clone(),
    )
    .await;

    for (idx, provider) in providers.iter().enumerate() {
        let client_id = format!("{}_{}", provider.name(), idx);
        let models = vec![ExtModelInfo {
            id: "kiro-claude-sonnet-4-6".to_string(),
            object: "model".to_string(),
            created: 0,
            owned_by: "kiro".to_string(),
            provider_type: "kiro".to_string(),
            display_name: None,
            name: Some("kiro-claude-sonnet-4-6".to_string()),
            version: None,
            description: None,
            input_token_limit: 0,
            output_token_limit: 0,
            supported_generation_methods: vec![],
            context_length: 0,
            max_completion_tokens: 0,
            supported_parameters: vec![],
            thinking: None,
            user_defined: false,
        }];
        registry
            .register_client(&client_id, provider.name(), models)
            .await;
    }

    let mut state = ProxyState::new(config, accounts, registry, providers.len());
    state.kiro_runtime = runtime;
    let state = Arc::new(state);
    state
        .publish_runtime_from_providers(providers)
        .await
        .expect("test providers should publish");

    let body = serde_json::json!({
        "model": "claude-sonnet-4.6",
        "messages": [{"role": "user", "content": "test"}]
    });

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn chat_completions_rejects_non_public_model_on_public_endpoint() {
    let app = test_app(Config::default());

    let body = serde_json::json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "hi"}]
    });

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn responses_endpoint_rejects_non_public_model_on_public_endpoint() {
    let app = test_app(Config::default());

    let body = serde_json::json!({
        "model": "gpt-4",
        "input": "hello"
    });

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/responses")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn responses_compact_endpoint_rejects_non_public_model_on_public_endpoint() {
    let app = test_app(Config::default());

    let body = serde_json::json!({
        "model": "gpt-4",
        "input": "hello"
    });

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/responses/compact")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn responses_compact_handler_documents_intentional_parity() {
    let source = std::fs::read_to_string("src/proxy/handlers.rs").expect("read handlers source");

    assert!(
        source.contains("TODO: /v1/responses/compact currently matches /v1/responses intentionally"),
        "responses_compact should document why it currently routes identically"
    );
}

#[tokio::test]
async fn gemini_models_endpoint() {
    let app = test_app(Config::default());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1beta/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn gemini_models_fallback_uses_provider_models_when_registry_is_empty() {
    let kiro_observed = Arc::new(Mutex::new(Vec::new()));
    let zed_observed = Arc::new(Mutex::new(Vec::new()));
    let providers: Vec<Arc<dyn Provider>> = vec![
        Arc::new(StubProvider::success(
            "kiro",
            "kiro_0",
            &["kiro-claude-sonnet-4-5"],
            kiro_observed,
        )),
        Arc::new(StubProvider::success(
            "zed",
            "zed_0",
            &["claude-sonnet-4-6"],
            zed_observed,
        )),
    ];
    let app = test_app_with_state(test_state_with_providers(
        Config::default(),
        Arc::new(ModelRegistry::new()),
        providers,
    ));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1beta/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let names: Vec<String> = json["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap().to_string())
        .collect();

    assert_eq!(
        names,
        vec![
            "models/kiro-claude-sonnet-4-5".to_string(),
            "models/claude-sonnet-4-6".to_string(),
        ]
    );
}

#[tokio::test]
async fn generic_claude_model_routes_to_kiro_with_fallback() {
    use rusuh::providers::model_info::ExtModelInfo;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let auth = serde_json::json!({
        "type": "kiro",
        "access_token": "test-token",
        "expires_at": "2030-01-01T00:00:00Z",
        "auth_method": "builder-id",
        "provider": "AWS",
        "region": "us-east-1",
        "client_id": "test-client-1",
        "client_secret": "secret1"
    });

    std::fs::write(
        dir.path().join("kiro-1.json"),
        serde_json::to_string_pretty(&auth).unwrap(),
    )
    .unwrap();

    let accounts = Arc::new(AccountManager::with_dir(dir.path()));
    accounts.reload().await.unwrap();

    let config = Config::default();
    let registry = Arc::new(ModelRegistry::new());
    let runtime = rusuh::proxy::KiroRuntimeState::default();
    let providers = rusuh::providers::registry::build_providers(
        &config,
        &accounts,
        registry.clone(),
        runtime.clone(),
    )
    .await;

    // Register Kiro provider with claude-sonnet-4-6
    for (idx, provider) in providers.iter().enumerate() {
        let client_id = format!("{}_{}", provider.name(), idx);
        let models = vec![ExtModelInfo {
            id: "kiro-claude-sonnet-4-6".to_string(),
            object: "model".to_string(),
            created: 0,
            owned_by: "kiro".to_string(),
            provider_type: "kiro".to_string(),
            display_name: None,
            name: Some("kiro-claude-sonnet-4-6".to_string()),
            version: None,
            description: None,
            input_token_limit: 0,
            output_token_limit: 0,
            supported_generation_methods: vec![],
            context_length: 0,
            max_completion_tokens: 0,
            supported_parameters: vec![],
            thinking: None,
            user_defined: false,
        }];
        registry
            .register_client(&client_id, provider.name(), models)
            .await;

        // Mark Kiro provider as quota exceeded to trigger fallback
        registry
            .set_quota_exceeded(&client_id, "kiro-claude-sonnet-4-6")
            .await;
    }

    let mut state = ProxyState::new(config, accounts, registry, providers.len());
    state.kiro_runtime = runtime;
    let state = Arc::new(state);
    state
        .publish_runtime_from_providers(providers)
        .await
        .expect("test providers should publish");

    // Request with generic "claude-sonnet-4.6" name
    let body = serde_json::json!({
        "model": "claude-sonnet-4.6",
        "messages": [{"role": "user", "content": "test"}]
    });

    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Should get quota exceeded error since Kiro is unavailable and no other provider configured
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn public_route_failed_selected_auth_request_does_not_poison_execution_session() {
    let seen = Arc::new(Mutex::new(Vec::new()));

    let providers: Vec<Arc<dyn Provider>> = vec![Arc::new(StubProvider::success(
        "zed",
        "zed_0",
        &["claude-sonnet-4-6"],
        seen.clone(),
    ))];

    let registry = Arc::new(ModelRegistry::new());
    registry
        .register_client(
            "zed_0",
            "zed",
            vec![make_ext_model("claude-sonnet-4-6", "zed", "zed")],
        )
        .await;

    let app = test_app_with_state(test_state_with_providers(
        Config::default(),
        registry,
        providers,
    ));

    let initial_body = serde_json::json!({
        "model": "claude-sonnet-4.6",
        "selected_auth_id": "zed_99",
        "execution_session_id": "session-public",
        "messages": [{"role": "user", "content": "hello"}]
    });

    let initial_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&initial_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(initial_resp.status(), StatusCode::TOO_MANY_REQUESTS);

    let follow_up_body = serde_json::json!({
        "model": "claude-sonnet-4.6",
        "execution_session_id": "session-public",
        "messages": [{"role": "user", "content": "continue"}]
    });

    let follow_up_resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&follow_up_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(follow_up_resp.status(), StatusCode::OK);
    assert_eq!(seen.lock().await.as_slice(), &["claude-sonnet-4-6"]);
}

#[tokio::test]
async fn chat_execution_session_sticks_selected_auth_across_requests() {
    let first_seen = Arc::new(Mutex::new(Vec::new()));
    let second_seen = Arc::new(Mutex::new(Vec::new()));

    let providers: Vec<Arc<dyn Provider>> = vec![
        Arc::new(StubProvider::success(
            "codex",
            "codex_0",
            &["gpt-5-codex"],
            first_seen.clone(),
        )),
        Arc::new(StubProvider::success(
            "codex",
            "codex_1",
            &["gpt-5-codex"],
            second_seen.clone(),
        )),
    ];

    let registry = Arc::new(ModelRegistry::new());
    registry
        .register_client(
            "codex_0",
            "codex",
            vec![make_ext_model("gpt-5-codex", "codex", "codex")],
        )
        .await;
    registry
        .register_client(
            "codex_1",
            "codex",
            vec![make_ext_model("gpt-5-codex", "codex", "codex")],
        )
        .await;

    let app = test_app_with_state(test_state_with_providers(
        Config::default(),
        registry,
        providers,
    ));

    let initial_body = serde_json::json!({
        "model": "gpt-5-codex",
        "selected_auth_id": "codex_1",
        "execution_session_id": "session-http",
        "messages": [{"role": "user", "content": "hello"}]
    });

    let initial_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/provider/codex/v1/chat/completions")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&initial_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(initial_resp.status(), StatusCode::OK);

    let follow_up_body = serde_json::json!({
        "model": "gpt-5-codex",
        "execution_session_id": "session-http",
        "messages": [{"role": "user", "content": "continue"}]
    });

    let follow_up_resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/provider/codex/v1/chat/completions")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&follow_up_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(follow_up_resp.status(), StatusCode::OK);

    assert!(first_seen.lock().await.is_empty());
    assert_eq!(
        second_seen.lock().await.as_slice(),
        &["gpt-5-codex", "gpt-5-codex"]
    );
}

#[tokio::test]
async fn management_endpoint_skips_auth() {
    let cfg = Config {
        api_keys: vec!["secret".into()],
        ..Default::default()
    };

    let app = test_app(cfg);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v0/management/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Should not be 401 — management skips auth
    assert_ne!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn spa_fallback_does_not_override_api_routes() {
    let app = test_app(Config::default());

    let api_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(api_resp.status(), StatusCode::OK);

    let spa_resp = app
        .oneshot(
            Request::builder()
                .uri("/dashboard/overview")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(matches!(
        spa_resp.status(),
        StatusCode::OK | StatusCode::NOT_FOUND | StatusCode::INTERNAL_SERVER_ERROR
    ));
    assert_ne!(spa_resp.status(), StatusCode::UNAUTHORIZED);
}

// ── Kiro auth-aware load balancing tests ─────────────────────────────────────

#[tokio::test]
async fn kiro_routing_skips_quota_exceeded_auth() {
    use rusuh::providers::model_info::ExtModelInfo;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();

    // Create two Kiro auth files
    let auth1 = serde_json::json!({
        "type": "kiro",
        "provider_key": "kiro",
        "access_token": "token1",
        "refresh_token": "refresh1",
        "expires_at": "2030-01-01T00:00:00Z",
        "auth_method": "builder-id",
        "provider": "AWS",
        "region": "us-east-1",
        "client_id": "test-client-1",
        "client_secret": "secret1"
    });

    let auth2 = serde_json::json!({
        "type": "kiro",
        "provider_key": "kiro",
        "access_token": "token2",
        "refresh_token": "refresh2",
        "expires_at": "2030-01-01T00:00:00Z",
        "auth_method": "builder-id",
        "provider": "AWS",
        "region": "us-east-1",
        "client_id": "test-client-2",
        "client_secret": "secret2"
    });

    std::fs::write(
        dir.path().join("kiro-1.json"),
        serde_json::to_string_pretty(&auth1).unwrap(),
    )
    .unwrap();

    std::fs::write(
        dir.path().join("kiro-2.json"),
        serde_json::to_string_pretty(&auth2).unwrap(),
    )
    .unwrap();

    // Load accounts and build providers
    let accounts = Arc::new(AccountManager::with_dir(dir.path()));
    accounts.reload().await.unwrap();

    let config = Config::default();
    let registry = Arc::new(ModelRegistry::new());
    let runtime = rusuh::proxy::KiroRuntimeState::default();
    let providers = rusuh::providers::registry::build_providers(
        &config,
        &accounts,
        registry.clone(),
        runtime.clone(),
    )
    .await;

    // Register both providers with the same model
    let test_model = "claude-sonnet-4";

    for (idx, provider) in providers.iter().enumerate() {
        let client_id = format!("{}_{}", provider.name(), idx);
        let models = vec![ExtModelInfo {
            id: test_model.to_string(),
            object: "model".to_string(),
            created: 0,
            owned_by: "kiro".to_string(),
            provider_type: "kiro".to_string(),
            display_name: None,
            name: Some(test_model.to_string()),
            version: None,
            description: None,
            input_token_limit: 0,
            output_token_limit: 0,
            supported_generation_methods: vec![],
            context_length: 0,
            max_completion_tokens: 0,
            supported_parameters: vec![],
            thinking: None,
            user_defined: false,
        }];
        registry
            .register_client(&client_id, provider.name(), models)
            .await;
    }

    // Mark first provider as quota-exceeded
    registry.set_quota_exceeded("kiro_0", test_model).await;

    // Verify only second provider is available
    let available = registry.available_clients_for_model(test_model).await;
    assert_eq!(available.len(), 1);
    assert_eq!(available[0], "kiro_1");

    // Build app state
    let mut state = ProxyState::new(config, accounts, registry, providers.len());
    state.kiro_runtime = runtime;
    let state = Arc::new(state);
    state
        .publish_runtime_from_providers(providers)
        .await
        .expect("test providers should publish");

    // Make a request - should use kiro_1, not kiro_0
    let body = serde_json::json!({
        "model": test_model,
        "messages": [{"role": "user", "content": "test"}]
    });

    let app = build_router(state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Should attempt request (will fail due to fake tokens, but that's OK)
    // The key is that it should NOT return 429 "no providers available"
    // because kiro_1 is still available
    assert_ne!(
        resp.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "should not return 429 when one provider is still available"
    );
}

#[tokio::test]
async fn kiro_routing_skips_suspended_auth() {
    use rusuh::providers::model_info::ExtModelInfo;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();

    // Create two Kiro auth files
    let auth1 = serde_json::json!({
        "type": "kiro",
        "provider_key": "kiro",
        "access_token": "token1",
        "refresh_token": "refresh1",
        "expires_at": "2030-01-01T00:00:00Z",
        "auth_method": "builder-id",
        "provider": "AWS",
        "region": "us-east-1",
        "client_id": "test-client-1",
        "client_secret": "secret1"
    });

    let auth2 = serde_json::json!({
        "type": "kiro",
        "provider_key": "kiro",
        "access_token": "token2",
        "refresh_token": "refresh2",
        "expires_at": "2030-01-01T00:00:00Z",
        "auth_method": "builder-id",
        "provider": "AWS",
        "region": "us-east-1",
        "client_id": "test-client-2",
        "client_secret": "secret2"
    });

    std::fs::write(
        dir.path().join("kiro-1.json"),
        serde_json::to_string_pretty(&auth1).unwrap(),
    )
    .unwrap();

    std::fs::write(
        dir.path().join("kiro-2.json"),
        serde_json::to_string_pretty(&auth2).unwrap(),
    )
    .unwrap();

    // Load accounts and build providers
    let accounts = Arc::new(AccountManager::with_dir(dir.path()));
    accounts.reload().await.unwrap();

    let config = Config::default();
    let registry = Arc::new(ModelRegistry::new());
    let runtime = rusuh::proxy::KiroRuntimeState::default();
    let providers = rusuh::providers::registry::build_providers(
        &config,
        &accounts,
        registry.clone(),
        runtime.clone(),
    )
    .await;

    // Register both providers with the same model
    let test_model = "claude-sonnet-4";

    for (idx, provider) in providers.iter().enumerate() {
        let client_id = format!("{}_{}", provider.name(), idx);
        let models = vec![ExtModelInfo {
            id: test_model.to_string(),
            object: "model".to_string(),
            created: 0,
            owned_by: "kiro".to_string(),
            provider_type: "kiro".to_string(),
            display_name: None,
            name: Some(test_model.to_string()),
            version: None,
            description: None,
            input_token_limit: 0,
            output_token_limit: 0,
            supported_generation_methods: vec![],
            context_length: 0,
            max_completion_tokens: 0,
            supported_parameters: vec![],
            thinking: None,
            user_defined: false,
        }];
        registry
            .register_client(&client_id, provider.name(), models)
            .await;
    }

    // Suspend first provider
    registry
        .suspend_client_model("kiro_0", test_model, "test suspension")
        .await;

    // Verify only second provider is available
    let available = registry.available_clients_for_model(test_model).await;
    assert_eq!(available.len(), 1);
    assert_eq!(available[0], "kiro_1");

    // Build app state
    let mut state = ProxyState::new(config, accounts, registry, providers.len());
    state.kiro_runtime = runtime;
    let state = Arc::new(state);
    state
        .publish_runtime_from_providers(providers)
        .await
        .expect("test providers should publish");

    // Make a request - should use kiro_1, not kiro_0
    let body = serde_json::json!({
        "model": test_model,
        "messages": [{"role": "user", "content": "test"}]
    });

    let app = build_router(state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Should attempt request (will fail due to fake tokens, but that's OK)
    assert_ne!(
        resp.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "should not return 429 when one provider is still available"
    );
}

#[tokio::test]
async fn kiro_routing_returns_error_when_all_unavailable() {
    use rusuh::providers::model_info::ExtModelInfo;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();

    // Create one Kiro auth file
    let auth1 = serde_json::json!({
        "type": "kiro",
        "provider_key": "kiro",
        "access_token": "token1",
        "refresh_token": "refresh1",
        "expires_at": "2030-01-01T00:00:00Z",
        "auth_method": "builder-id",
        "provider": "AWS",
        "region": "us-east-1",
        "client_id": "test-client-1",
        "client_secret": "secret1"
    });

    std::fs::write(
        dir.path().join("kiro-1.json"),
        serde_json::to_string_pretty(&auth1).unwrap(),
    )
    .unwrap();

    // Load accounts and build providers
    let accounts = Arc::new(AccountManager::with_dir(dir.path()));
    accounts.reload().await.unwrap();

    let config = Config::default();
    let registry = Arc::new(ModelRegistry::new());
    let runtime = rusuh::proxy::KiroRuntimeState::default();
    let providers = rusuh::providers::registry::build_providers(
        &config,
        &accounts,
        registry.clone(),
        runtime.clone(),
    )
    .await;

    // Register provider with a model
    let test_model = "claude-sonnet-4";

    for (idx, provider) in providers.iter().enumerate() {
        let client_id = format!("{}_{}", provider.name(), idx);
        let models = vec![ExtModelInfo {
            id: test_model.to_string(),
            object: "model".to_string(),
            created: 0,
            owned_by: "kiro".to_string(),
            provider_type: "kiro".to_string(),
            display_name: None,
            name: Some(test_model.to_string()),
            version: None,
            description: None,
            input_token_limit: 0,
            output_token_limit: 0,
            supported_generation_methods: vec![],
            context_length: 0,
            max_completion_tokens: 0,
            supported_parameters: vec![],
            thinking: None,
            user_defined: false,
        }];
        registry
            .register_client(&client_id, provider.name(), models)
            .await;
    }

    // Mark provider as quota-exceeded
    registry.set_quota_exceeded("kiro_0", test_model).await;

    // Verify no providers are available
    let available = registry.available_clients_for_model(test_model).await;
    assert_eq!(available.len(), 0);

    // Build app state
    let mut state = ProxyState::new(config, accounts, registry, providers.len());
    state.kiro_runtime = runtime;
    let state = Arc::new(state);
    state
        .publish_runtime_from_providers(providers)
        .await
        .expect("test providers should publish");

    // Make a request - should return error
    let body = serde_json::json!({
        "model": test_model,
        "messages": [{"role": "user", "content": "test"}]
    });

    let app = build_router(state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn public_models_catalog_is_curated_to_three_router_models() {
    let zed_observed = Arc::new(Mutex::new(Vec::new()));
    let kiro_observed = Arc::new(Mutex::new(Vec::new()));
    let zed_client_id = "auth-zed-record-1";
    let kiro_client_id = "auth-kiro-record-1";

    let providers: Vec<Arc<dyn Provider>> = vec![
        Arc::new(StubProvider::success_with_type(
            "zed-display",
            "zed",
            zed_client_id,
            &["claude-sonnet-4-6", "claude-sonnet-4-5"],
            zed_observed,
        )),
        Arc::new(StubProvider::success_with_type(
            "kiro-display",
            "kiro",
            kiro_client_id,
            &["kiro-claude-sonnet-4-5", "kiro-claude-sonnet-4-5-agentic"],
            kiro_observed,
        )),
    ];

    let registry = Arc::new(ModelRegistry::new());
    registry
        .register_client(
            zed_client_id,
            "zed",
            vec![
                make_ext_model("claude-sonnet-4-6", "zed", "zed"),
                make_ext_model("claude-sonnet-4-5", "zed", "zed"),
            ],
        )
        .await;
    registry
        .register_client(
            kiro_client_id,
            "kiro",
            vec![
                make_ext_model("kiro-claude-sonnet-4-5", "kiro", "kiro"),
                make_ext_model("kiro-claude-sonnet-4-5-agentic", "kiro", "kiro"),
            ],
        )
        .await;

    let app = test_app_with_state(test_state_with_providers(
        Config::default(),
        registry,
        providers,
    ));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let ids: Vec<String> = json["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap().to_string())
        .collect();

    assert_eq!(
        ids,
        vec![
            "claude-sonnet-4.6".to_string(),
            "claude-sonnet-4.5".to_string(),
            "claude-sonnet-4.5-thinking".to_string(),
        ]
    );
}

#[tokio::test]
async fn provider_pinned_kiro_models_expose_only_raw_kiro_ids() {
    let kiro_first_observed = Arc::new(Mutex::new(Vec::new()));
    let kiro_second_observed = Arc::new(Mutex::new(Vec::new()));
    let zed_observed = Arc::new(Mutex::new(Vec::new()));
    let providers: Vec<Arc<dyn Provider>> = vec![
        Arc::new(StubProvider::success(
            "kiro",
            "kiro_0",
            &["kiro-claude-sonnet-4-5", "kiro-claude-sonnet-4-5-agentic"],
            kiro_first_observed,
        )),
        Arc::new(StubProvider::success(
            "kiro",
            "kiro_1",
            &["kiro-claude-sonnet-4-5", "kiro-claude-sonnet-4-5-agentic"],
            kiro_second_observed,
        )),
        Arc::new(StubProvider::success(
            "zed",
            "zed_0",
            &["claude-sonnet-4-6", "claude-sonnet-4-5"],
            zed_observed,
        )),
    ];
    let app = test_app_with_state(test_state_with_providers(
        Config::default(),
        Arc::new(ModelRegistry::new()),
        providers,
    ));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/provider/kiro/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let ids: Vec<String> = json["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap().to_string())
        .collect();

    assert_eq!(
        ids,
        vec![
            "kiro-claude-sonnet-4-5".to_string(),
            "kiro-claude-sonnet-4-5-agentic".to_string(),
        ]
    );
}

#[tokio::test]
async fn provider_pinned_zed_models_expose_only_raw_zed_ids() {
    let kiro_observed = Arc::new(Mutex::new(Vec::new()));
    let zed_first_observed = Arc::new(Mutex::new(Vec::new()));
    let zed_second_observed = Arc::new(Mutex::new(Vec::new()));
    let providers: Vec<Arc<dyn Provider>> = vec![
        Arc::new(StubProvider::success(
            "kiro",
            "kiro_0",
            &["kiro-claude-sonnet-4-5", "kiro-claude-sonnet-4-5-agentic"],
            kiro_observed,
        )),
        Arc::new(StubProvider::success(
            "zed",
            "zed_0",
            &["claude-sonnet-4-6", "claude-sonnet-4-5"],
            zed_first_observed,
        )),
        Arc::new(StubProvider::success(
            "zed",
            "zed_1",
            &["claude-sonnet-4-6", "claude-sonnet-4-5"],
            zed_second_observed,
        )),
    ];
    let app = test_app_with_state(test_state_with_providers(
        Config::default(),
        Arc::new(ModelRegistry::new()),
        providers,
    ));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/provider/zed/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let ids: Vec<String> = json["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap().to_string())
        .collect();

    assert_eq!(
        ids,
        vec![
            "claude-sonnet-4-6".to_string(),
            "claude-sonnet-4-5".to_string(),
        ]
    );
}

#[tokio::test]
async fn public_endpoint_rejects_provider_native_model_ids() {
    let kiro_seen = Arc::new(Mutex::new(Vec::new()));
    let zed_seen = Arc::new(Mutex::new(Vec::new()));
    let providers: Vec<Arc<dyn Provider>> = vec![
        Arc::new(StubProvider::success(
            "kiro",
            "kiro_0",
            &["kiro-claude-sonnet-4-5"],
            kiro_seen.clone(),
        )),
        Arc::new(StubProvider::success(
            "zed",
            "zed_1",
            &["claude-sonnet-4-5"],
            zed_seen.clone(),
        )),
    ];
    let registry = Arc::new(ModelRegistry::new());
    registry
        .register_client(
            "kiro_0",
            "kiro",
            vec![make_ext_model("kiro-claude-sonnet-4-5", "kiro", "kiro")],
        )
        .await;
    registry
        .register_client(
            "zed_1",
            "zed",
            vec![make_ext_model("claude-sonnet-4-5", "zed", "zed")],
        )
        .await;

    let app = test_app_with_state(test_state_with_providers(
        Config::default(),
        registry,
        providers,
    ));

    for model in ["kiro-claude-sonnet-4-5", "claude-sonnet-4-5"] {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&basic_chat_request(model)).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    assert!(kiro_seen.lock().await.is_empty());
    assert!(zed_seen.lock().await.is_empty());
}

#[tokio::test]
async fn public_claude_sonnet_4_6_routes_only_to_zed_native_model() {
    let zed_seen = Arc::new(Mutex::new(Vec::new()));
    let kiro_seen = Arc::new(Mutex::new(Vec::new()));
    let providers: Vec<Arc<dyn Provider>> = vec![
        Arc::new(StubProvider::success(
            "kiro",
            "kiro_0",
            &["kiro-claude-sonnet-4-6"],
            kiro_seen.clone(),
        )),
        Arc::new(StubProvider::success(
            "zed",
            "zed_1",
            &["claude-sonnet-4-6"],
            zed_seen.clone(),
        )),
    ];
    let registry = Arc::new(ModelRegistry::new());
    registry
        .register_client(
            "kiro_0",
            "kiro",
            vec![make_ext_model("kiro-claude-sonnet-4-6", "kiro", "kiro")],
        )
        .await;
    registry
        .register_client(
            "zed_1",
            "zed",
            vec![make_ext_model("claude-sonnet-4-6", "zed", "zed")],
        )
        .await;

    let app = test_app_with_state(test_state_with_providers(
        Config::default(),
        registry,
        providers,
    ));
    let body = basic_chat_request("claude-sonnet-4.6");

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert!(kiro_seen.lock().await.is_empty());
    assert_eq!(zed_seen.lock().await.as_slice(), &["claude-sonnet-4-6"]);
}

#[tokio::test]
async fn public_claude_sonnet_4_5_routes_kiro_first_then_falls_back_to_zed() {
    let kiro_seen = Arc::new(Mutex::new(Vec::new()));
    let zed_seen = Arc::new(Mutex::new(Vec::new()));
    let providers: Vec<Arc<dyn Provider>> = vec![
        Arc::new(StubProvider::quota_exceeded(
            "kiro",
            "kiro_0",
            &["kiro-claude-sonnet-4-5"],
            kiro_seen.clone(),
            "kiro unavailable",
        )),
        Arc::new(StubProvider::success(
            "zed",
            "zed_1",
            &["claude-sonnet-4-5"],
            zed_seen.clone(),
        )),
    ];
    let registry = Arc::new(ModelRegistry::new());
    registry
        .register_client(
            "kiro_0",
            "kiro",
            vec![make_ext_model("kiro-claude-sonnet-4-5", "kiro", "kiro")],
        )
        .await;
    registry
        .register_client(
            "zed_1",
            "zed",
            vec![make_ext_model("claude-sonnet-4-5", "zed", "zed")],
        )
        .await;

    let app = test_app_with_state(test_state_with_providers(
        Config::default(),
        registry,
        providers,
    ));
    let body = basic_chat_request("claude-sonnet-4.5");

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        kiro_seen.lock().await.as_slice(),
        &["kiro-claude-sonnet-4-5"]
    );
    assert_eq!(zed_seen.lock().await.as_slice(), &["claude-sonnet-4-5"]);
}

#[tokio::test]
async fn public_thinking_model_stays_on_kiro_without_zed_fallback() {
    let kiro_first_seen = Arc::new(Mutex::new(Vec::new()));
    let kiro_second_seen = Arc::new(Mutex::new(Vec::new()));
    let zed_seen = Arc::new(Mutex::new(Vec::new()));
    let providers: Vec<Arc<dyn Provider>> = vec![
        Arc::new(StubProvider::quota_exceeded(
            "kiro",
            "kiro_0",
            &["kiro-claude-sonnet-4-5-agentic"],
            kiro_first_seen.clone(),
            "first kiro unavailable",
        )),
        Arc::new(StubProvider::success(
            "kiro",
            "kiro_1",
            &["kiro-claude-sonnet-4-5-agentic"],
            kiro_second_seen.clone(),
        )),
        Arc::new(StubProvider::success(
            "zed",
            "zed_0",
            &["claude-sonnet-4-5"],
            zed_seen.clone(),
        )),
    ];
    let registry = Arc::new(ModelRegistry::new());
    registry
        .register_client(
            "kiro_0",
            "kiro",
            vec![make_ext_model(
                "kiro-claude-sonnet-4-5-agentic",
                "kiro",
                "kiro",
            )],
        )
        .await;
    registry
        .register_client(
            "kiro_1",
            "kiro",
            vec![make_ext_model(
                "kiro-claude-sonnet-4-5-agentic",
                "kiro",
                "kiro",
            )],
        )
        .await;
    registry
        .register_client(
            "zed_2",
            "zed",
            vec![make_ext_model("claude-sonnet-4-5", "zed", "zed")],
        )
        .await;

    let app = test_app_with_state(test_state_with_providers(
        Config::default(),
        registry,
        providers,
    ));
    let body = basic_chat_request("claude-sonnet-4.5-thinking");

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        kiro_first_seen.lock().await.as_slice(),
        &["kiro-claude-sonnet-4-5-agentic"]
    );
    assert_eq!(
        kiro_second_seen.lock().await.as_slice(),
        &["kiro-claude-sonnet-4-5-agentic"]
    );
    assert!(zed_seen.lock().await.is_empty());
}

#[tokio::test]
async fn public_claude_sonnet_4_5_non_stream_returns_chat_completion_response() {
    let kiro_seen = Arc::new(Mutex::new(Vec::new()));
    let providers: Vec<Arc<dyn Provider>> = vec![Arc::new(StubProvider::success(
        "kiro",
        "kiro_0",
        &["kiro-claude-sonnet-4-5"],
        kiro_seen.clone(),
    ))];
    let registry = Arc::new(ModelRegistry::new());
    registry
        .register_client(
            "kiro_0",
            "kiro",
            vec![make_ext_model("kiro-claude-sonnet-4-5", "kiro", "kiro")],
        )
        .await;

    let app = test_app_with_state(test_state_with_providers(
        Config::default(),
        registry,
        providers,
    ));
    let body = serde_json::json!({
        "model": "claude-sonnet-4.5",
        "stream": false,
        "messages": [{"role": "user", "content": "hello"}]
    });

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        kiro_seen.lock().await.as_slice(),
        &["kiro-claude-sonnet-4-5"]
    );

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["object"], "chat.completion");
    assert_eq!(json["choices"].as_array().map(Vec::len), Some(1));
    assert_eq!(json["choices"][0]["message"]["role"], "assistant");
}

#[tokio::test]
async fn public_claude_sonnet_4_5_non_stream_preserves_final_tool_calls() {
    let kiro_seen = Arc::new(Mutex::new(Vec::new()));
    let tool_calls = vec![serde_json::json!({
        "id": "call_123",
        "type": "function",
        "function": {
            "name": "lookup_weather",
            "arguments": "{\"city\":\"Jakarta\"}"
        }
    })];
    let providers: Vec<Arc<dyn Provider>> = vec![Arc::new(StubProvider::success_with_tool_calls(
        "kiro",
        "kiro_0",
        &["kiro-claude-sonnet-4-5"],
        kiro_seen.clone(),
        tool_calls.clone(),
    ))];
    let registry = Arc::new(ModelRegistry::new());
    registry
        .register_client(
            "kiro_0",
            "kiro",
            vec![make_ext_model("kiro-claude-sonnet-4-5", "kiro", "kiro")],
        )
        .await;

    let app = test_app_with_state(test_state_with_providers(
        Config::default(),
        registry,
        providers,
    ));
    let body = serde_json::json!({
        "model": "claude-sonnet-4.5",
        "stream": false,
        "messages": [{"role": "user", "content": "hello"}]
    });

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        kiro_seen.lock().await.as_slice(),
        &["kiro-claude-sonnet-4-5"]
    );

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["object"], "chat.completion");
    assert_eq!(
        json["choices"][0]["message"]["tool_calls"],
        serde_json::Value::Array(tool_calls)
    );
}

#[test]
fn blocking_provider_uses_explicit_client_id() {
    let provider = BlockingProvider::new(
        "codex",
        "codex_0",
        &["gpt-5-codex"],
        Arc::new(Notify::new()),
        Arc::new(Notify::new()),
        Arc::new(Notify::new()),
        Arc::new(Notify::new()),
    );

    assert_eq!(provider.client_id(), "codex_0");
}

#[tokio::test]
async fn provider_models_request_does_not_hold_providers_lock_while_listing_models() {
    let list_started = Arc::new(Notify::new());
    let list_release = Arc::new(Notify::new());
    let chat_started = Arc::new(Notify::new());
    let chat_release = Arc::new(Notify::new());

    let providers: Vec<Arc<dyn Provider>> = vec![Arc::new(BlockingProvider::new(
        "codex",
        "codex_0",
        &["gpt-5-codex"],
        list_started.clone(),
        list_release.clone(),
        chat_started,
        chat_release,
    ))];
    let registry = Arc::new(ModelRegistry::new());
    let state = test_state_with_providers(Config::default(), registry, providers);
    let app = test_app_with_state(state.clone());

    let request = Request::builder()
        .uri("/api/provider/codex/v1/models")
        .body(Body::empty())
        .unwrap();

    let request_task = tokio::spawn(async move { app.oneshot(request).await.unwrap() });

    list_started.notified().await;

    let write_attempt = tokio::spawn({
        let state = state.clone();
        async move {
            let _guard = state.providers.write().await;
        }
    });

    tokio::time::timeout(std::time::Duration::from_millis(100), write_attempt)
        .await
        .expect("providers write lock should not be blocked by model listing")
        .expect("write-lock task should complete successfully");

    list_release.notify_waiters();

    let resp = request_task.await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn provider_chat_request_does_not_hold_providers_lock_while_awaiting_upstream() {
    let list_started = Arc::new(Notify::new());
    let list_release = Arc::new(Notify::new());
    let chat_started = Arc::new(Notify::new());
    let chat_release = Arc::new(Notify::new());

    let providers: Vec<Arc<dyn Provider>> = vec![Arc::new(BlockingProvider::new(
        "codex",
        "codex_0",
        &["gpt-5-codex"],
        list_started,
        list_release,
        chat_started.clone(),
        chat_release.clone(),
    ))];
    let registry = Arc::new(ModelRegistry::new());
    registry
        .register_client(
            "codex_0",
            "codex",
            vec![make_ext_model("gpt-5-codex", "codex", "codex")],
        )
        .await;
    let state = test_state_with_providers(Config::default(), registry, providers);
    let app = test_app_with_state(state.clone());

    let body = serde_json::json!({
        "model": "gpt-5-codex",
        "messages": [{"role": "user", "content": "hello"}]
    });
    let request = Request::builder()
        .method("POST")
        .uri("/api/provider/codex/v1/chat/completions")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let request_task = tokio::spawn(async move { app.oneshot(request).await.unwrap() });

    chat_started.notified().await;

    let write_attempt = tokio::spawn({
        let state = state.clone();
        async move {
            let _guard = state.providers.write().await;
        }
    });

    tokio::time::timeout(std::time::Duration::from_millis(100), write_attempt)
        .await
        .expect("providers write lock should not be blocked by chat completion")
        .expect("write-lock task should complete successfully");

    chat_release.notify_waiters();

    let resp = request_task.await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn blocking_provider_start_signals_are_not_lost_if_observed_after_spawn() {
    let list_started = Arc::new(Notify::new());
    let list_release = Arc::new(Notify::new());
    let chat_started = Arc::new(Notify::new());
    let chat_release = Arc::new(Notify::new());
    let provider = BlockingProvider::new(
        "codex",
        "codex_0",
        &["gpt-5-codex"],
        list_started.clone(),
        list_release.clone(),
        chat_started.clone(),
        chat_release.clone(),
    );

    let list_task = tokio::spawn(async move { provider.list_models().await });
    tokio::task::yield_now().await;
    tokio::time::timeout(
        std::time::Duration::from_millis(50),
        list_started.notified(),
    )
    .await
    .expect("list_started signal should be retained for a later waiter");
    list_release.notify_waiters();
    assert!(list_task.await.unwrap().is_ok());

    let provider = BlockingProvider::new(
        "codex",
        "codex_0",
        &["gpt-5-codex"],
        Arc::new(Notify::new()),
        Arc::new(Notify::new()),
        chat_started.clone(),
        chat_release.clone(),
    );
    let req = ChatCompletionRequest {
        model: "gpt-5-codex".to_string(),
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: MessageContent::Text("hello".to_string()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }],
        stream: None,
        max_tokens: None,
        temperature: None,
        top_p: None,
        tools: None,
        tool_choice: None,
        stop: None,
        extra: std::collections::HashMap::new(),
    };

    let chat_task = tokio::spawn(async move { provider.chat_completion(&req).await });
    tokio::task::yield_now().await;
    tokio::time::timeout(
        std::time::Duration::from_millis(50),
        chat_started.notified(),
    )
    .await
    .expect("chat_started signal should be retained for a later waiter");
    chat_release.notify_waiters();
    assert!(chat_task.await.unwrap().is_ok());
}
