use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Request, State};
use axum::http::StatusCode;
use clap::Parser;
use axum::routing::{get, post};
use axum::{Json, Router};
use http_body_util::BodyExt;
use rusuh::auth::cli::{Cli, Commands};
use rusuh::auth::github_copilot::{
    build_github_copilot_auth_record, create_token_storage, credential_file_name,
    preferred_label, save_auth_bundle, GithubCopilotAuthBundle, GithubCopilotTokenStorage,
    GithubOAuthTokenData, GithubUserInfo,
};
use rusuh::auth::store::FileTokenStore;
use serde_json::{json, Value};
use tokio::sync::Mutex;

#[test]
fn github_copilot_auth_filename_uses_username() {
    assert_eq!(
        credential_file_name("octocat"),
        "github-copilot-octocat.json"
    );
}

#[test]
fn github_copilot_auth_label_prefers_email_then_username() {
    let email_user = GithubUserInfo {
        id: 1,
        login: "octocat".into(),
        email: Some("octocat@github.com".into()),
        name: Some("The Octocat".into()),
    };
    assert_eq!(preferred_label(&email_user), "octocat@github.com");

    let username_only_user = GithubUserInfo {
        id: 2,
        login: "hubot".into(),
        email: None,
        name: None,
    };
    assert_eq!(preferred_label(&username_only_user), "hubot");
}

#[test]
fn github_copilot_auth_metadata_persists_github_oauth_fields_only() {
    let bundle = GithubCopilotAuthBundle {
        token_data: GithubOAuthTokenData {
            access_token: "gho_test_token".into(),
            token_type: "bearer".into(),
            scope: "read:user".into(),
        },
        user_info: GithubUserInfo {
            id: 42,
            login: "octocat".into(),
            email: Some("octocat@github.com".into()),
            name: Some("The Octocat".into()),
        },
    };

    let storage = create_token_storage(&bundle);
    assert_eq!(
        storage,
        GithubCopilotTokenStorage {
            access_token: "gho_test_token".into(),
            token_type: "bearer".into(),
            scope: "read:user".into(),
            user_id: 42,
            username: "octocat".into(),
            email: Some("octocat@github.com".into()),
            name: Some("The Octocat".into()),
        }
    );

    let record = build_github_copilot_auth_record(&bundle).expect("record should build");

    assert_eq!(
        record.metadata.get("access_token").and_then(|v| v.as_str()),
        Some("gho_test_token")
    );
    assert_eq!(
        record.metadata.get("token_type").and_then(|v| v.as_str()),
        Some("bearer")
    );
    assert_eq!(
        record.metadata.get("scope").and_then(|v| v.as_str()),
        Some("read:user")
    );
    assert_eq!(
        record.metadata.get("username").and_then(|v| v.as_str()),
        Some("octocat")
    );
    assert_eq!(
        record.metadata.get("email").and_then(|v| v.as_str()),
        Some("octocat@github.com")
    );
    assert!(!record.metadata.contains_key("copilot_api_token"));
    assert!(!record.metadata.contains_key("expires_at"));
    assert!(!record.metadata.contains_key("endpoint"));
}

#[test]
fn github_copilot_auth_record_uses_provider_key() {
    let bundle = GithubCopilotAuthBundle {
        token_data: GithubOAuthTokenData {
            access_token: "gho_test_token".into(),
            token_type: "bearer".into(),
            scope: "read:user".into(),
        },
        user_info: GithubUserInfo {
            id: 42,
            login: "octocat".into(),
            email: Some("octocat@github.com".into()),
            name: None,
        },
    };

    let record = build_github_copilot_auth_record(&bundle).expect("record should build");

    assert_eq!(record.provider, "github-copilot");
    assert_eq!(record.provider_key, "github-copilot");
    assert_eq!(record.id, "github-copilot-octocat.json");
    assert_eq!(record.label, "octocat@github.com");
    assert_eq!(
        record.metadata.get("provider_key").and_then(|v| v.as_str()),
        Some("github-copilot")
    );
    assert_eq!(
        record.metadata.get("type").and_then(|v| v.as_str()),
        Some("github-copilot")
    );
}

#[tokio::test]
async fn github_copilot_auth_save_persists_canonical_file() {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let store = FileTokenStore::new(temp_dir.path());
    let bundle = GithubCopilotAuthBundle {
        token_data: GithubOAuthTokenData {
            access_token: "gho_test_token".into(),
            token_type: "bearer".into(),
            scope: "read:user".into(),
        },
        user_info: GithubUserInfo {
            id: 42,
            login: "octocat".into(),
            email: Some("octocat@github.com".into()),
            name: Some("The Octocat".into()),
        },
    };

    let saved = save_auth_bundle(&store, &bundle)
        .await
        .expect("save should succeed");

    assert_eq!(
        saved.file_name().and_then(|value| value.to_str()),
        Some("github-copilot-octocat.json")
    );

    let records = store.list().await.expect("list auth files");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].provider, "github-copilot");
    assert_eq!(records[0].provider_key, "github-copilot");
    assert_eq!(records[0].label, "octocat@github.com");
}

