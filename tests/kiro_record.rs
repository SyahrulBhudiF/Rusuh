use std::collections::HashMap;

use chrono::DateTime;
use rusuh::auth::kiro::{parse_expiry, KiroTokenData, KiroTokenSource};
use rusuh::auth::kiro_record::{build_kiro_metadata, KIRO_PROVIDER_KEY};
use serde_json::{json, Value};

fn sample_token_data() -> KiroTokenData {
    KiroTokenData {
        access_token: "access".into(),
        refresh_token: "refresh".into(),
        profile_arn: String::new(),
        expires_at: "2026-01-01T00:00:00Z".into(),
        auth_method: "builder-id".into(),
        provider: "AWS Builder ID".into(),
        client_id: Some("client-id".into()),
        client_secret: Some("client-secret".into()),
        region: "us-east-1".into(),
        start_url: Some("https://view.awsapps.com/start".into()),
        email: Some("user@example.com".into()),
    }
}

#[test]
fn builds_canonical_metadata_with_provider_key() {
    let metadata = build_kiro_metadata(
        &sample_token_data(),
        &Some("Example".into()),
        KiroTokenSource::BuilderIdWeb,
    );

    assert_eq!(
        metadata.get("type").and_then(Value::as_str),
        Some(KIRO_PROVIDER_KEY)
    );
    assert_eq!(
        metadata.get("provider_key").and_then(Value::as_str),
        Some(KIRO_PROVIDER_KEY)
    );
    assert_eq!(
        metadata.get("auth_method").and_then(Value::as_str),
        Some("builder-id")
    );
    assert_eq!(
        metadata.get("label").and_then(Value::as_str),
        Some("Example")
    );
    assert_eq!(
        metadata.get("source").and_then(Value::as_str),
        Some("builder-id-web")
    );
}

#[test]
fn omits_empty_optional_fields() {
    let mut token_data = sample_token_data();
    token_data.client_id = Some(String::new());
    token_data.client_secret = Some(String::new());
    token_data.start_url = Some(String::new());
    token_data.email = Some(String::new());

    let metadata = build_kiro_metadata(&token_data, &None, KiroTokenSource::Import);

    assert!(!metadata.contains_key("client_id"));
    assert!(!metadata.contains_key("client_secret"));
    assert!(!metadata.contains_key("start_url"));
    assert!(!metadata.contains_key("email"));
    assert!(!metadata.contains_key("label"));
}

#[test]
fn legacy_social_records_keep_social_metadata_shape() {
    let mut token_data = sample_token_data();
    token_data.auth_method = "social".into();
    token_data.provider = "imported".into();
    token_data.client_id = None;
    token_data.client_secret = None;
    token_data.start_url = None;

    let metadata = build_kiro_metadata(
        &token_data,
        &Some("Imported social token".into()),
        KiroTokenSource::LegacySocial,
    );

    assert_eq!(metadata.get("type").and_then(Value::as_str), Some("kiro"));
    assert_eq!(
        metadata.get("provider_key").and_then(Value::as_str),
        Some("kiro")
    );
    assert_eq!(
        metadata.get("auth_method").and_then(Value::as_str),
        Some("social")
    );
    assert_eq!(
        metadata.get("provider").and_then(Value::as_str),
        Some("imported")
    );
    assert_eq!(
        metadata.get("source").and_then(Value::as_str),
        Some("legacy-social")
    );
    assert!(!metadata.contains_key("client_id"));
    assert!(!metadata.contains_key("client_secret"));
    assert!(!metadata.contains_key("start_url"));
}

#[test]
fn parse_expiry_returns_unix_epoch_when_missing() {
    let metadata = HashMap::new();

    assert_eq!(parse_expiry(&metadata), DateTime::UNIX_EPOCH);
}

#[test]
fn parse_expiry_returns_unix_epoch_when_invalid() {
    let mut metadata = HashMap::new();
    metadata.insert("expires_at".to_string(), json!("not-a-date"));

    assert_eq!(parse_expiry(&metadata), DateTime::UNIX_EPOCH);
}

#[test]
fn parse_expiry_uses_valid_rfc3339_value() {
    let mut metadata = HashMap::new();
    metadata.insert("expires_at".to_string(), json!("2026-05-01T12:34:56Z"));

    assert_eq!(
        parse_expiry(&metadata),
        DateTime::parse_from_rfc3339("2026-05-01T12:34:56Z")
            .unwrap()
            .with_timezone(&chrono::Utc)
    );
}
