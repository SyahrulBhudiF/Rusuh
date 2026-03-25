//! Tests for Kiro provider outcome classification and cooldown helpers.

use std::sync::Arc;
use std::time::{Duration, Instant};

use rusuh::auth::kiro_runtime::{KiroRateLimiter, QuotaStatus, UsageCheckRequest};
use rusuh::auth::manager::AccountManager;
use rusuh::config::Config;
use rusuh::providers::kiro_outcome::{
    body_indicates_suspension, body_indicates_token_error, body_matches_suspend_keywords,
    calculate_429_cooldown, classify_kiro_response, cooldown_for_outcome,
    cooldown_reason_for_outcome, registry_action_for_outcome, KiroRequestOutcome, RegistryAction,
    COOLDOWN_REASON_429, COOLDOWN_REASON_QUOTA, COOLDOWN_REASON_SUSPENDED,
};
use rusuh::providers::model_registry::ModelRegistry;
use rusuh::proxy::ProxyState;

// ── classify_kiro_response tests ─────────────────────────────────────────────

#[test]
fn classify_200_is_success() {
    assert_eq!(
        classify_kiro_response(200, "", 0),
        KiroRequestOutcome::Success
    );
}

#[test]
fn classify_201_is_success() {
    assert_eq!(
        classify_kiro_response(201, "", 0),
        KiroRequestOutcome::Success
    );
}

#[test]
fn classify_400_is_validation_error() {
    assert_eq!(
        classify_kiro_response(400, "bad request", 0),
        KiroRequestOutcome::ValidationError
    );
}

#[test]
fn classify_401_is_token_expired() {
    assert_eq!(
        classify_kiro_response(401, "", 0),
        KiroRequestOutcome::TokenExpired
    );
}

#[test]
fn classify_402_is_quota_exhausted() {
    assert_eq!(
        classify_kiro_response(402, "monthly limit reached", 0),
        KiroRequestOutcome::QuotaExhausted
    );
}

#[test]
fn classify_429_is_rate_limited() {
    assert_eq!(
        classify_kiro_response(429, "", 3),
        KiroRequestOutcome::RateLimited { retry_count: 3 }
    );
}

#[test]
fn classify_403_suspended() {
    assert_eq!(
        classify_kiro_response(403, "your account is SUSPENDED", 0),
        KiroRequestOutcome::Suspended
    );
}

#[test]
fn classify_403_temporarily_suspended() {
    assert_eq!(
        classify_kiro_response(403, "status: TEMPORARILY_SUSPENDED", 0),
        KiroRequestOutcome::Suspended
    );
}

#[test]
fn classify_403_token_error() {
    assert_eq!(
        classify_kiro_response(403, "access token has expired", 0),
        KiroRequestOutcome::TokenExpired
    );
}

#[test]
fn classify_403_generic() {
    assert_eq!(
        classify_kiro_response(403, "forbidden", 0),
        KiroRequestOutcome::NonRetryableError
    );
}

#[test]
fn classify_502_is_retryable() {
    assert_eq!(
        classify_kiro_response(502, "", 0),
        KiroRequestOutcome::RetryableServerError
    );
}

#[test]
fn classify_503_is_retryable() {
    assert_eq!(
        classify_kiro_response(503, "", 0),
        KiroRequestOutcome::RetryableServerError
    );
}

#[test]
fn classify_504_is_retryable() {
    assert_eq!(
        classify_kiro_response(504, "", 0),
        KiroRequestOutcome::RetryableServerError
    );
}

#[test]
fn classify_500_is_non_retryable() {
    assert_eq!(
        classify_kiro_response(500, "internal server error", 0),
        KiroRequestOutcome::NonRetryableError
    );
}

#[test]
fn classify_418_is_non_retryable() {
    assert_eq!(
        classify_kiro_response(418, "I'm a teapot", 0),
        KiroRequestOutcome::NonRetryableError
    );
}

// ── body pattern matching tests ──────────────────────────────────────────────

