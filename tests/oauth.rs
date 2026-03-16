//! Tests for OAuth session tracker and web OAuth endpoints.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tempfile::TempDir;
use tower::ServiceExt;

use rusuh::auth::manager::AccountManager;
use rusuh::config::{Config, ManagementConfig};
use rusuh::providers::model_registry::ModelRegistry;
use rusuh::proxy::oauth::OAuthSessionStore;
use rusuh::proxy::ProxyState;
use rusuh::router::build_router;

const SECRET: &str = "test-oauth-secret";

fn test_app(cfg: Config) -> axum::Router {
    let auth_dir = cfg.auth_dir.clone();
    let accounts = Arc::new(AccountManager::with_dir(auth_dir));
    let registry = Arc::new(ModelRegistry::new());
    let state = Arc::new(ProxyState::new(cfg, accounts, registry, 0));

    build_router(state.clone()).layer(axum::middleware::from_fn_with_state(
        state,
        rusuh::middleware::auth::api_key_auth,
    ))
}

fn mgmt_config(auth_dir: &str) -> Config {
    let mut cfg = Config::default();
    cfg.auth_dir = auth_dir.into();
    cfg.remote_management = ManagementConfig {
        allow_remote: true,
        secret_key: SECRET.into(),
    };
    cfg
}

fn mgmt_request(uri: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header("authorization", format!("Bearer {SECRET}"))
        .body(Body::empty())
        .unwrap()
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(json!(null))
}

// ── OAuthSessionStore unit tests ─────────────────────────────────────────────

#[tokio::test]
async fn session_store_register_and_get() {
    let store = OAuthSessionStore::new();
    store.register("state-1", "antigravity").await;

    let (provider, _status) = store.get_status("state-1").await.unwrap();
    assert_eq!(provider, "antigravity");
}

#[tokio::test]
async fn session_store_complete() {
    let store = OAuthSessionStore::new();
    store.register("s2", "antigravity").await;
    store.complete("s2").await;

    let (_, status) = store.get_status("s2").await.unwrap();
    assert!(matches!(
        status,
        rusuh::proxy::oauth::OAuthSessionStatus::Complete
    ));
}

#[tokio::test]
async fn session_store_error() {
    let store = OAuthSessionStore::new();
    store.register("s3", "antigravity").await;
    store.set_error("s3", "token exchange failed").await;

    let (_, status) = store.get_status("s3").await.unwrap();
    match status {
        rusuh::proxy::oauth::OAuthSessionStatus::Error(msg) => {
            assert_eq!(msg, "token exchange failed");
        }
        _ => panic!("expected Error status"),
    }
}

#[tokio::test]
async fn session_store_unknown_returns_none() {
    let store = OAuthSessionStore::new();
    assert!(store.get_status("nonexistent").await.is_none());
}

#[tokio::test]
async fn session_store_cleanup_removes_old() {
    let store = OAuthSessionStore::new();
    store.register("old", "test").await;

    // Cleanup with 0s max age — everything is "old"
    store.cleanup(0).await;
    assert!(store.get_status("old").await.is_none());
}

// ── Integration: antigravity-auth-url endpoint ───────────────────────────────

#[tokio::test]
async fn antigravity_auth_url_returns_url_and_state() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(mgmt_request("/v0/management/antigravity-auth-url"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "ok");
    assert!(body["url"]
        .as_str()
        .unwrap()
        .contains("accounts.google.com"));
    assert!(body["url"].as_str().unwrap().contains("redirect_uri="));
    assert!(!body["state"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn antigravity_auth_url_requires_auth() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    // No auth header
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v0/management/antigravity-auth-url")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── Integration: auth-status endpoint ────────────────────────────────────────

#[tokio::test]
async fn auth_status_no_state_returns_ok() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(mgmt_request("/v0/management/auth-status"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn auth_status_unknown_state_returns_ok() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(mgmt_request("/v0/management/auth-status?state=nonexistent"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "ok");
}

// ── Integration: callback rejects unknown state ──────────────────────────────

#[tokio::test]
async fn callback_rejects_unknown_state() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/antigravity/callback?code=fake&state=unknown")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK); // HTML response
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8_lossy(&bytes);
    assert!(html.contains("Unknown or expired"));
}
