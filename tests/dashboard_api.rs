use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tempfile::TempDir;
use tower::ServiceExt;

use rusuh::auth::manager::AccountManager;
use rusuh::config::Config;
use rusuh::providers::model_registry::ModelRegistry;
use rusuh::proxy::ProxyState;
use rusuh::router::build_router;

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

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("collect response body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("decode json body")
}

#[tokio::test]
async fn dashboard_accounts_and_overview_include_codex_auth_files() {
    let dir = TempDir::new().expect("create temp dir");

    let codex_auth = json!({
        "type": "codex",
        "provider_key": "codex",
        "email": "codex@example.com",
        "access_token": "access-token",
        "refresh_token": "refresh-token",
        "id_token": "id-token",
        "status": "active",
        "disabled": false
    });
    std::fs::write(
        dir.path().join("codex-user.json"),
        serde_json::to_string_pretty(&codex_auth).expect("serialize codex auth file"),
    )
    .expect("write codex auth file");

    let app = test_app(Config {
        auth_dir: dir.path().to_string_lossy().to_string(),
        api_keys: vec!["rsk-test".to_string()],
        ..Default::default()
    });

    let accounts_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/dashboard/accounts")
                .body(Body::empty())
                .expect("build accounts request"),
        )
        .await
        .expect("call /dashboard/accounts");

    assert_eq!(accounts_resp.status(), StatusCode::OK);
    let accounts_body = body_json(accounts_resp).await;

    assert_eq!(accounts_body["total"], 1);
    assert_eq!(accounts_body["items"][0]["provider"], "codex");
    assert_eq!(accounts_body["items"][0]["email"], "codex@example.com");

    let codex_group = accounts_body["grouped_counts"]
        .as_array()
        .expect("grouped_counts should be an array")
        .iter()
        .find(|entry| entry["provider"] == "codex")
        .expect("codex provider group should exist");
    assert_eq!(codex_group["total"], 1);
    assert_eq!(codex_group["active"], 1);

    let overview_resp = app
        .oneshot(
            Request::builder()
                .uri("/dashboard/overview")
                .body(Body::empty())
                .expect("build overview request"),
        )
        .await
        .expect("call /dashboard/overview");

    assert_eq!(overview_resp.status(), StatusCode::OK);
    let overview_body = body_json(overview_resp).await;

    let overview_codex_group = overview_body["account_summaries"]
        .as_array()
        .expect("account_summaries should be an array")
        .iter()
        .find(|entry| entry["provider"] == "codex")
        .expect("overview should include codex account summary");
    assert_eq!(overview_codex_group["total"], 1);
    assert_eq!(overview_codex_group["active"], 1);
}

#[tokio::test]
async fn dashboard_config_exposes_codex_api_key_entry_summary() {
    let app = test_app(Config {
        codex_api_keys: vec![rusuh::config::ProviderKeyEntry {
            api_key: "codex-key".to_string(),
            prefix: Some("team-codex".to_string()),
            base_url: Some("https://codex.example.internal/v1".to_string()),
            models: vec![],
            excluded_models: vec!["gpt-5-codex".to_string()],
            proxy_url: Some("http://proxy.internal:8080".to_string()),
            headers: std::collections::HashMap::from([(
                "x-trace-id".to_string(),
                "trace-1".to_string(),
            )]),
        }],
        ..Default::default()
    });

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/dashboard/config")
                .body(Body::empty())
                .expect("build config request"),
        )
        .await
        .expect("call /dashboard/config");

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;

    let codex_entries = body["codex_api_keys"]
        .as_array()
        .expect("codex_api_keys should be an array");
    assert_eq!(codex_entries.len(), 1);

    let entry = &codex_entries[0];
    assert_eq!(entry["prefix"], "team-codex");
    assert_eq!(entry["base_url"], "https://codex.example.internal/v1");
    assert_eq!(entry["excluded_model_count"], 1);
    assert_eq!(entry["has_proxy_url"], true);
    assert_eq!(entry["header_count"], 1);
    assert_eq!(entry["has_api_key"], true);
}