#[test]
fn suspension_pattern_suspended() {
    assert!(body_indicates_suspension("your account is SUSPENDED"));
}

#[test]
fn suspension_pattern_temporarily() {
    assert!(body_indicates_suspension("TEMPORARILY_SUSPENDED by admin"));
}

#[test]
fn suspension_pattern_negative() {
    // lowercase "suspended" should NOT match (Go is case-sensitive)
    assert!(!body_indicates_suspension("your account is suspended"));
}

#[test]
fn token_error_pattern_token() {
    assert!(body_indicates_token_error("invalid token provided"));
}

#[test]
fn token_error_pattern_expired() {
    assert!(body_indicates_token_error("credential expired"));
}

#[test]
fn token_error_pattern_negative() {
    assert!(!body_indicates_token_error("access denied"));
}

#[test]
fn suspend_keywords_case_insensitive() {
    assert!(body_matches_suspend_keywords(
        "Your Account Has Been disabled"
    ));
    assert!(body_matches_suspend_keywords("BANNED for abuse"));
    assert!(body_matches_suspend_keywords("Quota Exceeded"));
    assert!(body_matches_suspend_keywords(
        "too many requests to the API"
    ));
    assert!(!body_matches_suspend_keywords("everything is fine"));
}

// ── cooldown duration tests ──────────────────────────────────────────────────

#[test]
fn cooldown_429_retry_0() {
    assert_eq!(calculate_429_cooldown(0), Duration::from_secs(60));
}

#[test]
fn cooldown_429_retry_1() {
    assert_eq!(calculate_429_cooldown(1), Duration::from_secs(120));
}

#[test]
fn cooldown_429_retry_2() {
    assert_eq!(calculate_429_cooldown(2), Duration::from_secs(240));
}

#[test]
fn cooldown_429_retry_3_capped() {
    assert_eq!(calculate_429_cooldown(3), Duration::from_secs(300));
}

#[test]
fn cooldown_429_retry_10_capped() {
    assert_eq!(calculate_429_cooldown(10), Duration::from_secs(300));
}

#[test]
fn cooldown_for_success_is_none() {
    assert!(cooldown_for_outcome(&KiroRequestOutcome::Success).is_none());
}

#[test]
fn cooldown_for_rate_limited() {
    let d = cooldown_for_outcome(&KiroRequestOutcome::RateLimited { retry_count: 1 });
    assert_eq!(d, Some(Duration::from_secs(120)));
}

#[test]
fn cooldown_for_suspended_is_24h() {
    let d = cooldown_for_outcome(&KiroRequestOutcome::Suspended);
    assert_eq!(d, Some(Duration::from_secs(86400)));
}

#[test]
fn cooldown_for_quota_exhausted_is_24h() {
    let d = cooldown_for_outcome(&KiroRequestOutcome::QuotaExhausted);
    assert_eq!(d, Some(Duration::from_secs(86400)));
}

#[test]
fn cooldown_for_validation_error_is_none() {
    assert!(cooldown_for_outcome(&KiroRequestOutcome::ValidationError).is_none());
}

// ── cooldown reason tests ────────────────────────────────────────────────────

#[test]
fn cooldown_reason_429() {
    assert_eq!(
        cooldown_reason_for_outcome(&KiroRequestOutcome::RateLimited { retry_count: 0 }),
        Some(COOLDOWN_REASON_429)
    );
}

#[test]
fn cooldown_reason_suspended() {
    assert_eq!(
        cooldown_reason_for_outcome(&KiroRequestOutcome::Suspended),
        Some(COOLDOWN_REASON_SUSPENDED)
    );
}

#[test]
fn cooldown_reason_quota() {
    assert_eq!(
        cooldown_reason_for_outcome(&KiroRequestOutcome::QuotaExhausted),
        Some(COOLDOWN_REASON_QUOTA)
    );
}

#[test]
fn cooldown_reason_success_is_none() {
    assert!(cooldown_reason_for_outcome(&KiroRequestOutcome::Success).is_none());
}

