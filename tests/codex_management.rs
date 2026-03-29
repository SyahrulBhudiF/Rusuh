//! Integration tests for Codex management quota endpoint.

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::State;
use axum::http::{
    header::{AUTHORIZATION, CONTENT_TYPE},
    Request, StatusCode,
};
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
    body: String,
    content_type: &'static str,
    last_authorization: Arc<Mutex<Option<String>>>,
    last_chatgpt_account_id: Arc<Mutex<Option<String>>>,
    last_originator: Arc<Mutex<Option<String>>>,
    last_session_id: Arc<Mutex<Option<String>>>,
    last_user_agent: Arc<Mutex<Option<String>>>,
    last_version: Arc<Mutex<Option<String>>>,
    last_accept: Arc<Mutex<Option<String>>>,
    last_body: Arc<Mutex<Option<Value>>>,
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

async fn mock_responses_handler(
    State(state): State<MockCodexQuotaState>,
    request: Request<Body>,
) -> (StatusCode, [(axum::http::header::HeaderName, &'static str); 1], String) {
    let headers = request.headers();
    let authorization = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let chatgpt_account_id = headers
        .get("Chatgpt-Account-Id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let originator = headers
        .get("Originator")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let session_id = headers
        .get("Session_id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let version = headers
        .get("Version")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let accept = headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body_bytes = request.into_body().collect().await.unwrap().to_bytes();
    let body = if body_bytes.is_empty() {
        None
    } else {
        serde_json::from_slice(&body_bytes).ok()
    };

    *state.last_authorization.lock().await = authorization;
    *state.last_chatgpt_account_id.lock().await = chatgpt_account_id;
    *state.last_originator.lock().await = originator;
    *state.last_session_id.lock().await = session_id;
    *state.last_user_agent.lock().await = user_agent;
    *state.last_version.lock().await = version;
    *state.last_accept.lock().await = accept;
    *state.last_body.lock().await = body;

    (
        state.status,
        [(CONTENT_TYPE, state.content_type)],
        state.body,
    )
}

struct CodexQuotaProbeCapture {
    authorization: Arc<Mutex<Option<String>>>,
    chatgpt_account_id: Arc<Mutex<Option<String>>>,
    originator: Arc<Mutex<Option<String>>>,
    session_id: Arc<Mutex<Option<String>>>,
    user_agent: Arc<Mutex<Option<String>>>,
    version: Arc<Mutex<Option<String>>>,
    accept: Arc<Mutex<Option<String>>>,
    body: Arc<Mutex<Option<Value>>>,
}

async fn spawn_codex_quota_server(
    status: StatusCode,
    body: Value,
) -> (String, CodexQuotaProbeCapture) {
    spawn_codex_quota_server_with_body(status, body.to_string(), "application/json").await
}

async fn spawn_codex_quota_server_with_body(
    status: StatusCode,
    body: String,
    content_type: &'static str,
) -> (String, CodexQuotaProbeCapture) {
    let last_authorization = Arc::new(Mutex::new(None));
    let last_chatgpt_account_id = Arc::new(Mutex::new(None));
    let last_originator = Arc::new(Mutex::new(None));
    let last_session_id = Arc::new(Mutex::new(None));
    let last_user_agent = Arc::new(Mutex::new(None));
    let last_version = Arc::new(Mutex::new(None));
    let last_accept = Arc::new(Mutex::new(None));
    let last_body = Arc::new(Mutex::new(None));
    let state = MockCodexQuotaState {
        status,
        body,
        content_type,
        last_authorization: last_authorization.clone(),
        last_chatgpt_account_id: last_chatgpt_account_id.clone(),
        last_originator: last_originator.clone(),
        last_session_id: last_session_id.clone(),
        last_user_agent: last_user_agent.clone(),
        last_version: last_version.clone(),
        last_accept: last_accept.clone(),
        last_body: last_body.clone(),
    };

    let app = Router::new()
        .route("/wham/usage", get(mock_responses_handler).post(mock_responses_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    (
        format!("http://{addr}"),
        CodexQuotaProbeCapture {
            authorization: last_authorization,
            chatgpt_account_id: last_chatgpt_account_id,
            originator: last_originator,
            session_id: last_session_id,
            user_agent: last_user_agent,
            version: last_version,
            accept: last_accept,
            body: last_body,
        },
    )
}

async fn spawn_slow_codex_quota_server(delay: Duration) -> String {
    let app = Router::new().route(
        "/wham/usage",
        get(move || async move {
            tokio::time::sleep(delay).await;
            (StatusCode::OK, Json(json!({"account_id": "slow-account"})))
        })
        .post(move || async move {
            tokio::time::sleep(delay).await;
            (StatusCode::OK, Json(json!({"account_id": "slow-account"})))
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    format!("http://{addr}")
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
async fn check_codex_quota_returns_available_for_usage_window_response() {
    let dir = TempDir::new().unwrap();
    let (base_url, capture) = spawn_codex_quota_server(
        StatusCode::OK,
        json!({
            "user_id": "user_123",
            "account_id": "test-account",
            "email": "test-account@example.com",
            "plan_type": "team",
            "rate_limit": {
                "allowed": true,
                "limit_reached": false,
                "primary_window": {
                    "used_percent": 5,
                    "limit_window_seconds": 18000,
                    "reset_after_seconds": 15247,
                    "reset_at": 1774816656
                },
                "secondary_window": {
                    "used_percent": 9,
                    "limit_window_seconds": 604800,
                    "reset_after_seconds": 583764,
                    "reset_at": 1775385173
                }
            },
            "code_review_rate_limit": {
                "allowed": true,
                "limit_reached": false,
                "primary_window": {
                    "used_percent": 0,
                    "limit_window_seconds": 604800,
                    "reset_after_seconds": 604800,
                    "reset_at": 1775406209
                },
                "secondary_window": null
            },
            "additional_rate_limits": null,
            "credits": {
                "has_credits": false,
                "unlimited": false,
                "balance": null,
                "approx_local_messages": null,
                "approx_cloud_messages": null
            },
            "spend_control": {
                "reached": false
            },
            "promo": null
        }),
    )
    .await;
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
    assert_eq!(body["plan_type"], "team");
    assert_eq!(body["rate_limit"]["allowed"], true);
    assert_eq!(body["rate_limit"]["limit_reached"], false);
    assert_eq!(body["rate_limit"]["primary_window"]["used_percent"], 5);
    assert_eq!(body["rate_limit"]["primary_window"]["limit_window_seconds"], 18000);
    assert_eq!(body["rate_limit"]["primary_window"]["reset_after_seconds"], 15247);
    assert_eq!(body["rate_limit"]["primary_window"]["reset_at"], 1774816656);
    assert_eq!(body["rate_limit"]["secondary_window"]["used_percent"], 9);
    assert_eq!(body["code_review_rate_limit"]["primary_window"]["used_percent"], 0);
    assert_eq!(body["credits"]["has_credits"], false);
    assert_eq!(body["spend_control"]["reached"], false);
    assert_eq!(body["raw_response"]["account_id"], "test-account");
    assert_eq!(
        *capture.authorization.lock().await,
        Some("Bearer fake_access_token".to_string())
    );
    assert_eq!(
        *capture.chatgpt_account_id.lock().await,
        Some("test-account".to_string())
    );
    assert_eq!(
        *capture.originator.lock().await,
        Some("codex_cli_rs".to_string())
    );
    assert!(capture.session_id.lock().await.is_some());
    assert!(capture.user_agent.lock().await.is_some());
    assert!(capture.version.lock().await.is_some());
    assert_eq!(
        *capture.accept.lock().await,
        Some("application/json".to_string())
    );
    assert!(capture.body.lock().await.is_none());
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
        Duration::from_secs(20),
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

#[tokio::test]
async fn check_codex_quota_preserves_plain_text_upstream_error_body() {
    let dir = TempDir::new().unwrap();
    let (base_url, _) = spawn_codex_quota_server_with_body(
        StatusCode::BAD_REQUEST,
        "team workspace is not enabled for model listing".to_string(),
        "text/plain",
    )
    .await;
    write_codex_auth(
        &dir,
        "codex-plain-error.json",
        "team-account",
        &base_url,
        Some("team"),
    );

    let app = test_app(mgmt_config(dir.path().to_str().unwrap())).await;

    let resp = app
        .oneshot(mgmt_request_json(
            "POST",
            "/v0/management/codex/check-quota",
            json!({"name": "codex-plain-error.json"}),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;

    assert_eq!(body["account"], "team-account");
    assert_eq!(body["status"], "error");
    assert_eq!(body["upstream_status"], 400);
    assert_eq!(body["plan_type"], "team");
    assert_eq!(
        body["detail"],
        "team workspace is not enabled for model listing"
    );
}