#[test]
fn github_copilot_device_flow_cli_command_parses() {
    let cli = Cli::parse_from(["rusuh", "github-copilot-login"]);
    assert!(matches!(cli.command, Some(Commands::GithubCopilotLogin)));
}

#[derive(Clone)]
struct MockGithubCopilotLoginState {
    token_responses: Arc<Mutex<Vec<(StatusCode, Value)>>>,
    user_response: Value,
    copilot_token_response: Value,
    token_grant_requests: Arc<Mutex<Vec<String>>>,
}

async fn mock_device_code_handler() -> (StatusCode, Json<Value>) {
    (
        StatusCode::OK,
        Json(json!({
            "device_code": "device-code-123",
            "user_code": "ABCD-EFGH",
            "verification_uri": "https://github.com/login/device",
            "expires_in": 900,
            "interval": 0
        })),
    )
}

async fn mock_token_handler(
    State(state): State<MockGithubCopilotLoginState>,
    request: Request,
) -> (StatusCode, Json<Value>) {
    let body = request.into_body().collect().await.expect("read body").to_bytes();
    state
        .token_grant_requests
        .lock()
        .await
        .push(String::from_utf8(body.to_vec()).expect("utf8 body"));

    let mut responses = state.token_responses.lock().await;
    if responses.is_empty() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "missing mock token response"})),
        );
    }

    let (status, body) = responses.remove(0);
    (status, Json(body))
}

async fn mock_user_handler(
    State(state): State<MockGithubCopilotLoginState>,
) -> (StatusCode, Json<Value>) {
    (StatusCode::OK, Json(state.user_response))
}

async fn mock_copilot_token_handler(
    State(state): State<MockGithubCopilotLoginState>,
) -> (StatusCode, Json<Value>) {
    (StatusCode::OK, Json(state.copilot_token_response))
}

