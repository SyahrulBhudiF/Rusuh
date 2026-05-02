use std::sync::{Arc, OnceLock};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::{get, post};
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

const SECRET: &str = "test-oauth-secret";
const COPILOT_BASE_ENV: &str = "RUSUH_GITHUB_COPILOT_AUTH_BASE_URL";

fn copilot_env_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
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
        .expect("management request")
}

fn mgmt_post_json(uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("authorization", format!("Bearer {SECRET}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("management post request")
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = resp.into_body().collect().await.expect("collect body").to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(json!(null))
}

fn test_state(cfg: Config) -> Arc<ProxyState> {
    let auth_dir = cfg.auth_dir.clone();
    let accounts = Arc::new(AccountManager::with_dir(auth_dir));
    let registry = Arc::new(ModelRegistry::new());
    Arc::new(ProxyState::new(cfg, accounts, registry, 0))
}

fn test_app(state: Arc<ProxyState>) -> axum::Router {
    build_router(state.clone()).layer(axum::middleware::from_fn_with_state(
        state,
        rusuh::middleware::auth::api_key_auth,
    ))
}

#[derive(Clone)]
struct MockGithubCopilotManagementState {
    token_responses: Arc<Mutex<Vec<Value>>>,
    token_grant_requests: Arc<Mutex<Vec<String>>>,
    model_request_headers: Arc<Mutex<Vec<(Option<String>, Option<String>, Option<String>)>>>,
}

async fn mock_device_code_handler() -> (StatusCode, Json<Value>) {
    (
        StatusCode::OK,
        Json(json!({
            "device_code": "device-code-123",
            "user_code": "ABCD-EFGH",
            "verification_uri": "https://github.com/login/device",
            "expires_in": 60,
            "interval": 0
        })),
    )
}

async fn mock_token_handler(
    axum::extract::State(state): axum::extract::State<MockGithubCopilotManagementState>,
    body: String,
) -> (StatusCode, Json<Value>) {
    state.token_grant_requests.lock().await.push(body);
    let payload = state
        .token_responses
        .lock()
        .await
        .remove(0);
    (StatusCode::OK, Json(payload))
}

async fn mock_user_handler() -> (StatusCode, Json<Value>) {
    (
        StatusCode::OK,
        Json(json!({
            "id": 42,
            "login": "octocat",
            "email": "octocat@github.com",
            "name": "The Octocat"
        })),
    )
}

async fn mock_copilot_token_handler(
    axum::extract::State(_state): axum::extract::State<MockGithubCopilotManagementState>,
    _request: Request<Body>,
) -> (StatusCode, Json<Value>) {
    (
        StatusCode::OK,
        Json(json!({
            "token": "copilot-api-token",
            "expires_at": 4_102_444_800u64
        })),
    )
}

async fn mock_models_handler(
    axum::extract::State(state): axum::extract::State<MockGithubCopilotManagementState>,
    request: Request<Body>,
) -> (StatusCode, Json<Value>) {
    let editor_version = request
        .headers()
        .get("editor-version")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());
    let editor_plugin_version = request
        .headers()
        .get("editor-plugin-version")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());
    let copilot_integration_id = request
        .headers()
        .get("copilot-integration-id")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());

    state.model_request_headers.lock().await.push((
        editor_version.clone(),
        editor_plugin_version.clone(),
        copilot_integration_id.clone(),
    ));

    if editor_version.is_none() || editor_plugin_version.is_none() || copilot_integration_id.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "bad request: missing Editor-Version header for IDE auth"
            })),
        );
    }

    (
        StatusCode::OK,
        Json(json!({
            "data": [
                {
                    "id": "claude-haiku-4-5",
                    "object": "model",
                    "owned_by": "github-copilot"
                }
            ]
        })),
    )
}

async fn mock_models_error_handler() -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_GATEWAY,
        Json(json!({
            "error": "models unavailable"
        })),
    )
}