// ── registry action tests ────────────────────────────────────────────────────

#[test]
fn registry_action_success_clears() {
    assert_eq!(
        registry_action_for_outcome(&KiroRequestOutcome::Success),
        RegistryAction::ClearFailureState
    );
}

#[test]
fn registry_action_429_marks_quota() {
    assert_eq!(
        registry_action_for_outcome(&KiroRequestOutcome::RateLimited { retry_count: 0 }),
        RegistryAction::MarkQuotaExceeded
    );
}

#[test]
fn registry_action_quota_exhausted_marks_quota() {
    assert_eq!(
        registry_action_for_outcome(&KiroRequestOutcome::QuotaExhausted),
        RegistryAction::MarkQuotaExceeded
    );
}

#[test]
fn registry_action_suspended_suspends() {
    assert_eq!(
        registry_action_for_outcome(&KiroRequestOutcome::Suspended),
        RegistryAction::SuspendClient {
            reason: "account_suspended".to_string()
        }
    );
}

#[test]
fn registry_action_validation_error_is_none() {
    assert_eq!(
        registry_action_for_outcome(&KiroRequestOutcome::ValidationError),
        RegistryAction::None
    );
}

#[test]
fn registry_action_token_expired_is_none() {
    assert_eq!(
        registry_action_for_outcome(&KiroRequestOutcome::TokenExpired),
        RegistryAction::None
    );
}

#[test]
fn rate_limiter_marks_failures_and_success() {
    let mut limiter = KiroRateLimiter::default();
    let token_key = "kiro-auth-1";
    let now = Instant::now();

    assert!(limiter.is_token_available(token_key, now));

    limiter.mark_token_failed(token_key, now);
    assert!(!limiter.is_token_available(token_key, now));

    limiter.mark_token_success(token_key);
    assert!(limiter.is_token_available(token_key, now));
}

#[test]
fn rate_limiter_reports_required_wait_after_failure() {
    let mut limiter = KiroRateLimiter::default();
    let token_key = "kiro-auth-1";
    let now = Instant::now();

    limiter.mark_token_failed(token_key, now);

    assert_eq!(
        limiter.required_wait(token_key, now),
        Some(Duration::from_secs(30))
    );
}

#[test]
fn rate_limiter_marks_suspended_tokens_from_error_keywords() {
    let mut limiter = KiroRateLimiter::default();
    let token_key = "kiro-auth-1";
    let now = Instant::now();

    assert!(limiter.check_and_mark_suspended(token_key, "account disabled", now));
    assert!(!limiter.is_token_available(token_key, now));
}

#[tokio::test]
async fn proxy_state_initializes_kiro_runtime_dependencies() {
    let config = Config::default();
    let accounts = Arc::new(AccountManager::with_dir("/tmp/rusuh_test_nonexistent"));
    let registry = Arc::new(ModelRegistry::new());
    let state = ProxyState::new(config, accounts, registry, 0);

    let now = Instant::now();
    let auth_id = "kiro-auth-1";
    let model_id = "claude-sonnet-4";

    {
        let mut cooldown = state.kiro_runtime.cooldown.write().await;
        cooldown.set_cooldown(auth_id, model_id, Duration::from_secs(30), "test", now);
    }

    {
        let cooldown = state.kiro_runtime.cooldown.read().await;
        assert!(cooldown.is_in_cooldown(auth_id, model_id, now));
    }

    {
        let mut limiter = state.kiro_runtime.rate_limiter.write().await;
        limiter.mark_token_failed(auth_id, now);
        assert!(!limiter.is_token_available(auth_id, now));
    }

    let quota = state
        .kiro_runtime
        .quota_checker
        .check_quota(&UsageCheckRequest {
            access_token: "token".to_string(),
            profile_arn: "arn:aws:iam::123:role/test".to_string(),
            client_id: None,
            refresh_token: None,
        })
        .await;
    assert_eq!(quota, QuotaStatus::Unknown);
}
