use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use rusuh::auth::manager::AccountManager;
use rusuh::config::Config;
use rusuh::providers::model_registry::ModelRegistry;
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
    let mut cfg = Config::default();
    cfg.api_keys = vec!["secret-key".into()];

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
    let mut cfg = Config::default();
    cfg.api_keys = vec!["correct-key".into()];

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
    let mut cfg = Config::default();
    cfg.api_keys = vec!["correct-key".into()];

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
    let mut cfg = Config::default();
    cfg.api_keys = vec!["my-key".into()];

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
    let mut cfg = Config::default();
    cfg.api_keys = vec!["my-key".into()];

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
async fn chat_completions_no_providers_returns_error() {
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

    // 429 — no providers available
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
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
async fn management_endpoint_skips_auth() {
    let mut cfg = Config::default();
    cfg.api_keys = vec!["secret".into()];

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
        registry.register_client(&client_id, provider.name(), models).await;
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
    state.providers = providers;
    let state = Arc::new(state);

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
        registry.register_client(&client_id, provider.name(), models).await;
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
    state.providers = providers;
    let state = Arc::new(state);

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
        registry.register_client(&client_id, provider.name(), models).await;
    }

    // Mark provider as quota-exceeded
    registry.set_quota_exceeded("kiro_0", test_model).await;

    // Verify no providers are available
    let available = registry.available_clients_for_model(test_model).await;
    assert_eq!(available.len(), 0);

    // Build app state
    let mut state = ProxyState::new(config, accounts, registry, providers.len());
    state.kiro_runtime = runtime;
    state.providers = providers;
    let state = Arc::new(state);

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

    // Should return 429 when all providers are unavailable
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
}