async fn spawn_copilot_management_server_with_models_route(
    token_responses: Vec<Value>,
    models_route: axum::routing::MethodRouter<MockGithubCopilotManagementState>,
) -> (
    String,
    Arc<Mutex<Vec<String>>>,
    Arc<Mutex<Vec<(Option<String>, Option<String>, Option<String>)>>>,
) {
    let token_grant_requests = Arc::new(Mutex::new(Vec::new()));
    let model_request_headers = Arc::new(Mutex::new(Vec::new()));
    let state = MockGithubCopilotManagementState {
        token_responses: Arc::new(Mutex::new(token_responses)),
        token_grant_requests: token_grant_requests.clone(),
        model_request_headers: model_request_headers.clone(),
    };

    let app = Router::new()
        .route("/login/device/code", post(mock_device_code_handler))
        .route("/login/oauth/access_token", post(mock_token_handler))
        .route("/user", get(mock_user_handler))
        .route("/copilot_internal/v2/token", get(mock_copilot_token_handler))
        .route("/models", models_route)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock copilot management server");
    let addr = listener.local_addr().expect("mock server addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    (format!("http://{addr}"), token_grant_requests, model_request_headers)
}

async fn spawn_copilot_management_server(
    token_responses: Vec<Value>,
) -> (
    String,
    Arc<Mutex<Vec<String>>>,
    Arc<Mutex<Vec<(Option<String>, Option<String>, Option<String>)>>>,
) {
    spawn_copilot_management_server_with_models_route(token_responses, get(mock_models_handler)).await
}

struct EnvVarGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.previous.as_deref() {
            Some(value) => unsafe {
                std::env::set_var(self.key, value);
            },
            None => unsafe {
                std::env::remove_var(self.key);
            },
        }
    }
}

#[tokio::test]
async fn github_copilot_management_start_returns_device_flow_payload() {
    let _lock = copilot_env_lock().lock().await;
    let dir = TempDir::new().expect("temp dir");
    let (base_url, _requests, _model_headers) =
        spawn_copilot_management_server(vec![json!({
            "access_token": "gho_test_token",
            "token_type": "bearer",
            "scope": "read:user"
        })])
        .await;
    let _env = EnvVarGuard::set(COPILOT_BASE_ENV, &base_url);

    let state = test_state(mgmt_config(dir.path().to_str().expect("auth dir")));
    let app = test_app(state);

    let resp = app
        .oneshot(mgmt_request(
            "/v0/management/oauth/start?provider=github-copilot",
        ))
        .await
        .expect("start response");

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "ok");
    assert_eq!(body["provider"], "github-copilot");
    assert_eq!(body["auth_method"], "device_code");
    assert_eq!(body["verification_uri"], "https://github.com/login/device");
    assert_eq!(body["user_code"], "ABCD-EFGH");
    assert!(!body["state"].as_str().unwrap_or_default().is_empty());
}

