//! Integration test: Kiro auth files produce registered KiroProviders at startup.

use tempfile::TempDir;

use rusuh::auth::manager::AccountManager;
use rusuh::config::Config;

#[tokio::test]
async fn kiro_auth_file_produces_registered_provider() {
    let dir = TempDir::new().unwrap();

    // Write a valid Kiro auth JSON file (canonical shape from kiro_record.rs)
    let auth_json = serde_json::json!({
        "type": "kiro",
        "provider_key": "kiro",
        "access_token": "test-access-token",
        "refresh_token": "test-refresh-token",
        "expires_at": "2030-01-01T00:00:00Z",
        "auth_method": "builder-id",
        "provider": "AWS",
        "region": "us-east-1",
        "client_id": "test-client-id",
        "client_secret": "test-client-secret"
    });

    std::fs::write(
        dir.path().join("kiro-test.json"),
        serde_json::to_string_pretty(&auth_json).unwrap(),
    )
    .unwrap();

    // Load accounts from disk
    let accounts = AccountManager::with_dir(dir.path());
    accounts.reload().await.unwrap();

    // Build providers — should include the kiro provider
    let config = Config::default();
    let registry = std::sync::Arc::new(rusuh::providers::model_registry::ModelRegistry::new());
    let providers = rusuh::providers::registry::build_providers(
        &config,
        &accounts,
        registry,
        rusuh::proxy::KiroRuntimeState::default(),
    )
    .await;

    assert!(
        providers.iter().any(|p| p.name() == "kiro"),
        "expected at least one provider with name 'kiro', got: {:?}",
        providers.iter().map(|p| p.name()).collect::<Vec<_>>()
    );
}
