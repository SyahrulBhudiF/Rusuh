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

