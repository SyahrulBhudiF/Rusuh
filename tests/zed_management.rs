use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use http_body_util::BodyExt;
use rsa::pkcs1::DecodeRsaPublicKey;
use rsa::rand_core::OsRng;
use rsa::{Oaep, RsaPublicKey};
use serde_json::{json, Value};
use sha2::Sha256;
use tempfile::TempDir;
use tower::ServiceExt;
use uuid::Uuid;

use rusuh::auth::manager::AccountManager;
use rusuh::auth::zed::canonical_zed_login_filename;
use rusuh::config::{Config, ManagementConfig};
use rusuh::providers::model_registry::ModelRegistry;
use rusuh::proxy::ProxyState;
use rusuh::router::build_router;

const SECRET: &str = "test-zed-login-secret";

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

fn mgmt_post(uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("authorization", format!("Bearer {SECRET}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn mgmt_get(uri: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header("authorization", format!("Bearer {SECRET}"))
        .body(Body::empty())
        .unwrap()
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(json!(null))
}

fn encrypt_credential(public_key_b64: &str, plaintext: &str, padded: bool) -> String {
    let public_key_der = URL_SAFE_NO_PAD.decode(public_key_b64.as_bytes()).unwrap();
    let public_key = RsaPublicKey::from_pkcs1_der(&public_key_der).unwrap();
    let ciphertext = public_key
        .encrypt(&mut OsRng, Oaep::new::<Sha256>(), plaintext.as_bytes())
        .unwrap();
    if padded {
        URL_SAFE.encode(ciphertext)
    } else {
        URL_SAFE_NO_PAD.encode(ciphertext)
    }
}

async fn initiate_login(app: &axum::Router, name: Option<&str>) -> Value {
    let payload = match name {
        Some(name) => json!({ "name": name }),
        None => json!({}),
    };
    let resp = app
        .clone()
        .oneshot(mgmt_post("/v0/management/zed/login/initiate", payload))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    body_json(resp).await
}

async fn complete_callback(port: u64, user_id: &str, access_token: &str) {
    let response = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
        .get(format!(
            "http://127.0.0.1:{port}/?user_id={user_id}&access_token={access_token}"
        ))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_redirection());
}

#[tokio::test]
async fn login_initiate_returns_reachable_callback_port_and_zed_url() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap())).await;

    let body = initiate_login(&app, Some("work-zed")).await;
    let port = body["port"].as_u64().unwrap();
    let login_url = body["login_url"].as_str().unwrap();
    let session_id = body["session_id"].as_str().unwrap();
    let public_key = login_url.split("native_app_public_key=").nth(1).unwrap();

    assert_eq!(body["status"], "waiting");
    assert!(Uuid::parse_str(session_id).is_ok());
    assert!(login_url.starts_with("https://zed.dev/native_app_signin?"));
    assert!(login_url.contains(&format!("native_app_port={port}")));
    let public_key_der = URL_SAFE_NO_PAD.decode(public_key.as_bytes()).unwrap();
    assert!(RsaPublicKey::from_pkcs1_der(&public_key_der).is_ok());

    let response = reqwest::get(format!("http://127.0.0.1:{port}/"))
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn login_status_stays_waiting_until_callback_completion() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap())).await;

    let initiated = initiate_login(&app, None).await;
    let session_id = initiated["session_id"].as_str().unwrap();

    let resp = app
        .clone()
        .oneshot(mgmt_get(&format!(
            "/v0/management/zed/login/status?session_id={session_id}"
        )))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "waiting");
    assert_eq!(body["session_id"], session_id);
}

#[tokio::test]
async fn padded_callback_token_is_accepted() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap())).await;

    let initiated = initiate_login(&app, None).await;
    let session_id = initiated["session_id"].as_str().unwrap();
    let port = initiated["port"].as_u64().unwrap();
    let public_key = initiated["login_url"]
        .as_str()
        .unwrap()
        .split("native_app_public_key=")
        .nth(1)
        .unwrap();
    let encrypted = encrypt_credential(public_key, r#"{"token":"abc"}"#, true);

    complete_callback(port, "test-user", &encrypted).await;

    let resp = app
        .clone()
        .oneshot(mgmt_get(&format!(
            "/v0/management/zed/login/status?session_id={session_id}"
        )))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "completed");
    assert_eq!(body["user_id"], "test-user");
    assert_eq!(body["filename"], canonical_zed_login_filename("test-user"));
}

#[tokio::test]
async fn unpadded_callback_token_is_accepted() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap())).await;

    let initiated = initiate_login(&app, None).await;
    let session_id = initiated["session_id"].as_str().unwrap();
    let port = initiated["port"].as_u64().unwrap();
    let public_key = initiated["login_url"]
        .as_str()
        .unwrap()
        .split("native_app_public_key=")
        .nth(1)
        .unwrap();
    let encrypted = encrypt_credential(public_key, r#"{"token":"xyz"}"#, false);

    complete_callback(port, "another-user", &encrypted).await;

    let resp = app
        .clone()
        .oneshot(mgmt_get(&format!(
            "/v0/management/zed/login/status?session_id={session_id}"
        )))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "completed");
    assert_eq!(
        body["filename"],
        canonical_zed_login_filename("another-user")
    );
}

