//! Kiro request outcome classification — pure helpers for error classification,
//! cooldown duration determination, and registry action mapping.
//!
//! Mirrors `CLIProxyAPIPlus/internal/runtime/executor/kiro_executor.go` error handling.

use std::time::Duration;

// ── Cooldown constants ───────────────────────────────────────────────────────

/// Default short cooldown for 429 responses (1 minute base)
const DEFAULT_SHORT_COOLDOWN: Duration = Duration::from_secs(60);

/// Maximum short cooldown cap (5 minutes)
const MAX_SHORT_COOLDOWN: Duration = Duration::from_secs(300);

/// Long cooldown for suspended accounts (24 hours)
const LONG_COOLDOWN: Duration = Duration::from_secs(86400);

// ── Outcome enum ─────────────────────────────────────────────────────────────

/// Outcome of a Kiro API request, used to determine cooldown and registry actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KiroRequestOutcome {
    /// Request succeeded — clear failure state.
    Success,
    /// Rate limited (HTTP 429) — apply short exponential cooldown.
    RateLimited { retry_count: usize },
    /// Account suspended (HTTP 403 with SUSPENDED/TEMPORARILY_SUSPENDED in body).
    Suspended,
    /// Token expired or invalid (HTTP 403 with token-related keywords).
    TokenExpired,
    /// Quota exhausted (HTTP 402 or quota-related errors).
    QuotaExhausted,
    /// Validation error (HTTP 400) — not retryable, no cooldown.
    ValidationError,
    /// Retryable server error (HTTP 502/503/504).
    RetryableServerError,
    /// Non-retryable error — no cooldown needed.
    NonRetryableError,
}

// ── Cooldown reason strings ──────────────────────────────────────────────────

/// Cooldown reason for 429 rate limits.
pub const COOLDOWN_REASON_429: &str = "rate_limit_exceeded";

/// Cooldown reason for suspended accounts.
pub const COOLDOWN_REASON_SUSPENDED: &str = "account_suspended";

/// Cooldown reason for quota exhaustion.
pub const COOLDOWN_REASON_QUOTA: &str = "quota_exhausted";

// ── Classification helpers ───────────────────────────────────────────────────

/// Classify a Kiro HTTP response into a request outcome.
///
/// `retry_count` is the number of 429 retries so far (used for exponential backoff).
pub fn classify_kiro_response(status: u16, body: &str, retry_count: usize) -> KiroRequestOutcome {
    match status {
        200..=299 => KiroRequestOutcome::Success,
        400 => KiroRequestOutcome::ValidationError,
        401 => KiroRequestOutcome::TokenExpired,
        402 => KiroRequestOutcome::QuotaExhausted,
        403 => {
            if body_indicates_suspension(body) {
                KiroRequestOutcome::Suspended
            } else if body_indicates_token_error(body) {
                KiroRequestOutcome::TokenExpired
            } else {
                KiroRequestOutcome::NonRetryableError
            }
        }
        429 => KiroRequestOutcome::RateLimited { retry_count },
        502..=504 => KiroRequestOutcome::RetryableServerError,
        _ => KiroRequestOutcome::NonRetryableError,
    }
}

/// Check if a response body contains suspension patterns (case-sensitive, matching Go).
pub fn body_indicates_suspension(body: &str) -> bool {
    body.contains("SUSPENDED") || body.contains("TEMPORARILY_SUSPENDED")
}

/// Check if a response body contains token-related error patterns (case-sensitive, matching Go).
pub fn body_indicates_token_error(body: &str) -> bool {
    body.contains("token")
        || body.contains("expired")
        || body.contains("invalid")
        || body.contains("unauthorized")
}

/// Check if a response body contains suspension keywords (case-insensitive).
///
/// Used for the more aggressive `CheckAndMarkSuspended` classification
/// that the rate limiter performs.
pub fn body_matches_suspend_keywords(body: &str) -> bool {
    let lower = body.to_lowercase();
    const KEYWORDS: &[&str] = &[
        "suspended",
        "banned",
        "disabled",
        "account has been",
        "access denied",
        "rate limit exceeded",
        "too many requests",
        "quota exceeded",
    ];
    KEYWORDS.iter().any(|kw| lower.contains(kw))
}

// ── Cooldown duration helpers ────────────────────────────────────────────────

/// Calculate cooldown duration for an outcome.
///
/// Returns `None` for outcomes that don't require a cooldown.
pub fn cooldown_for_outcome(outcome: &KiroRequestOutcome) -> Option<Duration> {
    match outcome {
        KiroRequestOutcome::Success => None,
        KiroRequestOutcome::RateLimited { retry_count } => {
            Some(calculate_429_cooldown(*retry_count))
        }
        KiroRequestOutcome::Suspended => Some(LONG_COOLDOWN),
        KiroRequestOutcome::QuotaExhausted => Some(LONG_COOLDOWN),
        KiroRequestOutcome::TokenExpired => None,
        KiroRequestOutcome::ValidationError => None,
        KiroRequestOutcome::RetryableServerError => None,
        KiroRequestOutcome::NonRetryableError => None,
    }
}

/// Calculate exponential cooldown for 429 responses.
///
/// Formula: DEFAULT_SHORT_COOLDOWN * 2^retry_count, capped at MAX_SHORT_COOLDOWN.
pub fn calculate_429_cooldown(retry_count: usize) -> Duration {
    let base_secs = DEFAULT_SHORT_COOLDOWN.as_secs();
    let multiplier = 1u64.checked_shl(retry_count as u32).unwrap_or(u64::MAX);
    let secs = base_secs.saturating_mul(multiplier);
    let d = Duration::from_secs(secs);
    if d > MAX_SHORT_COOLDOWN {
        MAX_SHORT_COOLDOWN
    } else {
        d
    }
}

/// Get the cooldown reason string for an outcome, or `None` if no cooldown applies.
pub fn cooldown_reason_for_outcome(outcome: &KiroRequestOutcome) -> Option<&'static str> {
    match outcome {
        KiroRequestOutcome::RateLimited { .. } => Some(COOLDOWN_REASON_429),
        KiroRequestOutcome::Suspended => Some(COOLDOWN_REASON_SUSPENDED),
        KiroRequestOutcome::QuotaExhausted => Some(COOLDOWN_REASON_QUOTA),
        _ => None,
    }
}

// ── Registry action mapping ──────────────────────────────────────────────────

/// Actions the caller should take on the model registry after a request outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryAction {
    /// No registry update needed.
    None,
    /// Clear any failure state (quota exceeded, suspension) for this client/model.
    ClearFailureState,
    /// Mark the client as quota-exceeded for this model.
    MarkQuotaExceeded,
    /// Suspend the client for this model with the given reason.
    SuspendClient { reason: String },
}

/// Determine what registry action should follow a request outcome.
pub fn registry_action_for_outcome(outcome: &KiroRequestOutcome) -> RegistryAction {
    match outcome {
        KiroRequestOutcome::Success => RegistryAction::ClearFailureState,
        KiroRequestOutcome::RateLimited { .. } => RegistryAction::MarkQuotaExceeded,
        KiroRequestOutcome::QuotaExhausted => RegistryAction::MarkQuotaExceeded,
        KiroRequestOutcome::Suspended => RegistryAction::SuspendClient {
            reason: COOLDOWN_REASON_SUSPENDED.to_string(),
        },
        _ => RegistryAction::None,
    }
}