#[tokio::test]
async fn github_copilot_management_status_moves_from_pending_to_complete() {
    let _lock = copilot_env_lock().lock().await;
    let dir = TempDir::new().expect("temp dir");
    let (base_url, _requests, _model_headers) =
        spawn_copilot_management_server(vec![
            json!({"error": "authorization_pending"}),
            json!({
                "access_token": "gho_test_token",
                "token_type": "bearer",
                "scope": "read:user"
            }),
        ])
        .await;
    let _env = EnvVarGuard::set(COPILOT_BASE_ENV, &base_url);

    let state = test_state(mgmt_config(dir.path().to_str().expect("auth dir")));
    let app = test_app(state.clone());

    let start_resp = app
        .clone()
        .oneshot(mgmt_request(
            "/v0/management/oauth/start?provider=github-copilot",
        ))
        .await
        .expect("start response");
    let start_body = body_json(start_resp).await;
    let session_state = start_body["state"]
        .as_str()
        .expect("session state")
        .to_string();

    let immediate_status = app
        .clone()
        .oneshot(mgmt_request(&format!(
            "/v0/management/oauth/status?state={session_state}"
        )))
        .await
        .expect("immediate status response");
    let immediate_body = body_json(immediate_status).await;
    assert_eq!(immediate_body["status"], "wait");
    assert_eq!(immediate_body["provider"], "github-copilot");

    let mut completed_body = json!(null);
    for _ in 0..100 {
        let completed_status = app
            .clone()
            .oneshot(mgmt_request(&format!(
                "/v0/management/oauth/status?state={session_state}"
            )))
            .await
            .expect("completed status response");
        completed_body = body_json(completed_status).await;
        if completed_body["status"] == "ok" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert_eq!(completed_body["status"], "ok");
    assert_eq!(completed_body["provider"], "github-copilot");
}

#[tokio::test]
async fn github_copilot_management_completion_saves_auth_and_refreshes_runtime() {
    let _lock = copilot_env_lock().lock().await;
    let dir = TempDir::new().expect("temp dir");
    let (base_url, _requests, _model_headers) =
        spawn_copilot_management_server(vec![json!({
            "access_token": "gho_test_token",
            "token_type": "bearer",
            "scope": "read:user"
        })])
        .await;
    let _env = EnvVarGuard::set(COPILOT_BASE_ENV, &base_url);

    let state = test_state(mgmt_config(dir.path().to_str().expect("auth dir")));
    let app = test_app(state.clone());

    let start_resp = app
        .clone()
        .oneshot(mgmt_request(
            "/v0/management/oauth/start?provider=github-copilot",
        ))
        .await
        .expect("start response");
    let start_body = body_json(start_resp).await;
    let session_state = start_body["state"]
        .as_str()
        .expect("session state")
        .to_string();

    for _ in 0..20 {
        let status_resp = app
            .clone()
            .oneshot(mgmt_request(&format!(
                "/v0/management/oauth/status?state={session_state}"
            )))
            .await
            .expect("status response");
        let status_body = body_json(status_resp).await;
        if status_body["status"] == "ok" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    state.accounts.reload().await.expect("reload accounts");
    let copilot_accounts = state.accounts.accounts_for("github-copilot").await;
    assert_eq!(copilot_accounts.len(), 1);
    assert_eq!(copilot_accounts[0].label, "octocat@github.com");
    assert_eq!(
        copilot_accounts[0]
            .metadata
            .get("access_token")
            .and_then(Value::as_str),
        Some("gho_test_token")
    );

    let runtime_models = state
        .model_registry
        .available_clients_for_model("claude-sonnet-4-5")
        .await;
    assert_eq!(runtime_models.len(), 1);
    assert_eq!(runtime_models[0], "github-copilot-octocat.json");
}

#[tokio::test]
async fn github_copilot_management_failed_device_flow_marks_session_error() {
    let _lock = copilot_env_lock().lock().await;
    let dir = TempDir::new().expect("temp dir");
    let (base_url, requests, _model_headers) =
        spawn_copilot_management_server(vec![json!({"error": "access_denied"})]).await;
    let _env = EnvVarGuard::set(COPILOT_BASE_ENV, &base_url);

    let state = test_state(mgmt_config(dir.path().to_str().expect("auth dir")));
    let app = test_app(state);

    let start_resp = app
        .clone()
        .oneshot(mgmt_request(
            "/v0/management/oauth/start?provider=github-copilot",
        ))
        .await
        .expect("start response");
    let start_body = body_json(start_resp).await;
    let session_state = start_body["state"]
        .as_str()
        .expect("session state")
        .to_string();

    for _ in 0..20 {
        let status_resp = app
            .clone()
            .oneshot(mgmt_request(&format!(
                "/v0/management/oauth/status?state={session_state}"
            )))
            .await
            .expect("status response");
        let status_body = body_json(status_resp).await;
        if status_body["status"] == "error" {
            assert_eq!(status_body["provider"], "github-copilot");
            assert!(status_body["error"]
                .as_str()
                .unwrap_or_default()
                .contains("access_denied"));
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    assert_eq!(requests.lock().await.len(), 1);
}

#[tokio::test]
async fn github_copilot_management_models_returns_live_models_for_saved_account() {
    let _lock = copilot_env_lock().lock().await;
    let dir = TempDir::new().expect("temp dir");
    let (base_url, _requests, model_headers) =
        spawn_copilot_management_server(vec![json!({
            "access_token": "gho_test_token",
            "token_type": "bearer",
            "scope": "read:user"
        })])
        .await;
    let _env = EnvVarGuard::set(COPILOT_BASE_ENV, &base_url);

    let state = test_state(mgmt_config(dir.path().to_str().expect("auth dir")));
    let app = test_app(state.clone());

    let start_resp = app
        .clone()
        .oneshot(mgmt_request(
            "/v0/management/oauth/start?provider=github-copilot",
        ))
        .await
        .expect("start response");
    let start_body = body_json(start_resp).await;
    let session_state = start_body["state"]
        .as_str()
        .expect("session state")
        .to_string();

    for _ in 0..20 {
        let status_resp = app
            .clone()
            .oneshot(mgmt_request(&format!(
                "/v0/management/oauth/status?state={session_state}"
            )))
            .await
            .expect("status response");
        let status_body = body_json(status_resp).await;
        if status_body["status"] == "ok" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    let updated = state
        .accounts
        .update("github-copilot-octocat.json", |record| {
            record.metadata.insert(
                "copilot_api_url".to_string(),
                json!(format!("{base_url}")),
            );
            record.metadata.insert(
                "copilot_token_url".to_string(),
                json!(format!("{base_url}/copilot_internal/v2/token")),
            );
        })
        .await
        .expect("update copilot account");
    assert!(updated);

    let response = app
        .oneshot(mgmt_post_json(
            "/v0/management/github-copilot/models",
            json!({ "name": "github-copilot-octocat.json" }),
        ))
        .await
        .expect("models response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["account"], "github-copilot-octocat.json");
    assert_eq!(body["provider_key"], "github-copilot");
    assert_eq!(body["models"][0], "claude-haiku-4-5");
    let headers = model_headers.lock().await;
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].0.as_deref(), Some("vscode/1.99.0"));
    assert_eq!(headers[0].1.as_deref(), Some("copilot-chat/0.26.7"));
    assert_eq!(headers[0].2.as_deref(), Some("vscode-chat"));
}

#[tokio::test]
async fn github_copilot_management_models_returns_error_when_live_fetch_fails() {
    let _lock = copilot_env_lock().lock().await;
    let dir = TempDir::new().expect("temp dir");
    let (base_url, _requests, _model_headers) = spawn_copilot_management_server_with_models_route(
        vec![json!({
            "access_token": "gho_test_token",
            "token_type": "bearer",
            "scope": "read:user"
        })],
        get(mock_models_error_handler),
    )
    .await;
    let _env = EnvVarGuard::set(COPILOT_BASE_ENV, &base_url);

    let state = test_state(mgmt_config(dir.path().to_str().expect("auth dir")));
    let app = test_app(state.clone());

    let start_resp = app
        .clone()
        .oneshot(mgmt_request(
            "/v0/management/oauth/start?provider=github-copilot",
        ))
        .await
        .expect("start response");
    let start_body = body_json(start_resp).await;
    let session_state = start_body["state"]
        .as_str()
        .expect("session state")
        .to_string();

    for _ in 0..20 {
        let status_resp = app
            .clone()
            .oneshot(mgmt_request(&format!(
                "/v0/management/oauth/status?state={session_state}"
            )))
            .await
            .expect("status response");
        let status_body = body_json(status_resp).await;
        if status_body["status"] == "ok" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    let response = app
        .oneshot(mgmt_post_json(
            "/v0/management/github-copilot/models",
            json!({ "name": "github-copilot-octocat.json" }),
        ))
        .await
        .expect("models response");

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body = body_json(response).await;
    assert!(body["error"]
        .as_str()
        .unwrap_or_default()
        .contains("failed to fetch GitHub Copilot models"));
}
