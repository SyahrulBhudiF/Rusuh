//! Tests for Antigravity provider helpers: parse_expiry, int64_value, TokenState::needs_refresh.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use serde_json::json;

use rusuh::providers::antigravity::{
    int64_value, parse_expiry, TokenState, REFRESH_SKEW_SECS,
};

// ── parse_expiry ─────────────────────────────────────────────────────────────

#[test]
fn parse_expiry_from_expired_rfc3339() {
    let mut meta = HashMap::new();
    meta.insert("expired".into(), json!("2025-03-02T14:00:00Z"));
    let exp = parse_expiry(&meta);
    assert_eq!(
        exp,
        DateTime::parse_from_rfc3339("2025-03-02T14:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    );
}

#[test]
fn parse_expiry_from_timestamp_plus_expires_in() {
    let mut meta = HashMap::new();
    meta.insert("timestamp".into(), json!(1709388000000_i64));
    meta.insert("expires_in".into(), json!(3599));
    let exp = parse_expiry(&meta);
    let expected =
        DateTime::from_timestamp_millis(1709388000000).unwrap() + Duration::seconds(3599);
    assert_eq!(exp, expected);
}

#[test]
fn parse_expiry_from_expires_at_unix() {
    let mut meta = HashMap::new();
    meta.insert("expires_at".into(), json!(1709391599));
    let exp = parse_expiry(&meta);
    assert_eq!(exp, DateTime::from_timestamp(1709391599, 0).unwrap());
}

#[test]
fn parse_expiry_fallback_to_epoch() {
    let meta = HashMap::new();
    let exp = parse_expiry(&meta);
    assert_eq!(exp, DateTime::UNIX_EPOCH);
}

#[test]
fn parse_expiry_rfc3339_takes_priority() {
    let mut meta = HashMap::new();
    meta.insert("expired".into(), json!("2030-01-01T00:00:00Z"));
    meta.insert("timestamp".into(), json!(1000000000000_i64));
    meta.insert("expires_in".into(), json!(3599));
    let exp = parse_expiry(&meta);
    assert_eq!(
        exp,
        DateTime::parse_from_rfc3339("2030-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    );
}

// ── TokenState::needs_refresh ────────────────────────────────────────────────

#[test]
fn needs_refresh_empty_token() {
    let state = TokenState {
        access_token: String::new(),
        refresh_token: "refresh".into(),
        expires_at: Utc::now() + Duration::hours(2),
        last_refreshed_at: None,
    };
    assert!(state.needs_refresh());
}

#[test]
fn needs_refresh_expired_token() {
    let state = TokenState {
        access_token: "token".into(),
        refresh_token: "refresh".into(),
        expires_at: Utc::now() - Duration::seconds(1),
        last_refreshed_at: None,
    };
    assert!(state.needs_refresh());
}

#[test]
fn needs_refresh_within_skew() {
    let state = TokenState {
        access_token: "token".into(),
        refresh_token: "refresh".into(),
        // 40 min < 50 min skew → should refresh
        expires_at: Utc::now() + Duration::minutes(40),
        last_refreshed_at: None,
    };
    assert!(state.needs_refresh());
}

#[test]
fn no_refresh_when_far_from_expiry() {
    let state = TokenState {
        access_token: "token".into(),
        refresh_token: "refresh".into(),
        // 2 hours > 50 min skew → no refresh
        expires_at: Utc::now() + Duration::hours(2),
        last_refreshed_at: None,
    };
    assert!(!state.needs_refresh());
}

#[test]
fn refresh_skew_is_50_minutes() {
    assert_eq!(REFRESH_SKEW_SECS, 3000);
}

// ── int64_value ──────────────────────────────────────────────────────────────

#[test]
fn int64_value_from_number() {
    assert_eq!(int64_value(&json!(42)), Some(42));
    assert_eq!(int64_value(&json!(-1)), Some(-1));
}

#[test]
fn int64_value_from_string() {
    assert_eq!(int64_value(&json!("12345")), Some(12345));
    assert_eq!(int64_value(&json!("not_a_number")), None);
}

#[test]
fn int64_value_from_other() {
    assert_eq!(int64_value(&json!(true)), None);
    assert_eq!(int64_value(&json!(null)), None);
}
