//! Integration tests for auth file CRUD endpoints.

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

fn mgmt_request(method: &str, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {SECRET}"))
        .body(Body::empty())
        .unwrap()
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

// ── List (empty) ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn list_auth_files_empty() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(mgmt_request("GET", "/v0/management/auth-files"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["auth-files"].as_array().unwrap().len(), 0);
}

// ── Upload + List ────────────────────────────────────────────────────────────

#[tokio::test]
async fn upload_and_list_auth_file() {
    let dir = TempDir::new().unwrap();
    let cfg = mgmt_config(dir.path().to_str().unwrap());
    let app = test_app(cfg.clone());

    let auth_json = json!({
        "type": "antigravity",
        "email": "test@example.com",
        "access_token": "ya29.test",
        "refresh_token": "1//test",
        "project_id": "test-project"
    });

    let resp = app
        .oneshot(mgmt_request_json(
            "POST",
            "/v0/management/auth-files?name=test.json",
            auth_json,
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    // List should now show it
    let app2 = test_app(cfg);
    let resp = app2
        .oneshot(mgmt_request("GET", "/v0/management/auth-files"))
        .await
        .unwrap();

    let body = body_json(resp).await;
    let files = body["auth-files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["id"], "test.json");
    assert_eq!(files[0]["type"], "antigravity");
    assert_eq!(files[0]["email"], "test@example.com");
}

// ── Upload validation ────────────────────────────────────────────────────────

#[tokio::test]
async fn upload_rejects_non_json_name() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(mgmt_request_json(
            "POST",
            "/v0/management/auth-files?name=bad.txt",
            json!({"type": "test"}),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn upload_rejects_invalid_json_body() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v0/management/auth-files?name=test.json")
                .header("authorization", format!("Bearer {SECRET}"))
                .header("content-type", "application/json")
                .body(Body::from("not valid json{{{"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ── Download ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn download_auth_file() {
    let dir = TempDir::new().unwrap();
    let auth_data = json!({"type": "antigravity", "email": "dl@test.com"});
    std::fs::write(
        dir.path().join("dl-test.json"),
        serde_json::to_string_pretty(&auth_data).unwrap(),
    )
    .unwrap();

    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(mgmt_request(
            "GET",
            "/v0/management/auth-files/download?name=dl-test.json",
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let cd = resp
        .headers()
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(cd.contains("dl-test.json"));

    let body = body_json(resp).await;
    assert_eq!(body["email"], "dl@test.com");
}

#[tokio::test]
async fn download_nonexistent_returns_404() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(mgmt_request(
            "GET",
            "/v0/management/auth-files/download?name=nope.json",
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── Delete single ────────────────────────────────────────────────────────────

#[tokio::test]
async fn delete_single_auth_file() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("del.json"),
        r#"{"type": "test"}"#,
    )
    .unwrap();

    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(mgmt_request(
            "DELETE",
            "/v0/management/auth-files?name=del.json",
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert!(!dir.path().join("del.json").exists());
}

#[tokio::test]
async fn delete_nonexistent_returns_404() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(mgmt_request(
            "DELETE",
            "/v0/management/auth-files?name=nope.json",
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── Delete all ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn delete_all_auth_files() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.json"), r#"{"type":"t"}"#).unwrap();
    std::fs::write(dir.path().join("b.json"), r#"{"type":"t"}"#).unwrap();
    std::fs::write(dir.path().join("keep.txt"), "not json").unwrap();

    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(mgmt_request(
            "DELETE",
            "/v0/management/auth-files?all=true",
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["deleted"], 2);
    // Non-json files preserved
    assert!(dir.path().join("keep.txt").exists());
}

// ── Patch status ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn patch_status_disables_auth_file() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("status.json"),
        r#"{"type": "antigravity", "disabled": false}"#,
    )
    .unwrap();

    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(mgmt_request_json(
            "PATCH",
            "/v0/management/auth-files/status",
            json!({"name": "status.json", "disabled": true}),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["disabled"], true);

    // Verify file on disk
    let data: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join("status.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(data["disabled"], true);
    assert_eq!(data["status"], "disabled");
}

// ── Patch fields ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn patch_fields_updates_priority() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("fields.json"),
        r#"{"type": "antigravity"}"#,
    )
    .unwrap();

    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(mgmt_request_json(
            "PATCH",
            "/v0/management/auth-files/fields",
            json!({"name": "fields.json", "priority": 10, "prefix": "us-"}),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let data: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join("fields.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(data["priority"], 10);
    assert_eq!(data["prefix"], "us-");
}

#[tokio::test]
async fn patch_fields_no_fields_returns_400() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("nf.json"),
        r#"{"type": "test"}"#,
    )
    .unwrap();

    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(mgmt_request_json(
            "PATCH",
            "/v0/management/auth-files/fields",
            json!({"name": "nf.json"}),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ── Auth required ────────────────────────────────────────────────────────────

#[tokio::test]
async fn auth_files_requires_management_key() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v0/management/auth-files")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── Path traversal protection ──────────────────────────────────────────────────

#[tokio::test]
async fn upload_rejects_path_traversal_dotdot() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(mgmt_request_json(
            "POST",
            "/v0/management/auth-files?name=../../../etc/passwd.json",
            json!({"type": "evil"}),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert!(body["error"].as_str().unwrap().contains("invalid filename"));
}

#[tokio::test]
async fn upload_rejects_forward_slash_in_name() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(mgmt_request_json(
            "POST",
            "/v0/management/auth-files?name=subdir/evil.json",
            json!({"type": "evil"}),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn upload_rejects_backslash_in_name() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(mgmt_request_json(
            "POST",
            "/v0/management/auth-files?name=subdir%5Cevil.json",
            json!({"type": "evil"}),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn upload_rejects_null_bytes() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(mgmt_request_json(
            "POST",
            "/v0/management/auth-files?name=evil%00.json",
            json!({"type": "evil"}),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn upload_rejects_non_ascii() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(mgmt_request_json(
            "POST",
            "/v0/management/auth-files?name=%C3%A9vil.json",
            json!({"type": "evil"}),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn delete_rejects_path_traversal() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(mgmt_request(
            "DELETE",
            "/v0/management/auth-files?name=../../etc/passwd.json",
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn download_rejects_path_traversal() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(mgmt_request(
            "GET",
            "/v0/management/auth-files/download?name=../secret.json",
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn patch_status_rejects_path_traversal() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(mgmt_request_json(
            "PATCH",
            "/v0/management/auth-files/status",
            json!({"name": "../../../etc/shadow.json", "disabled": true}),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn patch_fields_rejects_path_traversal() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    let resp = app
        .oneshot(mgmt_request_json(
            "PATCH",
            "/v0/management/auth-files/fields",
            json!({"name": "../../evil.json", "priority": 99}),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ── Upload size limit ────────────────────────────────────────────────────────

#[tokio::test]
async fn upload_rejects_oversized_body() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap()));

    // 1 MiB + 1 byte should exceed the limit
    let oversized = "x".repeat(1024 * 1024 + 1);
    let body_json_str = format!(r#"{{"data": "{oversized}"}}"#);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v0/management/auth-files?name=big.json")
                .header("authorization", format!("Bearer {SECRET}"))
                .header("content-type", "application/json")
                .body(Body::from(body_json_str))
                .unwrap(),
        )
        .await
        .unwrap();

    // tower-http RequestBodyLimitLayer returns 413 Payload Too Large
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
}
