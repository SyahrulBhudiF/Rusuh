//! Tests for `kiro_cooldown` module.

use std::time::{Duration, Instant};

use rusuh::auth::kiro_runtime::CooldownManager;

#[test]
fn not_in_cooldown_initially() {
    let mgr = CooldownManager::new();
    let now = Instant::now();
    assert!(!mgr.is_in_cooldown("auth1", "model-a", now));
}

#[test]
fn set_and_query_cooldown() {
    let mut mgr = CooldownManager::new();
    let now = Instant::now();
    mgr.set_cooldown(
        "auth1",
        "model-a",
        Duration::from_secs(60),
        "rate limited",
        now,
    );

    assert!(mgr.is_in_cooldown("auth1", "model-a", now));
    assert!(mgr.is_in_cooldown("auth1", "model-a", now + Duration::from_secs(30)));
}

#[test]
fn cooldown_expires() {
    let mut mgr = CooldownManager::new();
    let now = Instant::now();
    mgr.set_cooldown("auth1", "model-a", Duration::from_secs(10), "429", now);

    // After 11 seconds it should be expired
    assert!(!mgr.is_in_cooldown("auth1", "model-a", now + Duration::from_secs(11)));
}

#[test]
fn remaining_cooldown_returns_duration() {
    let mut mgr = CooldownManager::new();
    let now = Instant::now();
    mgr.set_cooldown("auth1", "model-a", Duration::from_secs(60), "429", now);

    let remaining = mgr.remaining_cooldown("auth1", "model-a", now + Duration::from_secs(10));
    assert!(remaining.is_some());
    // Should be approximately 50 seconds
    let r = remaining.unwrap();
    assert!(r <= Duration::from_secs(50) && r >= Duration::from_secs(49));
}

#[test]
fn remaining_cooldown_none_when_expired() {
    let mut mgr = CooldownManager::new();
    let now = Instant::now();
    mgr.set_cooldown("auth1", "model-a", Duration::from_secs(5), "429", now);

    assert!(mgr
        .remaining_cooldown("auth1", "model-a", now + Duration::from_secs(6))
        .is_none());
}

#[test]
fn cooldown_reason() {
    let mut mgr = CooldownManager::new();
    let now = Instant::now();
    mgr.set_cooldown(
        "auth1",
        "model-a",
        Duration::from_secs(60),
        "quota exhausted",
        now,
    );

    assert_eq!(
        mgr.cooldown_reason("auth1", "model-a", now),
        Some("quota exhausted")
    );
}

#[test]
fn cooldown_reason_none_when_expired() {
    let mut mgr = CooldownManager::new();
    let now = Instant::now();
    mgr.set_cooldown("auth1", "model-a", Duration::from_secs(1), "429", now);

    assert!(mgr
        .cooldown_reason("auth1", "model-a", now + Duration::from_secs(2))
        .is_none());
}

#[test]
fn clear_cooldown() {
    let mut mgr = CooldownManager::new();
    let now = Instant::now();
    mgr.set_cooldown("auth1", "model-a", Duration::from_secs(60), "429", now);
    assert!(mgr.is_in_cooldown("auth1", "model-a", now));

    mgr.clear_cooldown("auth1", "model-a");
    assert!(!mgr.is_in_cooldown("auth1", "model-a", now));
}

#[test]
fn calculate_cooldown_for_429_default() {
    let d = CooldownManager::calculate_cooldown_for_429(None);
    assert_eq!(d, Duration::from_secs(60));
}

#[test]
fn calculate_cooldown_for_429_with_retry_after() {
    let d = CooldownManager::calculate_cooldown_for_429(Some(30));
    assert_eq!(d, Duration::from_secs(30));
}

#[test]
fn calculate_cooldown_for_429_clamps_low() {
    let d = CooldownManager::calculate_cooldown_for_429(Some(1));
    assert_eq!(d, Duration::from_secs(5)); // MIN_COOLDOWN
}

#[test]
fn calculate_cooldown_for_429_clamps_high() {
    let d = CooldownManager::calculate_cooldown_for_429(Some(9999));
    assert_eq!(d, Duration::from_secs(300)); // LONG_COOLDOWN
}

#[test]
fn calculate_cooldown_for_quota_exceeded() {
    let d = CooldownManager::calculate_cooldown_for_quota_exceeded();
    assert_eq!(d, Duration::from_secs(300));
}

#[test]
fn different_keys_are_independent() {
    let mut mgr = CooldownManager::new();
    let now = Instant::now();
    mgr.set_cooldown("auth1", "model-a", Duration::from_secs(60), "429", now);

    assert!(!mgr.is_in_cooldown("auth2", "model-a", now));
    assert!(!mgr.is_in_cooldown("auth1", "model-b", now));
}

#[test]
fn purge_expired_removes_old_entries() {
    let mut mgr = CooldownManager::new();
    let now = Instant::now();
    mgr.set_cooldown("auth1", "model-a", Duration::from_secs(5), "old", now);
    mgr.set_cooldown("auth2", "model-b", Duration::from_secs(60), "current", now);

    mgr.purge_expired(now + Duration::from_secs(10));

    assert!(!mgr.is_in_cooldown("auth1", "model-a", now + Duration::from_secs(10)));
    assert!(mgr.is_in_cooldown("auth2", "model-b", now + Duration::from_secs(10)));
}
