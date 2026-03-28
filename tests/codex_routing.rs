use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use bytes::Bytes;
use tokio::sync::Mutex;
use tower::ServiceExt;

use rusuh::auth::manager::AccountManager;
use rusuh::config::Config;
use rusuh::models::{
    ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse, ChatMessage, Choice,
    DeltaContent, MessageContent, ModelInfo, StreamChoice,
};
use rusuh::providers::model_info::ExtModelInfo;
use rusuh::providers::model_registry::ModelRegistry;
use rusuh::providers::{BoxStream, Provider};
use rusuh::proxy::ProxyState;
use rusuh::router::build_router;

#[derive(Debug)]
struct StubProvider {
    name: &'static str,
    models: Vec<ModelInfo>,
    observed_models: Arc<Mutex<Vec<String>>>,
    response_label: &'static str,
}

impl StubProvider {
    fn success(
        name: &'static str,
        model_ids: &[&str],
        observed_models: Arc<Mutex<Vec<String>>>,
        response_label: &'static str,
    ) -> Self {
        Self {
            name,
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
            response_label,
        }
    }
}

#[async_trait]
impl Provider for StubProvider {
    fn name(&self) -> &str {
        self.name
    }

    async fn list_models(&self) -> rusuh::error::AppResult<Vec<ModelInfo>> {
        Ok(self.models.clone())
    }

