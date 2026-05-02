//! Tests for Zed provider registration

use rusuh::auth::store::FileTokenStore;
use rusuh::providers::zed::scan_zed_providers;
use serde_json::json;
use std::fs;
use tempfile::TempDir;

#[tokio::test]
async fn test_scan_zed_providers_empty_dir() {
    let temp_dir = TempDir::new().unwrap();
    let store = FileTokenStore::new(temp_dir.path());

    let providers = scan_zed_providers(&store).await.unwrap();
    assert_eq!(providers.len(), 0);
}

#[tokio::test]
async fn test_scan_zed_providers_single_account() {
    let temp_dir = TempDir::new().unwrap();
    let store = FileTokenStore::new(temp_dir.path());

    // Create a zed auth file with correct structure
    let auth_file = temp_dir.path().join("zed-login-user1.json");
    let auth_data = json!({
        "type": "zed",
        "user_id": "user1",
        "credential_json": "{\"token\":\"abc123\"}"
    });
    fs::write(
        &auth_file,
        serde_json::to_string_pretty(&auth_data).unwrap(),
    )
    .unwrap();

    let providers = scan_zed_providers(&store).await.unwrap();
    assert_eq!(providers.len(), 1);

    let (filename, provider) = &providers[0];
    assert_eq!(filename, "zed-login-user1.json");
    assert_eq!(provider.user_id(), "user1");
    assert_eq!(provider.credential_json(), "{\"token\":\"abc123\"}");
}

#[tokio::test]
async fn test_scan_zed_providers_multiple_accounts() {
    let temp_dir = TempDir::new().unwrap();
    let store = FileTokenStore::new(temp_dir.path());

    // Create first zed auth file
    let auth_file1 = temp_dir.path().join("zed-login-user1.json");
    let auth_data1 = json!({
        "type": "zed",
        "user_id": "user1",
        "credential_json": "{\"token\":\"abc123\"}"
    });
    fs::write(
        &auth_file1,
        serde_json::to_string_pretty(&auth_data1).unwrap(),
    )
    .unwrap();

    // Create second zed auth file
    let auth_file2 = temp_dir.path().join("zed-login-user2.json");
    let auth_data2 = json!({
        "type": "zed",
        "user_id": "user2",
        "credential_json": "{\"token\":\"xyz789\"}"
    });
    fs::write(
        &auth_file2,
        serde_json::to_string_pretty(&auth_data2).unwrap(),
    )
    .unwrap();

    let providers = scan_zed_providers(&store).await.unwrap();
    assert_eq!(providers.len(), 2);

    // Verify both providers are present
    let user_ids: Vec<String> = providers
        .iter()
        .map(|(_, p)| p.user_id().to_string())
        .collect();
    assert!(user_ids.contains(&"user1".to_string()));
    assert!(user_ids.contains(&"user2".to_string()));
}

#[tokio::test]
async fn test_scan_zed_providers_ignores_non_zed() {
    let temp_dir = TempDir::new().unwrap();
    let store = FileTokenStore::new(temp_dir.path());

    // Create a zed auth file
    let zed_file = temp_dir.path().join("zed-login-user1.json");
    let zed_data = json!({
        "type": "zed",
        "user_id": "user1",
        "credential_json": "{\"token\":\"abc123\"}"
    });
    fs::write(&zed_file, serde_json::to_string_pretty(&zed_data).unwrap()).unwrap();

    // Create a kiro auth file (should be ignored)
    let kiro_file = temp_dir.path().join("kiro-builder-id.json");
    let kiro_data = json!({
        "type": "kiro",
        "auth_method": "builder_id"
    });
    fs::write(
        &kiro_file,
        serde_json::to_string_pretty(&kiro_data).unwrap(),
    )
    .unwrap();

    let providers = scan_zed_providers(&store).await.unwrap();
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].1.user_id(), "user1");
}

#[tokio::test]
async fn test_scan_zed_providers_skips_invalid() {
    let temp_dir = TempDir::new().unwrap();
    let store = FileTokenStore::new(temp_dir.path());

    // Create a valid zed auth file
    let valid_file = temp_dir.path().join("zed-login-valid.json");
    let valid_data = json!({
        "type": "zed",
        "user_id": "valid_user",
        "credential_json": "{\"token\":\"abc123\"}"
    });
    fs::write(
        &valid_file,
        serde_json::to_string_pretty(&valid_data).unwrap(),
    )
    .unwrap();

    // Create an invalid zed auth file (missing credential_json)
    let invalid_file = temp_dir.path().join("zed-login-invalid.json");
    let invalid_data = json!({
        "type": "zed",
        "user_id": "invalid_user"
    });
    fs::write(
        &invalid_file,
        serde_json::to_string_pretty(&invalid_data).unwrap(),
    )
    .unwrap();

    let providers = scan_zed_providers(&store).await.unwrap();
    // Should only get the valid one
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].1.user_id(), "valid_user");
}
