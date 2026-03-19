use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tempfile::TempDir;
use tower::ServiceExt;

use rusuh::auth::kiro_runtime::{QuotaChecker, QuotaStatus, UsageCheckRequest};
use rusuh::auth::manager::AccountManager;
use rusuh::config::{Config, ManagementConfig};
use rusuh::providers::model_registry::ModelRegistry;
use rusuh::proxy::ProxyState;
use rusuh::router::build_router;

const SECRET: &str = "test-mgmt-secret";

#[derive(Debug)]
struct FixedQuotaChecker {
    status: QuotaStatus,
}

#[async_trait::async_trait]
impl QuotaChecker for FixedQuotaChecker {
    async fn check_quota(&self, _request: &UsageCheckRequest) -> QuotaStatus {
        self.status.clone()
    }
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

async fn test_app_with_quota_checker(
    cfg: Config,
    quota_checker: Arc<dyn QuotaChecker>,
) -> axum::Router {
    let auth_dir = cfg.auth_dir.clone();
    let accounts = Arc::new(AccountManager::with_dir(auth_dir));
    accounts.reload().await.unwrap();
    let registry = Arc::new(ModelRegistry::new());
    let mut state = ProxyState::new(cfg, accounts, registry, 0);
    state.kiro_runtime.quota_checker = quota_checker;
    let state = Arc::new(state);

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

#[tokio::test]
async fn check_kiro_quota_uses_runtime_checker_status() {
    let dir = TempDir::new().unwrap();
    let auth = json!({
        "type": "kiro",
        "provider_key": "kiro",
        "access_token": "test-access-token",
        "refresh_token": "test-refresh-token",
        "profile_arn": "arn:aws:iam::123456789012:role/test",
        "expires_at": "2030-01-01T00:00:00Z",
        "auth_method": "import",
        "provider": "AWS",
        "client_id": "test-client-id",
        "client_secret": "test-client-secret"
    });
    std::fs::write(
        dir.path().join("kiro-test.json"),
        serde_json::to_string_pretty(&auth).unwrap(),
    )
    .unwrap();

    let checker = Arc::new(FixedQuotaChecker {
        status: QuotaStatus::Available {
            remaining: Some(42),
            next_reset: Some(1710777600000),
            breakdown: None,
        },
    });
    let app = test_app_with_quota_checker(mgmt_config(dir.path().to_str().unwrap()), checker).await;

    let resp = app
        .oneshot(mgmt_request_json(
            "POST",
            "/v0/management/kiro/check-quota",
            json!({"name": "kiro-test.json"}),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "available");
    assert_eq!(body["remaining"], 42);
    assert_eq!(body["next_reset"], 1710777600000i64);
}
