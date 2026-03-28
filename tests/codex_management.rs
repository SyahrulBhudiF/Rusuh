//! Integration tests for Codex management quota endpoint.

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::State;
use axum::http::{header::AUTHORIZATION, HeaderMap, Request, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::sync::Mutex;
use tower::ServiceExt;

use rusuh::auth::manager::AccountManager;
use rusuh::config::{Config, ManagementConfig};
use rusuh::providers::model_registry::ModelRegistry;
use rusuh::proxy::ProxyState;
use rusuh::router::build_router;

const SECRET: &str = "test-mgmt-secret";

#[derive(Clone)]
struct MockCodexQuotaState {
    status: StatusCode,
    body: Value,
    last_authorization: Arc<Mutex<Option<String>>>,
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

async fn test_app(cfg: Config) -> axum::Router {
    let auth_dir = cfg.auth_dir.clone();
    let accounts = Arc::new(AccountManager::with_dir(auth_dir));
    accounts.reload().await.unwrap();
    let registry = Arc::new(ModelRegistry::new());
    let state = Arc::new(ProxyState::new(cfg, accounts, registry, 0));

    build_router(state.clone()).layer(axum::middleware::from_fn_with_state(
        state,
        rusuh::middleware::auth::api_key_auth,
    ))
}

fn mgmt_request_json(method: &str, uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {SECRET}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(json!(null))
}

async fn mock_models_handler(
    State(state): State<MockCodexQuotaState>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    let authorization = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    *state.last_authorization.lock().await = authorization;
    (state.status, Json(state.body))
}

async fn spawn_codex_quota_server(
    status: StatusCode,
    body: Value,
) -> (String, Arc<Mutex<Option<String>>>) {
    let last_authorization = Arc::new(Mutex::new(None));
    let state = MockCodexQuotaState {
        status,
        body,
        last_authorization: last_authorization.clone(),
    };

    let app = Router::new()
        .route("/v1/models", get(mock_models_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    (format!("http://{addr}/v1"), last_authorization)
}

async fn spawn_slow_codex_quota_server(delay: Duration) -> String {
    let app = Router::new().route(
        "/v1/models",
        get(move || async move {
            tokio::time::sleep(delay).await;
            (StatusCode::OK, Json(json!({"data": []})))
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    format!("http://{addr}/v1")
}

fn write_codex_auth(
    dir: &TempDir,
    file_name: &str,
    account_id: &str,
    base_url: &str,
    plan_type: Option<&str>,
) {
    let mut auth = json!({
        "type": "codex",
        "provider_key": "codex",
        "id_token": "fake_id_token",
        "access_token": "fake_access_token",
        "refresh_token": "fake_refresh_token",
        "account_id": account_id,
        "email": format!("{account_id}@example.com"),
        "expired": "2030-01-01T00:00:00Z",
        "base_url": base_url,
    });

    if let Some(plan_type) = plan_type {
        auth["plan_type"] = json!(plan_type);
    }

    std::fs::write(
        dir.path().join(file_name),
        serde_json::to_string_pretty(&auth).unwrap(),
    )
    .unwrap();
}

#[tokio::test]
async fn check_codex_quota_missing_account_returns_404() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap())).await;

    let resp = app
        .oneshot(mgmt_request_json(
            "POST",
            "/v0/management/codex/check-quota",
            json!({"name": "nonexistent.json"}),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert!(body["error"].as_str().unwrap().contains("not found"));
}

#[tokio::test]
async fn check_codex_quota_returns_available_for_200_probe() {
    let dir = TempDir::new().unwrap();
    let (base_url, last_authorization) =
        spawn_codex_quota_server(StatusCode::OK, json!({"data": []})).await;
    write_codex_auth(
        &dir,
        "codex-test.json",
        "test-account",
        &base_url,
        Some("plus"),
    );

    let app = test_app(mgmt_config(dir.path().to_str().unwrap())).await;

    let resp = app
        .oneshot(mgmt_request_json(
            "POST",
            "/v0/management/codex/check-quota",
            json!({"name": "codex-test.json"}),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;

    assert_eq!(body["account"], "test-account");
    assert_eq!(body["status"], "available");
    assert_eq!(body["upstream_status"], 200);
    assert_eq!(body["plan_type"], "plus");
    assert_eq!(
        *last_authorization.lock().await,
        Some("Bearer fake_access_token".to_string())
    );
}

#[tokio::test]
async fn check_codex_quota_times_out_slow_probe() {
    let dir = TempDir::new().unwrap();
    let base_url = spawn_slow_codex_quota_server(Duration::from_secs(30)).await;
    write_codex_auth(
        &dir,
        "codex-slow.json",
        "slow-account",
        &base_url,
        Some("plus"),
    );

    let app = test_app(mgmt_config(dir.path().to_str().unwrap())).await;

    let response = tokio::time::timeout(
        Duration::from_secs(8),
        app.oneshot(mgmt_request_json(
            "POST",
            "/v0/management/codex/check-quota",
            json!({"name": "codex-slow.json"}),
        )),
    )
    .await;

    let resp = response.expect("quota probe should be bounded").unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_json(resp).await;
    assert_eq!(body["account"], "slow-account");
    assert_eq!(body["status"], "error");
    assert_eq!(body["upstream_status"], 0);
    assert_eq!(body["plan_type"], "plus");
    let detail = body["detail"].as_str().unwrap().to_ascii_lowercase();
    assert!(detail.contains("request failed:"));
}

#[tokio::test]
async fn check_codex_quota_returns_exhausted_for_usage_limit_reached() {
    let dir = TempDir::new().unwrap();
    let (base_url, _) = spawn_codex_quota_server(
        StatusCode::TOO_MANY_REQUESTS,
        json!({
            "error": {
                "type": "usage_limit_reached",
                "message": "Usage limit reached.",
                "resets_in_seconds": 90
            }
        }),
    )
    .await;
    write_codex_auth(
        &dir,
        "codex-exhausted.json",
        "exhausted-account",
        &base_url,
        Some("pro"),
    );

    let app = test_app(mgmt_config(dir.path().to_str().unwrap())).await;

    let resp = app
        .oneshot(mgmt_request_json(
            "POST",
            "/v0/management/codex/check-quota",
            json!({"name": "codex-exhausted.json"}),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;

    assert_eq!(body["account"], "exhausted-account");
    assert_eq!(body["status"], "exhausted");
    assert_eq!(body["upstream_status"], 429);
    assert_eq!(body["retry_after_seconds"], 90);
    assert_eq!(body["detail"], "Usage limit reached.");
    assert_eq!(body["plan_type"], "pro");
}
