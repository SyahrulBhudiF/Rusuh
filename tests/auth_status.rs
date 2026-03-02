//! Tests for AuthStatus enum and AuthRecord status fields.

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::Utc;
use serde_json::json;

use rusuh::auth::store::{AuthRecord, AuthStatus};

fn make_record(status: AuthStatus, disabled: bool) -> AuthRecord {
    AuthRecord {
        id: "test.json".into(),
        provider: "antigravity".into(),
        label: "test@example.com".into(),
        disabled,
        status,
        status_message: None,
        last_refreshed_at: None,
        path: PathBuf::from("test.json"),
        metadata: HashMap::new(),
        updated_at: Utc::now(),
    }
}

// ── AuthStatus ───────────────────────────────────────────────────────────────

#[test]
fn status_default_is_active() {
    assert_eq!(AuthStatus::default(), AuthStatus::Active);
}

#[test]
fn status_display() {
    assert_eq!(AuthStatus::Active.to_string(), "active");
    assert_eq!(AuthStatus::Disabled.to_string(), "disabled");
    assert_eq!(AuthStatus::Error.to_string(), "error");
    assert_eq!(AuthStatus::Pending.to_string(), "pending");
    assert_eq!(AuthStatus::Refreshing.to_string(), "refreshing");
    assert_eq!(AuthStatus::Unknown.to_string(), "unknown");
}

#[test]
fn status_from_str_loose() {
    assert_eq!(AuthStatus::from_str_loose("active"), AuthStatus::Active);
    assert_eq!(AuthStatus::from_str_loose("DISABLED"), AuthStatus::Disabled);
    assert_eq!(AuthStatus::from_str_loose(" Error "), AuthStatus::Error);
    assert_eq!(AuthStatus::from_str_loose("bogus"), AuthStatus::Unknown);
    assert_eq!(AuthStatus::from_str_loose(""), AuthStatus::Unknown);
}

#[test]
fn status_is_usable() {
    assert!(AuthStatus::Active.is_usable());
    assert!(AuthStatus::Refreshing.is_usable());
    assert!(!AuthStatus::Disabled.is_usable());
    assert!(!AuthStatus::Error.is_usable());
    assert!(!AuthStatus::Pending.is_usable());
    assert!(!AuthStatus::Unknown.is_usable());
}

#[test]
fn status_serde_roundtrip() {
    let json = serde_json::to_string(&AuthStatus::Active).unwrap();
    assert_eq!(json, "\"active\"");
    let parsed: AuthStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, AuthStatus::Active);
}

// ── AuthRecord effective_status ──────────────────────────────────────────────

#[test]
fn effective_status_uses_status_when_not_disabled() {
    let record = make_record(AuthStatus::Active, false);
    assert_eq!(record.effective_status(), AuthStatus::Active);
}

#[test]
fn effective_status_overrides_to_disabled() {
    let record = make_record(AuthStatus::Active, true);
    assert_eq!(record.effective_status(), AuthStatus::Disabled);
}

#[test]
fn effective_status_disabled_flag_wins_over_error() {
    let record = make_record(AuthStatus::Error, true);
    assert_eq!(record.effective_status(), AuthStatus::Disabled);
}

// ── AuthRecord metadata helpers ──────────────────────────────────────────────

#[test]
fn record_access_token_from_top_level() {
    let mut meta = HashMap::new();
    meta.insert("access_token".into(), json!("tok123"));
    let record = AuthRecord {
        metadata: meta,
        ..make_record(AuthStatus::Active, false)
    };
    assert_eq!(record.access_token(), Some("tok123"));
}

#[test]
fn record_access_token_from_nested_token() {
    let mut meta = HashMap::new();
    meta.insert("token".into(), json!({"access_token": "nested_tok"}));
    let record = AuthRecord {
        metadata: meta,
        ..make_record(AuthStatus::Active, false)
    };
    assert_eq!(record.access_token(), Some("nested_tok"));
}

#[test]
fn record_project_id() {
    let mut meta = HashMap::new();
    meta.insert("project_id".into(), json!("my-project-123"));
    let record = AuthRecord {
        metadata: meta,
        ..make_record(AuthStatus::Active, false)
    };
    assert_eq!(record.project_id(), Some("my-project-123"));
}

#[test]
fn record_email() {
    let mut meta = HashMap::new();
    meta.insert("email".into(), json!("user@example.com"));
    let record = AuthRecord {
        metadata: meta,
        ..make_record(AuthStatus::Active, false)
    };
    assert_eq!(record.email(), Some("user@example.com"));
}

#[test]
fn record_empty_metadata_returns_none() {
    let record = make_record(AuthStatus::Active, false);
    assert_eq!(record.access_token(), None);
    assert_eq!(record.email(), None);
    assert_eq!(record.project_id(), None);
}
