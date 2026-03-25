//! Tests for OAuth session tracker and web OAuth endpoints.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tempfile::TempDir;
use tower::ServiceExt;

use rusuh::auth::kiro_login::{CreateTokenResponse, RegisterClientResponse};
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
    Config {
        auth_dir: auth_dir.into(),
        remote_management: ManagementConfig {
            allow_remote: true,
            secret_key: SECRET.into(),
        },
        ..Default::default()
    }
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

// ── Integration: oauth/start endpoint ─────────────────────────────────────────
#[tokio::test]
async fn antigravity_auth_url_returns_url_and_state() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));
    let resp = app
        .oneshot(mgmt_request(
            "/v0/management/oauth/start?provider=antigravity",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "ok");
    assert_eq!(body["provider"], "antigravity");
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
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v0/management/oauth/start?provider=antigravity")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── Integration: oauth/status endpoint ───────────────────────────────────────

#[tokio::test]
async fn auth_status_no_state_returns_ok() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(mgmt_request("/v0/management/oauth/status"))
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
        .oneshot(mgmt_request(
            "/v0/management/oauth/status?state=nonexistent",
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn legacy_kiro_social_start_rejected() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(mgmt_request(
            "/v0/management/oauth/start?provider=kiro-google",
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "error");
    assert!(body["error"]
        .as_str()
        .unwrap_or_default()
        .contains("Kiro social login is unsupported"));
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

#[tokio::test]
async fn builder_id_callback_route_rejects_unknown_state() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/kiro/builder-id/callback?code=fake&state=unknown")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8_lossy(&bytes);
    assert!(html.contains("Unknown or expired OAuth session"));
}

#[tokio::test]
async fn oauth_status_reports_kiro_pending_session() {
    let dir = TempDir::new().unwrap();
    let cfg = mgmt_config(dir.path().to_str().unwrap());
    let auth_dir = cfg.auth_dir.clone();
    let accounts = Arc::new(AccountManager::with_dir(auth_dir));
    let registry = Arc::new(ModelRegistry::new());
    let state = Arc::new(ProxyState::new(cfg, accounts, registry, 0));
    state.oauth_sessions.register("kiro-pending", "kiro").await;

    let app = build_router(state.clone()).layer(axum::middleware::from_fn_with_state(
        state,
        rusuh::middleware::auth::api_key_auth,
    ));
    let status_resp = app
        .oneshot(mgmt_request(
            "/v0/management/oauth/status?state=kiro-pending",
        ))
        .await
        .unwrap();
    assert_eq!(status_resp.status(), StatusCode::OK);
    let status_body = body_json(status_resp).await;
    assert_eq!(status_body["status"], "wait");
    assert_eq!(status_body["provider"], "kiro");
}

#[tokio::test]
async fn builder_id_callback_rejects_missing_state() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/kiro/builder-id/callback?code=fake")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8_lossy(&bytes);
    assert!(html.contains("Missing state parameter"));
}

#[tokio::test]
async fn builder_id_callback_marks_error_when_provider_returns_error() {
    let dir = TempDir::new().unwrap();
    let cfg = mgmt_config(dir.path().to_str().unwrap());
    let auth_dir = cfg.auth_dir.clone();
    let accounts = Arc::new(AccountManager::with_dir(auth_dir));
    let registry = Arc::new(ModelRegistry::new());
    let state = Arc::new(ProxyState::new(cfg, accounts, registry, 0));
    state.oauth_sessions.register("kiro-err", "kiro").await;

    let app = build_router(state.clone()).layer(axum::middleware::from_fn_with_state(
        state.clone(),
        rusuh::middleware::auth::api_key_auth,
    ));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/kiro/builder-id/callback?state=kiro-err&error=access_denied")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8_lossy(&bytes);
    assert!(html.contains("Authentication failed"));
    assert!(html.contains("access_denied"));

    let status = state.oauth_sessions.get_status("kiro-err").await;
    match status {
        Some((provider, rusuh::proxy::oauth::OAuthSessionStatus::Error(message))) => {
            assert_eq!(provider, "kiro");
            assert_eq!(message, "access_denied");
        }
        other => panic!("expected kiro error session, got {other:?}"),
    }
}

#[tokio::test]
async fn builder_id_callback_marks_error_when_code_missing() {
    let dir = TempDir::new().unwrap();
    let cfg = mgmt_config(dir.path().to_str().unwrap());
    let auth_dir = cfg.auth_dir.clone();
    let accounts = Arc::new(AccountManager::with_dir(auth_dir));
    let registry = Arc::new(ModelRegistry::new());
    let state = Arc::new(ProxyState::new(cfg, accounts, registry, 0));
    state
        .oauth_sessions
        .register("kiro-missing-code", "kiro")
        .await;

    let app = build_router(state.clone()).layer(axum::middleware::from_fn_with_state(
        state.clone(),
        rusuh::middleware::auth::api_key_auth,
    ));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/kiro/builder-id/callback?state=kiro-missing-code")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8_lossy(&bytes);
    assert!(html.contains("Missing authorization code"));

    let status = state.oauth_sessions.get_status("kiro-missing-code").await;
    match status {
        Some((provider, rusuh::proxy::oauth::OAuthSessionStatus::Error(message))) => {
            assert_eq!(provider, "kiro");
            assert!(message.contains("missing authorization code"));
        }
        other => panic!("expected kiro error session, got {other:?}"),
    }
}

#[test]
fn builder_id_redirect_uri_matches_mounted_callback_route() {
    let redirect_uri = rusuh::proxy::oauth::builder_id_redirect_uri(8317);
    assert_eq!(
        redirect_uri,
        format!(
            "http://localhost:8317{}",
            rusuh::proxy::oauth::BUILDER_ID_CALLBACK_PATH
        )
    );
}

#[test]
fn builder_id_session_context_contains_persistence_critical_fields() {
    let registration = RegisterClientResponse {
        client_id: "client-id".into(),
        client_secret: "client-secret".into(),
        client_id_issued_at: 1,
        client_secret_expires_at: 2,
    };

    let context = rusuh::proxy::oauth::build_builder_id_session_context(
        &registration,
        "http://localhost:8317/kiro/builder-id/callback",
        Some("  Main Kiro  ".into()),
    );

    assert_eq!(
        context.get("client_id").and_then(Value::as_str),
        Some("client-id")
    );
    assert_eq!(
        context.get("client_secret").and_then(Value::as_str),
        Some("client-secret")
    );
    assert_eq!(
        context.get("redirect_uri").and_then(Value::as_str),
        Some("http://localhost:8317/kiro/builder-id/callback")
    );
    assert_eq!(
        context.get("auth_method").and_then(Value::as_str),
        Some("builder-id")
    );
    assert_eq!(context.get("provider").and_then(Value::as_str), Some("AWS"));
    assert_eq!(
        context.get("region").and_then(Value::as_str),
        Some("us-east-1")
    );
    assert_eq!(
        context.get("start_url").and_then(Value::as_str),
        Some("https://view.awsapps.com/start")
    );
    assert_eq!(
        context.get("label").and_then(Value::as_str),
        Some("Main Kiro")
    );
}

#[test]
fn builder_id_auth_record_uses_callback_context_and_token_response() {
    let registration = RegisterClientResponse {
        client_id: "client-id".into(),
        client_secret: "client-secret".into(),
        client_id_issued_at: 1,
        client_secret_expires_at: 2,
    };
    let context = rusuh::proxy::oauth::build_builder_id_session_context(
        &registration,
        "http://localhost:8317/kiro/builder-id/callback",
        Some("Main Kiro".into()),
    );
    let token_resp = CreateTokenResponse {
        access_token: "access-token".into(),
        token_type: "Bearer".into(),
        expires_in: 3600,
        refresh_token: Some("refresh-token".into()),
    };

    let record = rusuh::proxy::oauth::build_builder_id_auth_record(
        &context,
        token_resp,
        Some("user@example.com".into()),
    )
    .unwrap();

    assert_eq!(record.provider, "kiro");
    assert_eq!(record.provider_key, "kiro");
    assert_eq!(record.label, "Main Kiro");
    assert_eq!(
        record.metadata.get("auth_method").and_then(Value::as_str),
        Some("builder-id")
    );
    assert_eq!(
        record.metadata.get("provider").and_then(Value::as_str),
        Some("AWS")
    );
    assert_eq!(
        record.metadata.get("region").and_then(Value::as_str),
        Some("us-east-1")
    );
    assert_eq!(
        record.metadata.get("start_url").and_then(Value::as_str),
        Some("https://view.awsapps.com/start")
    );
    assert_eq!(
        record.metadata.get("client_id").and_then(Value::as_str),
        Some("client-id")
    );
    assert_eq!(
        record.metadata.get("client_secret").and_then(Value::as_str),
        Some("client-secret")
    );
    assert_eq!(
        record.metadata.get("access_token").and_then(Value::as_str),
        Some("access-token")
    );
    assert_eq!(
        record.metadata.get("refresh_token").and_then(Value::as_str),
        Some("refresh-token")
    );
    assert_eq!(
        record.metadata.get("email").and_then(Value::as_str),
        Some("user@example.com")
    );
}

#[test]
fn builder_id_auth_record_rejects_empty_access_token() {
    let registration = RegisterClientResponse {
        client_id: "client-id".into(),
        client_secret: "client-secret".into(),
        client_id_issued_at: 1,
        client_secret_expires_at: 2,
    };
    let context = rusuh::proxy::oauth::build_builder_id_session_context(
        &registration,
        "http://localhost:8317/kiro/builder-id/callback",
        None,
    );
    let token_resp = CreateTokenResponse {
        access_token: "   ".into(),
        token_type: "Bearer".into(),
        expires_in: 3600,
        refresh_token: Some("refresh-token".into()),
    };

    let error =
        rusuh::proxy::oauth::build_builder_id_auth_record(&context, token_resp, None).unwrap_err();
    assert!(error.to_string().contains("empty access token"));
}
