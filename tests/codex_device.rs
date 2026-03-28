use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use rusuh::auth::codex_device::{
    CodexDeviceEndpoints, codex_device_approval_url, codex_device_is_success_status,
    parse_codex_device_countdown_start_secs, parse_codex_device_poll_interval_secs,
    parse_device_token_response, parse_device_user_code_response, poll_codex_device_token,
    request_codex_device_user_code,
};
use serde_json::{json, Value};
use tokio::sync::Mutex;

#[test]
fn parse_device_user_code_response_supports_user_code_variants() {
    let first = parse_device_user_code_response(&json!({
        "device_auth_id": "dev_1",
        "user_code": "ABC-123",
        "verification_uri": "https://auth.openai.com/device",
        "interval": 7
    }))
    .expect("user_code variant should parse");

    assert_eq!(first.device_auth_id, "dev_1");
    assert_eq!(first.user_code, "ABC-123");
    assert_eq!(first.verification_uri, "https://auth.openai.com/device");

    let second = parse_device_user_code_response(&json!({
        "device_auth_id": "dev_2",
        "usercode": "XYZ-789",
        "verification_url": "https://auth.openai.com/device",
        "interval": "9"
    }))
    .expect("usercode fallback should parse");

    assert_eq!(second.device_auth_id, "dev_2");
    assert_eq!(second.user_code, "XYZ-789");
    assert_eq!(second.verification_uri, "https://auth.openai.com/device");
}

#[test]
fn parse_device_token_response_requires_required_fields() {
    let parsed = parse_device_token_response(&json!({
        "authorization_code": "auth_code",
        "code_verifier": "verifier",
        "code_challenge": "challenge"
    }))
    .expect("token response should parse");

    assert_eq!(parsed.authorization_code, "auth_code");
    assert_eq!(parsed.code_verifier, "verifier");
    assert_eq!(parsed.code_challenge, "challenge");

    let err = parse_device_token_response(&json!({"authorization_code": "missing"}))
        .expect_err("missing fields should fail");
    assert!(err.to_string().contains("missing required fields"));
}

#[test]
fn parse_poll_interval_accepts_number_and_string() {
    assert_eq!(parse_codex_device_poll_interval_secs(&json!(11)), 11);
    assert_eq!(parse_codex_device_poll_interval_secs(&json!("13")), 13);
    assert_eq!(parse_codex_device_poll_interval_secs(&json!("bad")), 5);
    assert_eq!(parse_codex_device_poll_interval_secs(&json!(null)), 5);
}

#[test]
fn countdown_start_uses_numeric_expires_in_or_default() {
    assert_eq!(parse_codex_device_countdown_start_secs(&json!(600)), 600);
    assert_eq!(parse_codex_device_countdown_start_secs(&json!("120")), 120);
    assert_eq!(parse_codex_device_countdown_start_secs(&json!(null)), 600);
}

#[test]
fn approval_url_prefers_complete_url_when_present() {
    let complete = parse_device_user_code_response(&json!({
        "device_auth_id": "dev_1",
        "user_code": "ABC-123",
        "verification_uri": "https://auth.openai.com/device",
        "verification_uri_complete": "https://auth.openai.com/device?user_code=ABC-123",
        "interval": 7
    }))
    .expect("response should parse");

    assert_eq!(
        codex_device_approval_url(&complete),
        "https://auth.openai.com/device?user_code=ABC-123"
    );

    let fallback = parse_device_user_code_response(&json!({
        "device_auth_id": "dev_2",
        "user_code": "XYZ-789",
        "verification_uri": "https://auth.openai.com/device",
        "interval": 7
    }))
    .expect("response should parse");

    assert_eq!(
        codex_device_approval_url(&fallback),
        "https://auth.openai.com/device"
    );
}

#[test]
fn success_status_is_any_2xx() {
    assert!(codex_device_is_success_status(200));
    assert!(codex_device_is_success_status(299));
    assert!(!codex_device_is_success_status(403));
    assert!(!codex_device_is_success_status(500));
}

#[test]
fn custom_auth_base_url_keeps_device_verification_on_same_host() {
    let endpoints = CodexDeviceEndpoints::from_auth_base_url("https://auth.example.test")
        .expect("custom auth base URL should build endpoints");

    assert_eq!(
        endpoints.user_code_url,
        "https://auth.example.test/api/accounts/deviceauth/usercode"
    );
    assert_eq!(
        endpoints.token_url,
        "https://auth.example.test/api/accounts/deviceauth/token"
    );
    assert_eq!(
        endpoints.verification_url,
        "https://auth.example.test/api/accounts/deviceauth/verify"
    );
    assert_eq!(
        endpoints.token_exchange_url,
        "https://auth.example.test/oauth/token"
    );
}

#[test]
fn parse_device_user_code_response_requires_verification_uri() {
    let err = parse_device_user_code_response(&json!({
        "device_auth_id": "dev_1",
        "user_code": "ABC-123",
        "interval": 7
    }))
    .expect_err("missing verification uri should fail");

    assert!(err
        .to_string()
        .contains("device flow response missing verification_uri"));
}