    async fn chat_completion(
        &self,
        req: &ChatCompletionRequest,
    ) -> rusuh::error::AppResult<ChatCompletionResponse> {
        self.observed_models.lock().await.push(req.model.clone());

        Ok(ChatCompletionResponse {
            id: format!("{}-ok", self.name),
            object: "chat.completion".to_string(),
            created: 0,
            model: req.model.clone(),
            choices: vec![Choice {
                index: 0,
                message: Some(ChatMessage {
                    role: "assistant".to_string(),
                    content: MessageContent::Text(self.response_label.to_string()),
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
        req: &ChatCompletionRequest,
    ) -> rusuh::error::AppResult<BoxStream> {
        self.observed_models.lock().await.push(req.model.clone());
        let chunk = ChatCompletionChunk {
            id: format!("{}-stream", self.name),
            object: "chat.completion.chunk".to_string(),
            created: 0,
            model: req.model.clone(),
            choices: vec![StreamChoice {
                index: 0,
                delta: DeltaContent {
                    role: Some("assistant".to_string()),
                    content: Some(self.response_label.to_string()),
                    tool_calls: None,
                },
                finish_reason: None,
            }],
        };
        let payload = serde_json::to_string(&chunk)
            .map_err(|error| rusuh::error::AppError::Internal(error.into()))?;
        let sse = format!("data: {payload}\\n\\ndata: [DONE]\\n\\n");
        Ok(Box::pin(futures::stream::iter(vec![Ok(Bytes::from(sse))])))
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
    let mut state = ProxyState::new(cfg, accounts, registry, providers.len());
    state.providers = tokio::sync::RwLock::new(providers);
    Arc::new(state)
}

#[tokio::test]
async fn selected_auth_id_routes_to_requested_codex_client() {
    let first_seen = Arc::new(Mutex::new(Vec::new()));
    let second_seen = Arc::new(Mutex::new(Vec::new()));

    let providers: Vec<Arc<dyn Provider>> = vec![
        Arc::new(StubProvider::success(
            "codex",
            &["gpt-5-codex"],
            first_seen.clone(),
            "handled by first",
        )),
        Arc::new(StubProvider::success(
            "codex",
            &["gpt-5-codex"],
            second_seen.clone(),
            "handled by second",
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

    let body = serde_json::json!({
        "model": "gpt-5-codex",
        "selected_auth_id": "codex_1",
        "messages": [{"role": "user", "content": "hello"}]
    });

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/provider/codex/v1/chat/completions")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert!(first_seen.lock().await.is_empty());
    assert_eq!(second_seen.lock().await.as_slice(), &["gpt-5-codex"]);
}

#[tokio::test]
async fn unknown_selected_auth_id_is_rejected() {
    let first_seen = Arc::new(Mutex::new(Vec::new()));
    let providers: Vec<Arc<dyn Provider>> = vec![Arc::new(StubProvider::success(
        "codex",
        &["gpt-5-codex"],
        first_seen.clone(),
        "handled by first",
    ))];

    let registry = Arc::new(ModelRegistry::new());
    registry
        .register_client(
            "codex_0",
            "codex",
            vec![make_ext_model("gpt-5-codex", "codex", "codex")],
        )
        .await;

    let app = test_app_with_state(test_state_with_providers(
        Config::default(),
        registry,
        providers,
    ));

    let body = serde_json::json!({
        "model": "gpt-5-codex",
        "selected_auth_id": "codex_99",
        "messages": [{"role": "user", "content": "hello"}]
    });

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/provider/codex/v1/chat/completions")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(first_seen.lock().await.is_empty());
}

#[tokio::test]
async fn execution_session_sticks_to_first_selected_auth() {
    let first_seen = Arc::new(Mutex::new(Vec::new()));
    let second_seen = Arc::new(Mutex::new(Vec::new()));

    let providers: Vec<Arc<dyn Provider>> = vec![
        Arc::new(StubProvider::success(
            "codex",
            &["gpt-5-codex"],
            first_seen.clone(),
            "handled by first",
        )),
        Arc::new(StubProvider::success(
            "codex",
            &["gpt-5-codex"],
            second_seen.clone(),
            "handled by second",
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

    let first_request = serde_json::json!({
        "model": "gpt-5-codex",
        "execution_session_id": "session-sticky",
        "messages": [{"role": "user", "content": "hello"}]
    });

    let first_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/provider/codex/v1/chat/completions")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&first_request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first_resp.status(), StatusCode::OK);

    let second_request = serde_json::json!({
        "model": "gpt-5-codex",
        "execution_session_id": "session-sticky",
        "messages": [{"role": "user", "content": "again"}]
    });

    let second_resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/provider/codex/v1/chat/completions")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&second_request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second_resp.status(), StatusCode::OK);

    assert_eq!(
        first_seen.lock().await.as_slice(),
        &["gpt-5-codex", "gpt-5-codex"]
    );
    assert!(second_seen.lock().await.is_empty());
}

#[tokio::test]
async fn failed_selected_auth_request_does_not_poison_execution_session() {
    let seen = Arc::new(Mutex::new(Vec::new()));

    let providers: Vec<Arc<dyn Provider>> = vec![Arc::new(StubProvider::success(
        "codex",
        &["gpt-5-codex"],
        seen.clone(),
        "handled by first",
    ))];

    let registry = Arc::new(ModelRegistry::new());
    registry
        .register_client(
            "codex_0",
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
        "execution_session_id": "session-unpoisoned",
        "selected_auth_id": "codex_99",
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
    assert_eq!(initial_resp.status(), StatusCode::TOO_MANY_REQUESTS);

    let follow_up_body = serde_json::json!({
        "model": "gpt-5-codex",
        "execution_session_id": "session-unpoisoned",
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
    assert_eq!(seen.lock().await.as_slice(), &["gpt-5-codex"]);
}

#[tokio::test]
async fn responses_execution_session_prefers_previous_selected_auth() {
    let first_seen = Arc::new(Mutex::new(Vec::new()));
    let second_seen = Arc::new(Mutex::new(Vec::new()));

    let providers: Vec<Arc<dyn Provider>> = vec![
        Arc::new(StubProvider::success(
            "codex",
            &["gpt-5-codex"],
            first_seen.clone(),
            "handled by first",
        )),
        Arc::new(StubProvider::success(
            "codex",
            &["gpt-5-codex"],
            second_seen.clone(),
            "handled by second",
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
        "execution_session_id": "session-a",
        "selected_auth_id": "codex_1",
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
        "execution_session_id": "session-a",
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
