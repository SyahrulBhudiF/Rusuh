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
use rusuh::proxy::ProxyState;
use rusuh::router::build_router;

const SECRET: &str = "test-mgmt-secret";

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

// ── Import tests ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn import_rejects_missing_user_id() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap())).await;

    let resp = app
        .oneshot(mgmt_request_json(
            "POST",
            "/v0/management/zed/import",
            json!({
                "name": "test-zed.json",
                "credential_json": "{\"token\":\"abc\"}"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert!(body["error"].as_str().unwrap().contains("user_id"));
}

#[tokio::test]
async fn import_rejects_whitespace_only_user_id() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap())).await;

    let resp = app
        .oneshot(mgmt_request_json(
            "POST",
            "/v0/management/zed/import",
            json!({
                "name": "test-zed.json",
                "user_id": "   ",
                "credential_json": "{\"token\":\"abc\"}"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert!(body["error"].as_str().unwrap().contains("user_id"));
}

#[tokio::test]
async fn import_rejects_missing_credential_json() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap())).await;

    let resp = app
        .oneshot(mgmt_request_json(
            "POST",
            "/v0/management/zed/import",
            json!({
                "name": "test-zed.json",
                "user_id": "test-user"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert!(body["error"].as_str().unwrap().contains("credential_json"));
}

#[tokio::test]
async fn import_rejects_whitespace_only_credential_json() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap())).await;

    let resp = app
        .oneshot(mgmt_request_json(
            "POST",
            "/v0/management/zed/import",
            json!({
                "name": "test-zed.json",
                "user_id": "test-user",
                "credential_json": "   "
            }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert!(body["error"].as_str().unwrap().contains("credential_json"));
}

#[tokio::test]
async fn import_writes_valid_zed_auth_file() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap())).await;

    let resp = app
        .oneshot(mgmt_request_json(
            "POST",
            "/v0/management/zed/import",
            json!({
                "name": "test-zed.json",
                "user_id": "test-user",
                "credential_json": "{\"token\":\"abc\"}"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["filename"], "test-zed.json");

    let file_path = dir.path().join("test-zed.json");
    assert!(file_path.exists());

    let file_text = std::fs::read_to_string(&file_path).unwrap();
    let expected = serde_json::to_string_pretty(&json!({
        "type": "zed",
        "user_id": "test-user",
        "credential_json": "{\"token\":\"abc\"}"
    }))
    .unwrap();
    assert_eq!(file_text, expected);

    let content: Value = serde_json::from_str(&file_text).unwrap();
    assert_eq!(content["type"], "zed");
    assert_eq!(content["user_id"], "test-user");
    assert_eq!(content["credential_json"], "{\"token\":\"abc\"}");
}

#[tokio::test]
async fn import_trims_and_persists_fields() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap())).await;

    let resp = app
        .oneshot(mgmt_request_json(
            "POST",
            "/v0/management/zed/import",
            json!({
                "name": "trimmed-zed.json",
                "user_id": "  test-user  ",
                "credential_json": "  {\"token\":\"abc\"}  "
            }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["filename"], "trimmed-zed.json");

    let file_path = dir.path().join("trimmed-zed.json");
    let content: Value =
        serde_json::from_str(&std::fs::read_to_string(&file_path).unwrap()).unwrap();
    assert_eq!(content["user_id"], "test-user");
    assert_eq!(content["credential_json"], "{\"token\":\"abc\"}");
}

#[tokio::test]
async fn import_auto_adds_json_extension() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap())).await;

    let resp = app
        .oneshot(mgmt_request_json(
            "POST",
            "/v0/management/zed/import",
            json!({
                "name": "test-zed",
                "user_id": "test-user",
                "credential_json": "{\"token\":\"abc\"}"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["filename"], "test-zed.json");

    let file_path = dir.path().join("test-zed.json");
    assert!(file_path.exists());
}

#[tokio::test]
async fn import_prevents_overwrite() {
    let dir = TempDir::new().unwrap();

    // Create existing file
    let existing = json!({
        "type": "zed",
        "user_id": "existing-user",
        "credential_json": "{\"token\":\"old\"}"
    });
    std::fs::write(
        dir.path().join("test-zed.json"),
        serde_json::to_string_pretty(&existing).unwrap(),
    )
    .unwrap();

    let app = test_app(mgmt_config(dir.path().to_str().unwrap())).await;

    let resp = app
        .oneshot(mgmt_request_json(
            "POST",
            "/v0/management/zed/import",
            json!({
                "name": "test-zed.json",
                "user_id": "new-user",
                "credential_json": "{\"token\":\"new\"}"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = body_json(resp).await;
    assert!(body["error"].as_str().unwrap().contains("already exists"));

    // Verify original file unchanged
    let file_path = dir.path().join("test-zed.json");
    let content: Value =
        serde_json::from_str(&std::fs::read_to_string(&file_path).unwrap()).unwrap();
    assert_eq!(content["user_id"], "existing-user");
}

#[tokio::test]
async fn import_rejects_traversal_name_and_does_not_write_outside_auth_dir() {
    let dir = TempDir::new().unwrap();
    let auth_dir = dir.path().join("nested").join("auth");
    std::fs::create_dir_all(&auth_dir).unwrap();

    let outside_path = dir.path().join("nested").join("outside.json");
    assert!(!outside_path.exists());

    let app = test_app(mgmt_config(auth_dir.to_str().unwrap())).await;

    let resp = app
        .oneshot(mgmt_request_json(
            "POST",
            "/v0/management/zed/import",
            json!({
                "name": "../outside",
                "user_id": "test-user",
                "credential_json": "{\"token\":\"abc\"}"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert!(body["error"].as_str().unwrap().contains("invalid filename"));
    assert!(!outside_path.exists());
    assert!(!auth_dir.join("../outside.json").exists());
    assert!(!auth_dir.join(r"..\outside.json").exists());
}

// ── Quota tests ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn check_quota_returns_flattened_response() {
    let dir = TempDir::new().unwrap();
    let auth = json!({
        "type": "zed",
        "user_id": "test-user",
        "credential_json": "{\"token\":\"test-token\"}"
    });
    std::fs::write(
        dir.path().join("work-zed.json"),
        serde_json::to_string_pretty(&auth).unwrap(),
    )
    .unwrap();

    let app = test_app(mgmt_config(dir.path().to_str().unwrap())).await;

    let resp = app
        .oneshot(mgmt_request_json(
            "POST",
            "/v0/management/zed/check-quota",
            json!({"name": "work-zed.json"}),
        ))
        .await
        .unwrap();

    // Response should have flattened structure even if upstream fails
    let body = body_json(resp).await;
    assert!(body.get("account").is_some());
    assert!(body.get("status").is_some());
    assert!(body.get("plan").is_some());
    assert!(body.get("plan_v2").is_some());
    assert!(body.get("plan_v3").is_some());
    assert!(body.get("subscription_started_at").is_some());
    assert!(body.get("subscription_ended_at").is_some());
    assert!(body.get("model_requests_used").is_some());
    assert!(body.get("model_requests_limit").is_some());
    assert!(body.get("edit_predictions_used").is_some());
    assert!(body.get("edit_predictions_limit").is_some());
    assert!(body.get("is_account_too_young").is_some());
    assert!(body.get("has_overdue_invoices").is_some());
    assert!(body.get("is_usage_based_billing_enabled").is_some());
    assert!(body.get("feature_flags").is_some());
    assert!(body.get("error").is_some());
    assert!(body.get("upstream_status").is_some());
}

#[tokio::test]
async fn check_quota_matches_by_auth_record_id() {
    let dir = TempDir::new().unwrap();
    let auth = json!({
        "type": "zed",
        "user_id": "test-user",
        "credential_json": "{\"token\":\"test-token\"}"
    });
    std::fs::write(
        dir.path().join("work-zed.json"),
        serde_json::to_string_pretty(&auth).unwrap(),
    )
    .unwrap();

    let app = test_app(mgmt_config(dir.path().to_str().unwrap())).await;

    // Should match by basename
    let resp = app
        .oneshot(mgmt_request_json(
            "POST",
            "/v0/management/zed/check-quota",
            json!({"name": "work-zed.json"}),
        ))
        .await
        .unwrap();

    let body = body_json(resp).await;
    assert_eq!(body["account"], "work-zed.json");
}

#[tokio::test]
async fn check_quota_returns_error_for_nonexistent_account() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap())).await;

    let resp = app
        .oneshot(mgmt_request_json(
            "POST",
            "/v0/management/zed/check-quota",
            json!({"name": "nonexistent.json"}),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert!(body["error"].as_str().unwrap().contains("not found"));
}