#[tokio::test]
async fn first_login_writes_canonical_filename() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap())).await;

    let initiated = initiate_login(&app, None).await;
    let session_id = initiated["session_id"].as_str().unwrap();
    let port = initiated["port"].as_u64().unwrap();
    let public_key = initiated["login_url"]
        .as_str()
        .unwrap()
        .split("native_app_public_key=")
        .nth(1)
        .unwrap();
    let encrypted = encrypt_credential(public_key, r#"{"token":"first"}"#, false);

    complete_callback(port, "first-user", &encrypted).await;

    let resp = app
        .clone()
        .oneshot(mgmt_get(&format!(
            "/v0/management/zed/login/status?session_id={session_id}"
        )))
        .await
        .unwrap();
    let body = body_json(resp).await;
    let filename = canonical_zed_login_filename("first-user");
    assert_eq!(body["filename"], filename);

    let content: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join(filename)).unwrap()).unwrap();
    assert_eq!(content["type"], "zed");
    assert_eq!(content["user_id"], "first-user");
    assert_eq!(content["credential_json"], r#"{"token":"first"}"#);
}

#[tokio::test]
async fn relogin_updates_existing_canonical_file_instead_of_creating_duplicate() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap())).await;

    let first = initiate_login(&app, None).await;
    let first_session_id = first["session_id"].as_str().unwrap();
    let first_port = first["port"].as_u64().unwrap();
    let first_public_key = first["login_url"]
        .as_str()
        .unwrap()
        .split("native_app_public_key=")
        .nth(1)
        .unwrap();
    let first_encrypted = encrypt_credential(first_public_key, r#"{"token":"old-token"}"#, false);

    complete_callback(first_port, "same-user", &first_encrypted).await;

    let first_resp = app
        .clone()
        .oneshot(mgmt_get(&format!(
            "/v0/management/zed/login/status?session_id={first_session_id}"
        )))
        .await
        .unwrap();
    let first_body = body_json(first_resp).await;
    let canonical = canonical_zed_login_filename("same-user");
    assert_eq!(first_body["filename"], canonical);

    let second = initiate_login(&app, None).await;
    let second_session_id = second["session_id"].as_str().unwrap();
    let second_port = second["port"].as_u64().unwrap();
    let second_public_key = second["login_url"]
        .as_str()
        .unwrap()
        .split("native_app_public_key=")
        .nth(1)
        .unwrap();
    let second_encrypted = encrypt_credential(second_public_key, r#"{"token":"new-token"}"#, false);

    complete_callback(second_port, "same-user", &second_encrypted).await;

    let second_resp = app
        .clone()
        .oneshot(mgmt_get(&format!(
            "/v0/management/zed/login/status?session_id={second_session_id}"
        )))
        .await
        .unwrap();
    let second_body = body_json(second_resp).await;
    assert_eq!(second_body["filename"], canonical);

    let entries = std::fs::read_dir(dir.path()).unwrap().count();
    assert_eq!(entries, 1);
    let content: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join(canonical)).unwrap())
            .unwrap();
    assert_eq!(content["credential_json"], r#"{"token":"new-token"}"#);
}

#[tokio::test]
async fn relogin_reuses_existing_legacy_filename_for_same_user() {
    let dir = TempDir::new().unwrap();
    let legacy_name = "my-custom-zed.json";
    std::fs::write(
        dir.path().join(legacy_name),
        serde_json::to_string_pretty(&json!({
            "type": "zed",
            "user_id": "legacy-user",
            "credential_json": "old-token"
        }))
        .unwrap(),
    )
    .unwrap();

    let app = test_app(mgmt_config(dir.path().to_str().unwrap())).await;
    let initiated = initiate_login(&app, None).await;
    let session_id = initiated["session_id"].as_str().unwrap();
    let port = initiated["port"].as_u64().unwrap();
    let public_key = initiated["login_url"]
        .as_str()
        .unwrap()
        .split("native_app_public_key=")
        .nth(1)
        .unwrap();
    let encrypted = encrypt_credential(public_key, r#"{"token":"updated-token"}"#, false);

    complete_callback(port, "legacy-user", &encrypted).await;

    let resp = app
        .clone()
        .oneshot(mgmt_get(&format!(
            "/v0/management/zed/login/status?session_id={session_id}"
        )))
        .await
        .unwrap();
    let body = body_json(resp).await;
    assert_eq!(body["filename"], legacy_name);
    assert!(!dir
        .path()
        .join(canonical_zed_login_filename("legacy-user"))
        .exists());

    let content: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join(legacy_name)).unwrap())
            .unwrap();
    assert_eq!(content["credential_json"], r#"{"token":"updated-token"}"#);
}

#[tokio::test]
async fn list_zed_models_returns_static_catalog_for_existing_auth_file() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("zed-user.json"),
        serde_json::to_string_pretty(&json!({
            "type": "zed",
            "user_id": "user-1",
            "credential_json": "{\"access_token\":\"token\"}"
        }))
        .unwrap(),
    )
    .unwrap();

    let app = test_app(mgmt_config(dir.path().to_str().unwrap())).await;

    let resp = app
        .clone()
        .oneshot(mgmt_post(
            "/v0/management/zed/models",
            json!({"name": "zed-user.json"}),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["account"], "zed-user.json");
    assert_eq!(body["provider_key"], "zed");
    assert_eq!(
        body["models"],
        json!([
            "claude-sonnet-4-6",
            "claude-sonnet-4-5",
            "claude-haiku-4-5",
            "gpt-5.4",
            "gpt-5.3-codex",
            "gpt-5.2",
            "gpt-5.2-codex",
            "gpt-5-mini",
            "gpt-5-nano",
            "gemini-3.1-pro-preview",
            "gemini-3-pro-preview",
            "gemini-3-flash"
        ])
    );
}

#[tokio::test]
async fn list_zed_models_returns_not_found_for_missing_auth_file() {
    let dir = TempDir::new().unwrap();
    let app = test_app(mgmt_config(dir.path().to_str().unwrap())).await;

    let resp = app
        .clone()
        .oneshot(mgmt_post(
            "/v0/management/zed/models",
            json!({"name": "zed-user.json"}),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_eq!(body["error"], "Zed auth file not found");
}