async fn spawn_github_copilot_login_server(
    token_responses: Vec<(StatusCode, Value)>,
) -> (String, Arc<Mutex<Vec<String>>>) {
    let token_grant_requests = Arc::new(Mutex::new(Vec::new()));
    let state = MockGithubCopilotLoginState {
        token_responses: Arc::new(Mutex::new(token_responses)),
        user_response: json!({
            "id": 42,
            "login": "octocat",
            "email": "octocat@github.com",
            "name": "The Octocat"
        }),
        copilot_token_response: json!({
            "token": "copilot-api-token",
            "expires_at": 4_102_444_800u64,
            "endpoint": "https://api.githubcopilot.com"
        }),
        token_grant_requests: token_grant_requests.clone(),
    };

    let app = Router::new()
        .route("/login/device/code", post(mock_device_code_handler))
        .route("/login/oauth/access_token", post(mock_token_handler))
        .route("/user", get(mock_user_handler))
        .route("/copilot_internal/v2/token", get(mock_copilot_token_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind login mock server");
    let addr = listener.local_addr().expect("mock addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    (format!("http://{addr}"), token_grant_requests)
}

#[tokio::test]
async fn github_copilot_auth_device_flow_bootstrap_and_success_path() {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let store = FileTokenStore::new(temp_dir.path());
    let (base_url, token_grant_requests) = spawn_github_copilot_login_server(vec![(
        StatusCode::OK,
        json!({
            "access_token": "gho_test_token",
            "token_type": "bearer",
            "scope": "read:user"
        }),
    )])
    .await;

    let saved = rusuh::auth::github_copilot_login::login_with_base_url(&store, &base_url)
        .await
        .expect("device flow should succeed");

    assert_eq!(
        saved.file_name().and_then(|value| value.to_str()),
        Some("github-copilot-octocat.json")
    );

    let requests = token_grant_requests.lock().await;
    assert_eq!(requests.len(), 1);
    assert!(requests[0].contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code"));
    assert!(requests[0].contains("device_code=device-code-123"));
}

#[tokio::test]
async fn github_copilot_auth_device_flow_retries_authorization_pending() {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let store = FileTokenStore::new(temp_dir.path());
    let (base_url, token_grant_requests) = spawn_github_copilot_login_server(vec![
        (
            StatusCode::OK,
            json!({"error": "authorization_pending"}),
        ),
        (
            StatusCode::OK,
            json!({
                "access_token": "gho_test_token",
                "token_type": "bearer",
                "scope": "read:user"
            }),
        ),
    ])
    .await;

    rusuh::auth::github_copilot_login::login_with_base_url(&store, &base_url)
        .await
        .expect("device flow should eventually succeed");

    assert_eq!(token_grant_requests.lock().await.len(), 2);
}

#[tokio::test]
async fn github_copilot_auth_device_flow_slow_down_increases_polling() {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let store = FileTokenStore::new(temp_dir.path());
    let (base_url, token_grant_requests) = spawn_github_copilot_login_server(vec![
        (StatusCode::OK, json!({"error": "slow_down"})),
        (
            StatusCode::OK,
            json!({
                "access_token": "gho_test_token",
                "token_type": "bearer",
                "scope": "read:user"
            }),
        ),
    ])
    .await;

    let started = tokio::time::Instant::now();
    rusuh::auth::github_copilot_login::login_with_base_url(&store, &base_url)
        .await
        .expect("device flow should eventually succeed after slow_down");

    assert_eq!(token_grant_requests.lock().await.len(), 2);
    assert!(started.elapsed() >= Duration::from_secs(10));
}

#[tokio::test]
async fn github_copilot_auth_device_flow_expired_token_is_terminal() {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let store = FileTokenStore::new(temp_dir.path());
    let (base_url, _token_grant_requests) = spawn_github_copilot_login_server(vec![(
        StatusCode::OK,
        json!({"error": "expired_token"}),
    )])
    .await;

    let error = rusuh::auth::github_copilot_login::login_with_base_url(&store, &base_url)
        .await
        .expect_err("expired_token should fail");
    assert!(error.to_string().contains("expired_token"));
}

#[tokio::test]
async fn github_copilot_auth_device_flow_access_denied_is_terminal() {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let store = FileTokenStore::new(temp_dir.path());
    let (base_url, _token_grant_requests) = spawn_github_copilot_login_server(vec![(
        StatusCode::OK,
        json!({"error": "access_denied"}),
    )])
    .await;

    let error = rusuh::auth::github_copilot_login::login_with_base_url(&store, &base_url)
        .await
        .expect_err("access_denied should fail");
    assert!(error.to_string().contains("access_denied"));
}

#[test]
fn github_copilot_runtime_validates_trusted_hosts() {
    assert!(rusuh::auth::github_copilot_runtime::is_trusted_copilot_host(
        "api.githubcopilot.com"
    ));
    assert!(rusuh::auth::github_copilot_runtime::is_trusted_copilot_host(
        "api.individual.githubcopilot.com"
    ));
    assert!(rusuh::auth::github_copilot_runtime::is_trusted_copilot_host(
        "api.business.githubcopilot.com"
    ));
    assert!(rusuh::auth::github_copilot_runtime::is_trusted_copilot_host(
        "copilot-proxy.githubusercontent.com"
    ));
    assert!(!rusuh::auth::github_copilot_runtime::is_trusted_copilot_host(
        "evil.example.com"
    ));
}

#[test]
fn github_copilot_runtime_expiry_buffer_refresh_logic() {
    let valid_until = chrono::Utc::now() + chrono::Duration::minutes(10);
    assert!(rusuh::auth::github_copilot_runtime::token_is_still_valid_until(
        valid_until,
        chrono::Utc::now()
    ));

    let near_expiry = chrono::Utc::now() + chrono::Duration::minutes(4);
    assert!(!rusuh::auth::github_copilot_runtime::token_is_still_valid_until(
        near_expiry,
        chrono::Utc::now()
    ));
}

#[derive(Clone)]
struct MockGithubCopilotRuntimeState {
    body: Value,
}

async fn mock_runtime_token_handler(
    State(state): State<MockGithubCopilotRuntimeState>,
) -> (StatusCode, Json<Value>) {
    (StatusCode::OK, Json(state.body))
}

async fn spawn_runtime_token_server(body: Value) -> String {
    let state = MockGithubCopilotRuntimeState { body };
    let app = Router::new()
        .route("/copilot_internal/v2/token", get(mock_runtime_token_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind runtime server");
    let addr = listener.local_addr().expect("runtime addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    format!("http://{addr}")
}

#[tokio::test]
async fn github_copilot_auth_runtime_token_exchange_parses_response() {
    let base_url = spawn_runtime_token_server(json!({
        "token": "copilot-api-token",
        "expires_at": 4_102_444_800u64,
        "endpoint": "https://api.githubcopilot.com"
    }))
    .await;

    let token = rusuh::auth::github_copilot_runtime::exchange_github_token_for_copilot_token_with_url(
        &reqwest::Client::new(),
        "gho_test_token",
        &format!("{base_url}/copilot_internal/v2/token"),
    )
    .await
    .expect("token exchange should succeed");

    assert_eq!(token.token, "copilot-api-token");
    assert_eq!(token.endpoint.as_deref(), Some("https://api.githubcopilot.com"));
}
