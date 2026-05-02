use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::{http::StatusCode, routing::post, Json, Router};
use tempfile::TempDir;
use tokio::sync::Notify;

use rusuh::auth::manager::AccountManager;
use rusuh::config::Config;
use rusuh::error::AppError;
use rusuh::providers::model_info::ExtModelInfo;
use rusuh::providers::model_registry::ModelRegistry;
use rusuh::proxy::ProxyState;

fn make_model(id: &str, provider: &str) -> ExtModelInfo {
    ExtModelInfo {
        id: id.to_string(),
        object: "model".into(),
        created: 0,
        owned_by: provider.to_string(),
        provider_type: provider.to_string(),
        display_name: None,
        name: None,
        version: None,
        description: None,
        input_token_limit: 0,
        output_token_limit: 0,
        supported_generation_methods: vec![],
        context_length: 0,
        max_completion_tokens: 0,
        supported_parameters: vec![],
        supported_endpoints: None,
        thinking: None,
        user_defined: false,
    }
}

async fn spawn_failing_antigravity_models_server() -> String {
    let app = Router::new().route(
        "/v1internal:fetchAvailableModels",
        post(|| async {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": "boom"})),
            )
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    format!("http://{addr}")
}

async fn spawn_antigravity_models_server(model_id: &str) -> String {
    let model_id = model_id.to_string();
    let app = Router::new().route(
        "/v1internal:fetchAvailableModels",
        post(move || {
            let model_id = model_id.clone();
            async move {
                (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "models": {
                            model_id: {"state": "ENABLED"}
                        }
                    })),
                )
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    format!("http://{addr}")
}

async fn spawn_blocked_antigravity_models_server(
    model_id: &str,
    request_started: Arc<Notify>,
    release_request: Arc<Notify>,
) -> String {
    let model_id = model_id.to_string();
    let first_request_is_blocked = Arc::new(AtomicBool::new(true));
    let app = Router::new().route(
        "/v1internal:fetchAvailableModels",
        post(move || {
            let model_id = model_id.clone();
            let request_started = request_started.clone();
            let release_request = release_request.clone();
            let first_request_is_blocked = first_request_is_blocked.clone();
            async move {
                if first_request_is_blocked.swap(false, Ordering::SeqCst) {
                    request_started.notify_one();
                    release_request.notified().await;
                }
                (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "models": {
                            model_id: {"state": "ENABLED"}
                        }
                    })),
                )
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    format!("http://{addr}")
}

async fn spawn_failing_zed_server() -> String {
    let app = Router::new()
        .route(
            "/client/llm_tokens",
            post(|| async {
                (
                    StatusCode::OK,
                    Json(serde_json::json!({"token": "bad-token"})),
                )
            }),
        )
        .route(
            "/models",
            axum::routing::get(|| async {
                (
                    StatusCode::UNAUTHORIZED,
                    [("x-zed-expired-token", "1")],
                    Json(serde_json::json!({"error": "expired"})),
                )
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    format!("http://{addr}")
}

#[tokio::test]
async fn refresh_provider_runtime_keeps_existing_registration_when_replacement_listing_fails() {
    let dir = TempDir::new().unwrap();
    let failing_base_url = spawn_failing_antigravity_models_server().await;

    let auth_json = serde_json::json!({
        "type": "antigravity",
        "provider_key": "antigravity",
        "email": "test@example.com",
        "access_token": "ya29.test",
        "refresh_token": "1//test",
        "project_id": "test-project",
        "base_url": failing_base_url,
        "expired": "2030-01-01T00:00:00Z"
    });
    std::fs::write(
        dir.path().join("antigravity-test.json"),
        serde_json::to_string_pretty(&auth_json).unwrap(),
    )
    .unwrap();

    let accounts = Arc::new(AccountManager::with_dir(dir.path()));
    accounts.reload().await.unwrap();
    let registry = Arc::new(ModelRegistry::new());
    registry
        .register_client(
            "antigravity_0",
            "antigravity",
            vec![make_model("gemini-2.5-pro", "antigravity")],
        )
        .await;

    let state = ProxyState::new(Config::default(), accounts.clone(), registry.clone(), 1);
    let existing_providers = rusuh::providers::registry::build_providers(
        &Config::default(),
        &accounts,
        registry.clone(),
        state.kiro_runtime.clone(),
    )
    .await;
    state
        .publish_runtime_from_providers(existing_providers)
        .await
        .unwrap();

    state.refresh_provider_runtime().await.unwrap();

    assert_eq!(registry.get_model_count("gemini-2.5-pro").await, 1);
    assert_eq!(
        registry.available_clients_for_model("gemini-2.5-pro").await,
        vec!["antigravity_0".to_string()]
    );
}

#[test]
fn supported_endpoints_survive_model_registration_and_lookup() {
    let runtime = tokio::runtime::Runtime::new().expect("create runtime");
    runtime.block_on(async {
        let registry = ModelRegistry::new();
        let mut model = make_model("claude-3.7-sonnet", "github-copilot");
        model.supported_endpoints = Some(vec!["chat/completions".into(), "responses".into()]);

        registry
            .register_client("github-copilot_0", "github-copilot", vec![model])
            .await;

        let stored = registry
            .lookup_model_info("claude-3.7-sonnet", "github-copilot")
            .await
            .expect("model should exist");

        assert_eq!(
            stored.supported_endpoints,
            Some(vec!["chat/completions".into(), "responses".into()])
        );
    });
}

#[test]
fn github_copilot_static_metadata_marks_responses_only_models() {
    let codex = rusuh::providers::static_models::lookup_static_model("gpt-5-codex")
        .expect("static copilot codex metadata should exist");

    assert_eq!(codex.provider_type, "github-copilot");
    assert_eq!(
        codex.supported_endpoints,
        Some(vec!["responses".to_string()])
    );
}

#[test]
fn config_injects_github_copilot_dotted_claude_aliases() {
    let cfg = Config::default();
    let aliases = cfg
        .oauth_model_alias
        .get("github-copilot")
        .expect("github-copilot aliases should be injected");

    assert!(aliases.iter().any(|entry| {
        entry.alias == "claude-sonnet-4.5" && entry.name == "claude-sonnet-4-5"
    }));
    assert!(aliases.iter().any(|entry| {
        entry.alias == "claude-sonnet-4.5-thinking"
            && entry.name == "claude-sonnet-4-5-thinking"
    }));
}

#[test]
fn static_metadata_enriches_live_models_without_inventing_extra_visible_models() {
    let runtime = tokio::runtime::Runtime::new().expect("create runtime");
    runtime.block_on(async {
        let registry = ModelRegistry::new();
        registry
            .register_client(
                "github-copilot_0",
                "github-copilot",
                vec![make_model("claude-sonnet-4-5", "github-copilot")],
            )
            .await;

        let available = registry.get_available_models("openai").await;
        assert_eq!(available.len(), 1);
        assert_eq!(available[0]["id"], "claude-sonnet-4-5");

        let info = registry
            .lookup_model_info("claude-sonnet-4-5", "github-copilot")
            .await
            .expect("registered live model should be returned");
        assert_eq!(info.id, "claude-sonnet-4-5");
        assert_eq!(info.provider_type, "github-copilot");
    });
}

#[tokio::test]
async fn refresh_provider_runtime_removes_orphaned_clients() {
    let dir = TempDir::new().unwrap();
    let antigravity_base_url = spawn_antigravity_models_server("gemini-2.5-pro").await;

    let auth_json = serde_json::json!({
        "type": "antigravity",
        "provider_key": "antigravity",
        "email": "test@example.com",
        "access_token": "ya29.test",
        "refresh_token": "1//test",
        "project_id": "test-project",
        "base_url": antigravity_base_url,
        "expired": "2030-01-01T00:00:00Z"
    });
    std::fs::write(
        dir.path().join("antigravity-test.json"),
        serde_json::to_string_pretty(&auth_json).unwrap(),
    )
    .unwrap();

    let accounts = Arc::new(AccountManager::with_dir(dir.path()));
    accounts.reload().await.unwrap();
    let registry = Arc::new(ModelRegistry::new());
    registry
        .register_client(
            "antigravity-test.json",
            "antigravity",
            vec![make_model("gemini-2.5-pro", "antigravity")],
        )
        .await;

    let config = Config::default();
    let state = ProxyState::new(config.clone(), accounts.clone(), registry.clone(), 1);
    let existing_providers = rusuh::providers::registry::build_providers(
        &config,
        &accounts,
        registry.clone(),
        state.kiro_runtime.clone(),
    )
    .await;
    state
        .publish_runtime_from_providers(existing_providers)
        .await
        .unwrap();

    std::fs::remove_file(dir.path().join("antigravity-test.json")).unwrap();
    accounts.reload().await.unwrap();

    state.refresh_provider_runtime().await.unwrap();

    assert_eq!(registry.get_model_count("gemini-2.5-pro").await, 0);
    assert!(registry
        .available_clients_for_model("gemini-2.5-pro")
        .await
        .is_empty());
}

#[tokio::test]
async fn refresh_provider_runtime_does_not_register_partial_replacement_before_failure() {
    let dir = TempDir::new().unwrap();
    let antigravity_base_url = spawn_antigravity_models_server("gemini-2.5-flash").await;
    let zed_base_url = spawn_failing_zed_server().await;

    let antigravity_json = serde_json::json!({
        "type": "antigravity",
        "provider_key": "antigravity",
        "email": "test@example.com",
        "access_token": "ya29.test",
        "refresh_token": "1//test",
        "project_id": "test-project",
        "base_url": antigravity_base_url,
        "expired": "2030-01-01T00:00:00Z"
    });
    std::fs::write(
        dir.path().join("antigravity-test.json"),
        serde_json::to_string_pretty(&antigravity_json).unwrap(),
    )
    .unwrap();

    let zed_json = serde_json::json!({
        "type": "zed",
        "provider_key": "zed",
        "user_id": "zed-user",
        "credential_json": format!("Bearer {zed_base_url}")
    });
    std::fs::write(
        dir.path().join("zed-test.json"),
        serde_json::to_string_pretty(&zed_json).unwrap(),
    )
    .unwrap();

    let accounts = Arc::new(AccountManager::with_dir(dir.path()));
    accounts.reload().await.unwrap();
    let registry = Arc::new(ModelRegistry::new());
    let state = ProxyState::new(Config::default(), accounts, registry.clone(), 0);

    let error = state
        .refresh_provider_runtime()
        .await
        .expect_err("later provider failure should abort refresh");

    match error {
        AppError::ProviderOperation {
            op,
            provider,
            source,
        } => {
            assert_eq!(op, "list_models");
            assert_eq!(provider, "zed");
            assert!(!source.to_string().is_empty());
        }
        other => panic!("expected provider operation error, got {other}"),
    }

    assert_eq!(registry.get_model_count("gemini-2.5-flash").await, 0);
    assert!(!registry.has_client("antigravity_0").await);
    assert!(state.providers.read().await.is_empty());
}

#[tokio::test]
async fn rebuild_runtime_snapshot_does_not_publish_before_registry_sync() {
    let dir = TempDir::new().unwrap();
    let antigravity_base_url = spawn_antigravity_models_server("gemini-2.5-flash").await;

    let antigravity_json = serde_json::json!({
        "type": "antigravity",
        "provider_key": "antigravity",
        "email": "test@example.com",
        "access_token": "ya29.test",
        "refresh_token": "1//test",
        "project_id": "test-project",
        "base_url": antigravity_base_url,
        "expired": "2030-01-01T00:00:00Z"
    });
    std::fs::write(
        dir.path().join("antigravity-test.json"),
        serde_json::to_string_pretty(&antigravity_json).unwrap(),
    )
    .unwrap();

    let accounts = Arc::new(AccountManager::with_dir(dir.path()));
    accounts.reload().await.unwrap();
    let registry = Arc::new(ModelRegistry::new());
    let state = ProxyState::new(Config::default(), accounts, registry.clone(), 0);

    let (providers, replacement_models) = state
        .rebuild_runtime_snapshot()
        .await
        .expect("rebuild should succeed");
    let rebuilt_client_id = replacement_models
        .keys()
        .next()
        .cloned()
        .expect("rebuild should return one client");

    assert_eq!(providers.len(), 1);
    assert_eq!(replacement_models.len(), 1);
    assert!(state.providers.read().await.is_empty());
    assert!(!registry.has_client(&rebuilt_client_id).await);
}

#[tokio::test]
async fn refresh_provider_runtime_clears_stale_execution_session_selection_when_provider_ids_change(
) {
    let dir = TempDir::new().unwrap();
    let first_auth = serde_json::json!({
        "type": "codex",
        "provider_key": "codex",
        "access_token": "access-1",
        "refresh_token": "refresh-1",
        "id_token": "id-1",
        "account_id": "acct-1",
        "email": "first@example.com",
        "expired": "2030-01-01T00:00:00Z",
        "last_refresh": "2026-03-18T00:00:00Z"
    });
    std::fs::write(
        dir.path().join("codex-first.json"),
        serde_json::to_string_pretty(&first_auth).unwrap(),
    )
    .unwrap();

    let accounts = Arc::new(AccountManager::with_dir(dir.path()));
    accounts.reload().await.unwrap();
    let registry = Arc::new(ModelRegistry::new());
    let state = ProxyState::new(Config::default(), accounts.clone(), registry.clone(), 0);

    state
        .execution_sessions
        .set_selected_auth(
            "session-stale".to_string(),
            "codex-missing.json".to_string(),
            true,
        )
        .await;

    state.refresh_provider_runtime().await.unwrap();

    assert_eq!(
        state
            .execution_sessions
            .get_selected_auth("session-stale")
            .await,
        None
    );
    assert!(registry.has_client("codex-first.json").await);
}

#[tokio::test]
async fn concurrent_refresh_provider_runtime_does_not_leave_registry_ahead_of_providers() {
    let dir = TempDir::new().unwrap();
    let first_request_started = Arc::new(Notify::new());
    let release_first_request = Arc::new(Notify::new());
    let first_base_url = spawn_blocked_antigravity_models_server(
        "gemini-2.5-flash",
        first_request_started.clone(),
        release_first_request.clone(),
    )
    .await;
    let second_base_url = spawn_antigravity_models_server("gemini-2.5-pro-preview").await;
    let first_path = dir.path().join("antigravity-first.json");
    let second_path = dir.path().join("antigravity-second.json");

    let first_auth = serde_json::json!({
        "type": "antigravity",
        "provider_key": "antigravity",
        "email": "first@example.com",
        "access_token": "ya29.first",
        "refresh_token": "1//first",
        "project_id": "first-project",
        "base_url": first_base_url,
        "expired": "2030-01-01T00:00:00Z"
    });
    std::fs::write(
        &first_path,
        serde_json::to_string_pretty(&first_auth).unwrap(),
    )
    .unwrap();

    let accounts = Arc::new(AccountManager::with_dir(dir.path()));
    accounts.reload().await.unwrap();
    let registry = Arc::new(ModelRegistry::new());
    let state = Arc::new(ProxyState::new(
        Config::default(),
        accounts.clone(),
        registry.clone(),
        0,
    ));

    let first_refresh = {
        let state = state.clone();
        tokio::spawn(async move { state.refresh_provider_runtime().await })
    };

    first_request_started.notified().await;

    std::fs::remove_file(&first_path).unwrap();

    let second_auth = serde_json::json!({
        "type": "antigravity",
        "provider_key": "antigravity",
        "email": "second@example.com",
        "access_token": "ya29.second",
        "refresh_token": "1//second",
        "project_id": "second-project",
        "base_url": second_base_url,
        "expired": "2030-01-01T00:00:00Z"
    });
    std::fs::write(
        &second_path,
        serde_json::to_string_pretty(&second_auth).unwrap(),
    )
    .unwrap();
    accounts.reload().await.unwrap();

    let second_refresh = {
        let state = state.clone();
        tokio::spawn(async move { state.refresh_provider_runtime().await })
    };

    release_first_request.notify_one();
    first_refresh.await.unwrap().unwrap();
    second_refresh.await.unwrap().unwrap();

    let provider_ids: Vec<String> = state
        .providers
        .read()
        .await
        .iter()
        .map(|provider| provider.client_id().to_string())
        .collect();

    for client_id in &provider_ids {
        assert!(registry.has_client(client_id).await);
    }
    assert!(
        provider_ids
            .iter()
            .any(|client_id| client_id == "antigravity-second.json"),
        "final providers should come from the latest refresh"
    );
    assert!(
        !provider_ids
            .iter()
            .any(|client_id| client_id == "antigravity-first.json"),
        "stale providers should be replaced by the later refresh"
    );
    assert!(
        registry.has_client("antigravity-second.json").await,
        "registry should stay in sync with the published providers after concurrent refresh"
    );
    assert!(
        !registry.has_client("antigravity-first.json").await,
        "stale registry entries should be removed by the later refresh"
    );
    assert_eq!(
        registry
            .available_clients_for_model("gemini-2.5-pro-preview")
            .await,
        vec!["antigravity-second.json".to_string()]
    );
}

#[tokio::test]
async fn register_and_list_models() {
    let reg = ModelRegistry::new();
    let models = vec![
        make_model("gpt-4", "openai"),
        make_model("gpt-3.5", "openai"),
    ];

    reg.register_client("client_0", "openai", models).await;

    let available = reg.get_available_models("openai").await;
    let ids: Vec<&str> = available.iter().filter_map(|v| v["id"].as_str()).collect();
    assert!(ids.contains(&"gpt-4"));
    assert!(ids.contains(&"gpt-3.5"));
}

#[tokio::test]
async fn get_model_providers_multi() {
    let reg = ModelRegistry::new();
    reg.register_client(
        "c1",
        "antigravity",
        vec![make_model("gemini-2.5-pro", "antigravity")],
    )
    .await;
    reg.register_client("c2", "gemini", vec![make_model("gemini-2.5-pro", "gemini")])
        .await;

    let providers = reg.get_model_providers("gemini-2.5-pro").await;
    assert!(providers.contains(&"antigravity".to_string()));
    assert!(providers.contains(&"gemini".to_string()));
}

#[tokio::test]
async fn unregister_removes_models() {
    let reg = ModelRegistry::new();
    reg.register_client("c1", "openai", vec![make_model("gpt-4", "openai")])
        .await;

    assert_eq!(reg.get_model_count("gpt-4").await, 1);

    reg.unregister_client("c1").await;

    assert_eq!(reg.get_model_count("gpt-4").await, 0);
    assert!(reg.get_model_providers("gpt-4").await.is_empty());
}

#[tokio::test]
async fn ref_counting_multiple_clients() {
    let reg = ModelRegistry::new();
    reg.register_client("c1", "openai", vec![make_model("gpt-4", "openai")])
        .await;
    reg.register_client("c2", "openai", vec![make_model("gpt-4", "openai")])
        .await;

    assert_eq!(reg.get_model_count("gpt-4").await, 2);

    reg.unregister_client("c1").await;
    assert_eq!(reg.get_model_count("gpt-4").await, 1);

    reg.unregister_client("c2").await;
    assert_eq!(reg.get_model_count("gpt-4").await, 0);
}

#[tokio::test]
async fn quota_exceeded_set_and_clear() {
    let reg = ModelRegistry::new();
    reg.register_client("c1", "openai", vec![make_model("gpt-4", "openai")])
        .await;

    // Model is registered
    assert!(reg.client_supports_model("c1", "gpt-4").await);

    // Set quota exceeded — model still registered, but quota tracked
    reg.set_quota_exceeded("c1", "gpt-4").await;
    // client_supports_model only checks registration, not quota
    assert!(reg.client_supports_model("c1", "gpt-4").await);

    // Clear quota
    reg.clear_quota_exceeded("c1", "gpt-4").await;
    assert!(reg.client_supports_model("c1", "gpt-4").await);
}

#[tokio::test]
async fn suspend_and_resume() {
    let reg = ModelRegistry::new();
    reg.register_client("c1", "openai", vec![make_model("gpt-4", "openai")])
        .await;

    // Suspend — model still registered
    reg.suspend_client_model("c1", "gpt-4", "testing").await;
    assert!(reg.client_supports_model("c1", "gpt-4").await);

    // Resume
    reg.resume_client_model("c1", "gpt-4").await;
    assert!(reg.client_supports_model("c1", "gpt-4").await);
}

#[tokio::test]
async fn empty_registry_returns_empty() {
    let reg = ModelRegistry::new();
    assert!(reg.get_available_models("openai").await.is_empty());
    assert!(reg.get_model_providers("nonexistent").await.is_empty());
    assert_eq!(reg.get_model_count("nonexistent").await, 0);
    assert!(!reg.client_supports_model("x", "y").await);
}

#[tokio::test]
async fn register_empty_models_unregisters() {
    let reg = ModelRegistry::new();
    reg.register_client("c1", "openai", vec![make_model("gpt-4", "openai")])
        .await;
    assert_eq!(reg.get_model_count("gpt-4").await, 1);

    // Re-register with empty models → should unregister
    reg.register_client("c1", "openai", vec![]).await;
    assert_eq!(reg.get_model_count("gpt-4").await, 0);
}

#[tokio::test]
async fn reconcile_updates_models() {
    let reg = ModelRegistry::new();
    reg.register_client(
        "c1",
        "openai",
        vec![
            make_model("gpt-4", "openai"),
            make_model("gpt-3.5", "openai"),
        ],
    )
    .await;

    assert_eq!(reg.get_model_count("gpt-4").await, 1);
    assert_eq!(reg.get_model_count("gpt-3.5").await, 1);

    // Re-register: remove gpt-3.5, add gpt-4o
    reg.register_client(
        "c1",
        "openai",
        vec![
            make_model("gpt-4", "openai"),
            make_model("gpt-4o", "openai"),
        ],
    )
    .await;

    assert_eq!(reg.get_model_count("gpt-4").await, 1);
    assert_eq!(reg.get_model_count("gpt-4o").await, 1);
    assert_eq!(reg.get_model_count("gpt-3.5").await, 0);
}

#[tokio::test]
async fn available_clients_excludes_quota_exceeded() {
    let reg = ModelRegistry::new();
    reg.register_client("c1", "kiro", vec![make_model("model-a", "kiro")])
        .await;
    reg.register_client("c2", "kiro", vec![make_model("model-a", "kiro")])
        .await;

    reg.set_quota_exceeded("c1", "model-a").await;

    let available = reg.available_clients_for_model("model-a").await;
    assert!(available.contains(&"c2".to_string()));
    assert!(!available.contains(&"c1".to_string()));
}

#[tokio::test]
async fn available_clients_excludes_suspended() {
    let reg = ModelRegistry::new();
    reg.register_client("c1", "kiro", vec![make_model("model-a", "kiro")])
        .await;
    reg.register_client("c2", "kiro", vec![make_model("model-a", "kiro")])
        .await;

    reg.suspend_client_model("c1", "model-a", "auth error")
        .await;

    let available = reg.available_clients_for_model("model-a").await;
    assert!(available.contains(&"c2".to_string()));
    assert!(!available.contains(&"c1".to_string()));
}

#[tokio::test]
async fn client_is_effectively_available_basic() {
    let reg = ModelRegistry::new();
    reg.register_client("c1", "kiro", vec![make_model("model-a", "kiro")])
        .await;

    assert!(reg.client_is_effectively_available("c1", "model-a").await);

    // Unregistered client
    assert!(!reg.client_is_effectively_available("c99", "model-a").await);

    // Suspend and check
    reg.suspend_client_model("c1", "model-a", "test").await;
    assert!(!reg.client_is_effectively_available("c1", "model-a").await);

    // Resume and check
    reg.resume_client_model("c1", "model-a").await;
    assert!(reg.client_is_effectively_available("c1", "model-a").await);
}

#[tokio::test]
async fn client_is_effectively_available_quota_exceeded() {
    let reg = ModelRegistry::new();
    reg.register_client("c1", "kiro", vec![make_model("model-a", "kiro")])
        .await;

    reg.set_quota_exceeded("c1", "model-a").await;
    assert!(!reg.client_is_effectively_available("c1", "model-a").await);

    reg.clear_quota_exceeded("c1", "model-a").await;
    assert!(reg.client_is_effectively_available("c1", "model-a").await);
}

#[tokio::test]
async fn available_clients_with_one_suspended_other_remains() {
    let reg = ModelRegistry::new();
    reg.register_client("c1", "kiro", vec![make_model("model-a", "kiro")])
        .await;
    reg.register_client("c2", "kiro", vec![make_model("model-a", "kiro")])
        .await;
    reg.register_client("c3", "kiro", vec![make_model("model-a", "kiro")])
        .await;

    reg.suspend_client_model("c1", "model-a", "auth error")
        .await;
    reg.set_quota_exceeded("c2", "model-a").await;

    let available = reg.available_clients_for_model("model-a").await;
    assert_eq!(available.len(), 1);
    assert!(available.contains(&"c3".to_string()));
}

#[tokio::test]
async fn available_clients_empty_when_all_unavailable() {
    let reg = ModelRegistry::new();
    reg.register_client("c1", "kiro", vec![make_model("model-a", "kiro")])
        .await;

    reg.suspend_client_model("c1", "model-a", "broken").await;

    let available = reg.available_clients_for_model("model-a").await;
    assert!(available.is_empty());
}

#[tokio::test]
async fn available_clients_for_nonexistent_model() {
    let reg = ModelRegistry::new();
    let available = reg.available_clients_for_model("nonexistent").await;
    assert!(available.is_empty());
}

#[tokio::test]
async fn quota_expired_readmits_client() {
    use std::time::{Duration, Instant};

    let reg = ModelRegistry::new();
    reg.register_client("c1", "kiro", vec![make_model("model-a", "kiro")])
        .await;

    // Set quota exceeded at a time far in the past (beyond 5-min window)
    let past = Instant::now() - Duration::from_secs(600);
    reg.set_quota_exceeded_at("c1", "model-a", past).await;

    // Should be available because the quota exceeded entry has expired
    assert!(reg.client_is_effectively_available("c1", "model-a").await);
    let available = reg.available_clients_for_model("model-a").await;
    assert!(available.contains(&"c1".to_string()));
}
