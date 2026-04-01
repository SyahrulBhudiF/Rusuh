//! Tests for Zed auth parsing and credential handling.

use rusuh::auth::store::FileTokenStore;
use rusuh::auth::zed::{
    canonical_zed_login_filename, extract_zed_label, parse_zed_credential, zed_user_ids_match,
};
use serde_json::json;
use std::collections::HashMap;
use tempfile::TempDir;

#[test]
fn parses_valid_zed_credential() {
    let json = json!({
        "user_id": "test-user",
        "credential_json": "secret-json"
    });

    let result = parse_zed_credential(&json);
    assert!(result.is_ok());
    let (user_id, credential_json) = result.unwrap();
    assert_eq!(user_id, "test-user");
    assert_eq!(credential_json, "secret-json");
}

#[test]
fn test_parse_zed_credential_missing_user_id() {
    let json = json!({
        "credential_json": "{\"token\":\"secret\"}"
    });

    let result = parse_zed_credential(&json);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("user_id"));
}

#[test]
fn test_parse_zed_credential_missing_credential_json() {
    let json = json!({
        "user_id": "test-user-123"
    });

    let result = parse_zed_credential(&json);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("credential_json"));
}

#[test]
fn test_parse_zed_credential_wrong_types() {
    let json = json!({
        "user_id": 123,
        "credential_json": "{\"token\":\"secret\"}"
    });

    let result = parse_zed_credential(&json);
    assert!(result.is_err());
}

#[test]
fn test_extract_zed_label() {
    let user_id = "user@example.com";
    let credential_json = "{\"token\":\"secret\"}";

    let label = extract_zed_label(user_id, credential_json);
    assert_eq!(label, "user@example.com");
}

#[test]
fn test_canonical_zed_login_filename_simple() {
    let user_id = "user@example.com";
    let filename = canonical_zed_login_filename(user_id);
    assert_eq!(filename, "zed-login-user@example.com.json");
}

#[test]
fn canonical_filename_uses_user_id() {
    let user_id = "user.name+test@example.com";
    let filename = canonical_zed_login_filename(user_id);
    assert_eq!(filename, "zed-login-user.name+test@example.com.json");
}

#[test]
fn test_canonical_zed_login_filename_with_illegal_chars() {
    // Note: \\t becomes a tab character in the string literal
    let user_id = "user/name\test:file*name?<>|";
    let filename = canonical_zed_login_filename(user_id);
    // Path-illegal characters (/, :, *, ?, <, >, |) should be replaced with '-'
    // The tab character is not path-illegal so it remains
    assert_eq!(filename, "zed-login-user-name\test-file-name----.json");
}

#[test]
fn test_canonical_zed_login_filename_with_backslash() {
    // Test actual backslash character
    let user_id = r"user\name";
    let filename = canonical_zed_login_filename(user_id);
    assert_eq!(filename, "zed-login-user-name.json");
}

#[test]
#[cfg(target_os = "windows")]
fn test_zed_user_ids_match_windows_case_insensitive() {
    assert!(zed_user_ids_match("User@Example.Com", "user@example.com"));
    assert!(zed_user_ids_match("TEST", "test"));
    assert!(!zed_user_ids_match("user1", "user2"));
}

#[test]
#[cfg(not(target_os = "windows"))]
fn test_zed_user_ids_match_unix_case_sensitive() {
    assert!(!zed_user_ids_match("User@Example.Com", "user@example.com"));
    assert!(!zed_user_ids_match("TEST", "test"));
    assert!(zed_user_ids_match("user@example.com", "user@example.com"));
    assert!(!zed_user_ids_match("user1", "user2"));
}

#[tokio::test]
async fn test_store_extract_label_prefers_zed_user_id() {
    let temp_dir = TempDir::new().unwrap();
    let store = FileTokenStore::new(temp_dir.path());

    // Create a Zed auth file with user_id and credential_json
    let mut metadata = HashMap::new();
    metadata.insert("type".to_string(), json!("zed"));
    metadata.insert("user_id".to_string(), json!("zed-user@example.com"));
    metadata.insert(
        "credential_json".to_string(),
        json!("{\"token\":\"secret\"}"),
    );
    metadata.insert("email".to_string(), json!("other@example.com"));
    metadata.insert("label".to_string(), json!("other-label"));

    let auth_file = temp_dir.path().join("zed-login-test.json");
    let json_str = serde_json::to_string_pretty(&metadata).unwrap();
    tokio::fs::write(&auth_file, json_str).await.unwrap();

    // List and verify the label comes from user_id
    let records = store.list().await.unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].provider, "zed");
    assert_eq!(records[0].label, "zed-user@example.com");
}

#[tokio::test]
async fn test_store_extract_label_fallback_when_no_zed_fields() {
    let temp_dir = TempDir::new().unwrap();
    let store = FileTokenStore::new(temp_dir.path());

    // Create a Zed auth file without user_id/credential_json
    let mut metadata = HashMap::new();
    metadata.insert("type".to_string(), json!("zed"));
    metadata.insert("email".to_string(), json!("fallback@example.com"));

    let auth_file = temp_dir.path().join("zed-login-test2.json");
    let json_str = serde_json::to_string_pretty(&metadata).unwrap();
    tokio::fs::write(&auth_file, json_str).await.unwrap();

    // List and verify the label falls back to email
    let records = store.list().await.unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].provider, "zed");
    assert_eq!(records[0].label, "fallback@example.com");
}
