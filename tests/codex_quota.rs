use chrono::{DateTime, Utc};
use serde_json::json;

#[test]
fn test_parse_codex_retry_after_seconds_from_resets_in_seconds() {
    let now = Utc::now();
    let error_body = json!({
        "error": {
            "type": "usage_limit_reached",
            "resets_in_seconds": 3600
        }
    });

    let result = rusuh::auth::codex_runtime::parse_codex_retry_after_seconds(429, &error_body, now);
    assert_eq!(result, Some(3600));
}

#[test]
fn test_parse_codex_retry_after_seconds_prefers_resets_at() {
    let now = DateTime::parse_from_rfc3339("2026-03-28T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let resets_at = now.timestamp() + 7200; // 2 hours from now

    let error_body = json!({
        "error": {
            "type": "usage_limit_reached",
            "resets_at": resets_at,
            "resets_in_seconds": 3600
        }
    });

    let result = rusuh::auth::codex_runtime::parse_codex_retry_after_seconds(429, &error_body, now);
    assert_eq!(result, Some(7200));
}

#[test]
fn test_parse_codex_retry_after_seconds_ignores_non_429() {
    let now = Utc::now();
    let error_body = json!({
        "error": {
            "type": "usage_limit_reached",
            "resets_in_seconds": 3600
        }
    });

    let result = rusuh::auth::codex_runtime::parse_codex_retry_after_seconds(500, &error_body, now);
    assert_eq!(result, None);
}

#[test]
fn test_parse_codex_retry_after_seconds_ignores_non_usage_limit_reached() {
    let now = Utc::now();
    let error_body = json!({
        "error": {
            "type": "rate_limit_exceeded",
            "resets_in_seconds": 3600
        }
    });

    let result = rusuh::auth::codex_runtime::parse_codex_retry_after_seconds(429, &error_body, now);
    assert_eq!(result, None);
}

#[test]
fn test_parse_codex_retry_after_seconds_resets_at_in_past() {
    let now = DateTime::parse_from_rfc3339("2026-03-28T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let resets_at = now.timestamp() - 3600; // 1 hour ago

    let error_body = json!({
        "error": {
            "type": "usage_limit_reached",
            "resets_at": resets_at
        }
    });

    let result = rusuh::auth::codex_runtime::parse_codex_retry_after_seconds(429, &error_body, now);
    assert_eq!(result, None);
}

#[test]
fn test_parse_codex_retry_after_seconds_resets_at_exactly_now() {
    let now = DateTime::parse_from_rfc3339("2026-03-28T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let resets_at = now.timestamp();

    let error_body = json!({
        "error": {
            "type": "usage_limit_reached",
            "resets_at": resets_at
        }
    });

    let result = rusuh::auth::codex_runtime::parse_codex_retry_after_seconds(429, &error_body, now);
    assert_eq!(result, None);
}

#[test]
fn test_parse_codex_retry_after_seconds_missing_error_field() {
    let now = Utc::now();
    let error_body = json!({
        "message": "Rate limit exceeded"
    });

    let result = rusuh::auth::codex_runtime::parse_codex_retry_after_seconds(429, &error_body, now);
    assert_eq!(result, None);
}

#[test]
fn test_parse_codex_retry_after_seconds_missing_type_field() {
    let now = Utc::now();
    let error_body = json!({
        "error": {
            "resets_in_seconds": 3600
        }
    });

    let result = rusuh::auth::codex_runtime::parse_codex_retry_after_seconds(429, &error_body, now);
    assert_eq!(result, None);
}

#[test]
fn test_parse_codex_retry_after_seconds_no_reset_fields() {
    let now = Utc::now();
    let error_body = json!({
        "error": {
            "type": "usage_limit_reached"
        }
    });

    let result = rusuh::auth::codex_runtime::parse_codex_retry_after_seconds(429, &error_body, now);
    assert_eq!(result, None);
}