#[derive(Clone)]
struct MockDeviceState {
    user_code_status: StatusCode,
    user_code_body: Value,
    poll_bodies: Arc<Mutex<Vec<(StatusCode, Value)>>>,
    user_code_requests: Arc<Mutex<Vec<Value>>>,
    poll_requests: Arc<Mutex<Vec<Value>>>,
}

async fn mock_user_code_handler(
    State(state): State<MockDeviceState>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    state.user_code_requests.lock().await.push(body);
    (state.user_code_status, Json(state.user_code_body))
}

async fn mock_poll_handler(
    State(state): State<MockDeviceState>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    state.poll_requests.lock().await.push(body);

    let mut poll_bodies = state.poll_bodies.lock().await;
    if poll_bodies.is_empty() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "poll response not configured"})),
        );
    }

    let (status, response_body) = poll_bodies.remove(0);
    (status, Json(response_body))
}

async fn spawn_device_mock_server(
    user_code_status: StatusCode,
    user_code_body: Value,
    poll_bodies: Vec<(StatusCode, Value)>,
) -> (String, Arc<Mutex<Vec<Value>>>, Arc<Mutex<Vec<Value>>>) {
    let user_code_requests = Arc::new(Mutex::new(Vec::new()));
    let poll_requests = Arc::new(Mutex::new(Vec::new()));

    let state = MockDeviceState {
        user_code_status,
        user_code_body,
        poll_bodies: Arc::new(Mutex::new(poll_bodies)),
        user_code_requests: user_code_requests.clone(),
        poll_requests: poll_requests.clone(),
    };

    let app = Router::new()
        .route("/usercode", post(mock_user_code_handler))
        .route("/token", post(mock_poll_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock listener");
    let addr = listener.local_addr().expect("read listener addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    (format!("http://{addr}"), user_code_requests, poll_requests)
}

#[tokio::test]
async fn request_device_user_code_parses_success_payload() {
    let user_code_body = json!({
        "device_auth_id": "dev_123",
        "usercode": "ABC-123",
        "interval": "7"
    });

    let (base_url, user_code_requests, _poll_requests) =
        spawn_device_mock_server(StatusCode::OK, user_code_body, vec![]).await;

    let response = request_codex_device_user_code(
        &reqwest::Client::new(),
        &format!("{base_url}/usercode"),
        "https://auth.openai.com/codex/device",
    )
    .await
    .expect("user code request should succeed");

    assert_eq!(response.device_auth_id, "dev_123");
    assert_eq!(response.user_code, "ABC-123");
    assert_eq!(response.interval_secs, 7);
    assert_eq!(
        response.verification_uri,
        "https://auth.openai.com/codex/device"
    );

    let requests = user_code_requests.lock().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].get("client_id").and_then(Value::as_str),
        Some(rusuh::auth::codex::CLIENT_ID)
    );
}

#[tokio::test]
async fn request_device_user_code_404_is_reported_as_unavailable() {
    let user_code_body = json!({"error": "missing"});

    let (base_url, _user_code_requests, _poll_requests) =
        spawn_device_mock_server(StatusCode::NOT_FOUND, user_code_body, vec![]).await;

    let error = request_codex_device_user_code(
        &reqwest::Client::new(),
        &format!("{base_url}/usercode"),
        "https://auth.openai.com/codex/device",
    )
    .await
    .expect_err("404 should fail");

    assert!(error
        .to_string()
        .contains("codex device endpoint is unavailable (status 404)"));
}

#[tokio::test]
async fn poll_device_token_retries_for_403_and_404_until_success() {
    let user_code_body = json!({
        "device_auth_id": "unused",
        "user_code": "unused",
        "verification_uri": "https://auth.openai.com/codex/device",
        "interval": 1
    });

    let poll_bodies = vec![
        (StatusCode::FORBIDDEN, json!({"error": "pending"})),
        (StatusCode::NOT_FOUND, json!({"error": "pending"})),
        (
            StatusCode::OK,
            json!({
                "authorization_code": "auth_code",
                "code_verifier": "verifier",
                "code_challenge": "challenge"
            }),
        ),
    ];

    let (base_url, _user_code_requests, poll_requests) =
        spawn_device_mock_server(StatusCode::OK, user_code_body, poll_bodies).await;

    let response = poll_codex_device_token(
        &reqwest::Client::new(),
        &format!("{base_url}/token"),
        "dev_123",
        "ABC-123",
        Duration::from_millis(1),
        Duration::from_secs(1),
    )
    .await
    .expect("polling should eventually succeed");

    assert_eq!(response.authorization_code, "auth_code");
    assert_eq!(response.code_verifier, "verifier");
    assert_eq!(response.code_challenge, "challenge");

    let requests = poll_requests.lock().await;
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests[0].get("device_auth_id").and_then(Value::as_str),
        Some("dev_123")
    );
    assert_eq!(
        requests[0].get("user_code").and_then(Value::as_str),
        Some("ABC-123")
    );
}
