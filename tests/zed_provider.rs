//! Tests for ZedProvider runtime behavior

use rusuh::auth::store::{AuthRecord, AuthStatus};
use rusuh::providers::zed::{TokenCache, ZedProvider};
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use chrono::Utc;

fn make_zed_record(user_id: &str, credential_json: &str) -> AuthRecord {
    let mut metadata = HashMap::new();
    metadata.insert("user_id".to_string(), json!(user_id));
    metadata.insert("credential_json".to_string(), json!(credential_json));

    AuthRecord {
        id: "test.json".into(),
        provider: "zed".into(),
        provider_key: "zed".into(),
        label: user_id.to_string(),
        disabled: false,
        status: AuthStatus::Active,
        status_message: None,
        last_refreshed_at: None,
        path: PathBuf::from("test.json"),
        metadata,
        updated_at: Utc::now(),
    }
}

#[test]
fn test_token_cache_new() {
    let cache = TokenCache::new("test-token".to_string(), 3600);
    assert_eq!(cache.token(), "test-token");
    assert!(!cache.is_expired());
}

#[test]
fn test_token_cache_expired_with_buffer() {
    // Create a cache that expires in 30 seconds (less than 60-second buffer)
    let cache = TokenCache::new("test-token".to_string(), 30);
    // Should be considered expired due to 60-second refresh buffer
    assert!(cache.is_expired());
}

#[test]
fn test_token_cache_not_expired_with_buffer() {
    // Create a cache that expires in 120 seconds (more than 60-second buffer)
    let cache = TokenCache::new("test-token".to_string(), 120);
    // Should not be expired (120 - 60 = 60 seconds remaining)
    assert!(!cache.is_expired());
}

#[test]
fn test_token_cache_exact_buffer_boundary() {
    // Create a cache that expires in exactly 60 seconds
    let cache = TokenCache::new("test-token".to_string(), 60);
    // Should be considered expired at the buffer boundary
    assert!(cache.is_expired());
}

#[test]
fn test_token_cache_already_expired() {
    // Create a cache with past expiry
    let expires_at = SystemTime::now() - Duration::from_secs(10);
    let cache = TokenCache {
        token: "test-token".to_string(),
        expires_at,
    };
    assert!(cache.is_expired());
}

#[test]
fn test_zed_provider_new_from_auth_record() {
    let record = make_zed_record("user123", "{\"key\":\"value\"}");

    let provider = ZedProvider::new(record).unwrap();
    assert_eq!(provider.user_id(), "user123");
    assert_eq!(provider.credential_json(), "{\"key\":\"value\"}");
}

#[test]
fn test_zed_provider_new_missing_user_id() {
    let mut metadata = HashMap::new();
    metadata.insert("credential_json".to_string(), json!("{\"key\":\"value\"}"));

    let record = AuthRecord {
        id: "test.json".into(),
        provider: "zed".into(),
        provider_key: "zed".into(),
        label: "test".to_string(),
        disabled: false,
        status: AuthStatus::Active,
        status_message: None,
        last_refreshed_at: None,
        path: PathBuf::from("test.json"),
        metadata,
        updated_at: Utc::now(),
    };

    let result = ZedProvider::new(record);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("user_id"));
}

#[test]
fn test_zed_provider_new_missing_credential_json() {
    let mut metadata = HashMap::new();
    metadata.insert("user_id".to_string(), json!("user123"));

    let record = AuthRecord {
        id: "test.json".into(),
        provider: "zed".into(),
        provider_key: "zed".into(),
        label: "test".to_string(),
        disabled: false,
        status: AuthStatus::Active,
        status_message: None,
        last_refreshed_at: None,
        path: PathBuf::from("test.json"),
        metadata,
        updated_at: Utc::now(),
    };

    let result = ZedProvider::new(record);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("credential_json"));
}

#[test]
fn test_zed_provider_token_cache_initially_empty() {
    let record = make_zed_record("user123", "{\"key\":\"value\"}");
    let provider = ZedProvider::new(record).unwrap();

    // Access the token cache to verify it's initially None
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let cache = provider.token_cache.lock().await;
        assert!(cache.is_none());
    });
}

#[test]
fn test_zed_provider_models_cache_initially_empty() {
    let record = make_zed_record("user123", "{\"key\":\"value\"}");
    let provider = ZedProvider::new(record).unwrap();

    // Access the models cache to verify it's initially None
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let cache = provider.models_cache.lock().await;
        assert!(cache.is_none());
    });
}

#[tokio::test]
async fn test_zed_provider_build_headers() {
    let record = make_zed_record("user123", "{\"key\":\"value\"}");
    let provider = ZedProvider::new(record).unwrap();

    // Set a token in the cache
    {
        let mut cache = provider.token_cache.lock().await;
        *cache = Some(TokenCache::new("test-bearer-token".to_string(), 3600));
    }

    let headers = provider.build_headers().await.unwrap();

    assert_eq!(headers.get("authorization").unwrap(), "Bearer test-bearer-token");
    assert_eq!(headers.get("content-type").unwrap(), "application/json");
    assert_eq!(headers.get("x-zed-version").unwrap(), "0.222.4");
}

#[tokio::test]
async fn test_zed_provider_build_headers_no_token() {
    let record = make_zed_record("user123", "{\"key\":\"value\"}");
    let provider = ZedProvider::new(record).unwrap();

    // Don't set a token - should error
    let result = provider.build_headers().await;
    assert!(result.is_err());
}
