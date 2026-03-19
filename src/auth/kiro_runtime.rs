//! Kiro runtime utilities — cooldown, quota, rate limiter, usage checker, and refresh manager.
//!
//! Consolidates:
//! - `kiro_cooldown.rs`
//! - `kiro_quota.rs`
//! - `kiro_rate_limiter.rs`
//! - `kiro_usage_checker.rs`
//! - `kiro_refresh_manager.rs`

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::time::interval;
use tracing::{debug, info, warn};

use super::manager::AccountManager;
use super::store::AuthRecord;

// ── Cooldown ──────────────────────────────────────────────────────────────────

/// Key for cooldown entries: (auth_id, model_id).
type CooldownKey = (String, String);

/// A single cooldown entry with expiry and reason.
#[derive(Debug, Clone)]
pub struct CooldownEntry {
    pub expires_at: Instant,
    pub reason: String,
}

/// Thread-safe cooldown manager.
///
/// Designed for synchronous access behind a `tokio::sync::RwLock` at the call
/// site when shared across tasks.
#[derive(Debug, Default)]
pub struct CooldownManager {
    entries: HashMap<CooldownKey, CooldownEntry>,
}

/// Default cooldown for a generic 429 response.
const DEFAULT_429_COOLDOWN: Duration = Duration::from_secs(60);

/// Minimum cooldown floor.
const MIN_COOLDOWN: Duration = Duration::from_secs(5);

/// Long cooldown for quota-exhausted or suspension-like errors.
const LONG_COOLDOWN: Duration = Duration::from_secs(300);

impl CooldownManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a cooldown for (auth_id, model_id) starting from `now`.
    pub fn set_cooldown(
        &mut self,
        auth_id: &str,
        model_id: &str,
        duration: Duration,
        reason: &str,
        now: Instant,
    ) {
        self.entries.insert(
            (auth_id.to_string(), model_id.to_string()),
            CooldownEntry {
                expires_at: now + duration,
                reason: reason.to_string(),
            },
        );
    }

    /// Check whether the given (auth_id, model_id) is currently in cooldown.
    pub fn is_in_cooldown(&self, auth_id: &str, model_id: &str, now: Instant) -> bool {
        self.entries
            .get(&(auth_id.to_string(), model_id.to_string()))
            .is_some_and(|e| now < e.expires_at)
    }

    /// Get remaining cooldown duration, or `None` if not in cooldown.
    pub fn remaining_cooldown(
        &self,
        auth_id: &str,
        model_id: &str,
        now: Instant,
    ) -> Option<Duration> {
        self.entries
            .get(&(auth_id.to_string(), model_id.to_string()))
            .and_then(|e| {
                if now < e.expires_at {
                    Some(e.expires_at - now)
                } else {
                    None
                }
            })
    }

    /// Get the cooldown reason, or `None` if not in cooldown.
    pub fn cooldown_reason(&self, auth_id: &str, model_id: &str, now: Instant) -> Option<&str> {
        self.entries
            .get(&(auth_id.to_string(), model_id.to_string()))
            .and_then(|e| {
                if now < e.expires_at {
                    Some(e.reason.as_str())
                } else {
                    None
                }
            })
    }

    /// Clear cooldown for a specific (auth_id, model_id).
    pub fn clear_cooldown(&mut self, auth_id: &str, model_id: &str) {
        self.entries
            .remove(&(auth_id.to_string(), model_id.to_string()));
    }

    /// Calculate an appropriate cooldown duration for a 429 response.
    ///
    /// If a `Retry-After` header value (in seconds) is provided, use it
    /// (clamped to `[MIN_COOLDOWN, LONG_COOLDOWN]`). Otherwise fall back to
    /// `DEFAULT_429_COOLDOWN`.
    pub fn calculate_cooldown_for_429(retry_after_secs: Option<u64>) -> Duration {
        match retry_after_secs {
            Some(secs) => {
                let d = Duration::from_secs(secs);
                d.clamp(MIN_COOLDOWN, LONG_COOLDOWN)
            }
            None => DEFAULT_429_COOLDOWN,
        }
    }

    /// Calculate a long cooldown for quota-exhaustion or suspension errors.
    pub fn calculate_cooldown_for_quota_exceeded() -> Duration {
        LONG_COOLDOWN
    }

    /// Purge expired entries to avoid unbounded growth.
    pub fn purge_expired(&mut self, now: Instant) {
        self.entries.retain(|_, e| now < e.expires_at);
    }
}

// ── Quota ─────────────────────────────────────────────────────────────────────

/// Quota breakdown by type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaBreakdown {
    pub base_remaining: u64,
    pub free_trial_remaining: u64,
    pub subscription_title: Option<String>,
}

/// Quota status for a single Kiro auth record.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum QuotaStatus {
    /// Quota information is not available (checker disabled or not yet probed).
    #[default]
    Unknown,
    /// Quota is available with an optional remaining count and next reset timestamp.
    Available {
        remaining: Option<u64>,
        next_reset: Option<i64>,
        breakdown: Option<QuotaBreakdown>,
    },
    /// Quota is exhausted — requests should not be sent.
    Exhausted { detail: String },
}

impl QuotaStatus {
    /// Returns `true` if the quota is known to be exhausted.
    pub fn is_exhausted(&self) -> bool {
        matches!(self, Self::Exhausted { .. })
    }

    /// Returns the remaining quota if known and not exhausted.
    pub fn remaining(&self) -> Option<u64> {
        match self {
            Self::Available { remaining, .. } => *remaining,
            _ => None,
        }
    }

    /// Returns the next reset timestamp (Unix milliseconds) if known.
    pub fn next_reset(&self) -> Option<i64> {
        match self {
            Self::Available { next_reset, .. } => *next_reset,
            _ => None,
        }
    }
}

/// Request parameters for quota checking.
///
/// Contains all metadata needed for the real Kiro usage check:
/// - access_token: Bearer token for Authorization header
/// - profile_arn: Used in query params and for region extraction
/// - client_id: Used for account key derivation (priority 1)
/// - refresh_token: Used for account key derivation (priority 2)
#[derive(Debug, Clone)]
pub struct UsageCheckRequest {
    pub access_token: String,
    pub profile_arn: String,
    pub client_id: Option<String>,
    pub refresh_token: Option<String>,
}

/// Trait for checking Kiro quota.
///
/// Implementations can be real HTTP checkers or test fakes.
/// The default `NoOpQuotaChecker` always returns `Unknown`.
#[async_trait::async_trait]
pub trait QuotaChecker: Send + Sync {
    /// Check quota for the given request metadata.
    async fn check_quota(&self, request: &UsageCheckRequest) -> QuotaStatus;
}

/// No-op checker — always returns `Unknown`. Does not block requests.
#[derive(Debug, Default)]
pub struct NoOpQuotaChecker;

#[async_trait::async_trait]
impl QuotaChecker for NoOpQuotaChecker {
    async fn check_quota(&self, _request: &UsageCheckRequest) -> QuotaStatus {
        QuotaStatus::Unknown
    }
}

// ── Rate limiter ──────────────────────────────────────────────────────────────

/// Key for rate limiter state: auth/account identifier.
type TokenKey = String;

/// Per-auth backoff state.
#[derive(Debug, Clone, Default)]
struct TokenState {
    fail_count: usize,
    cooldown_end: Option<Instant>,
}

/// Default backoff for the first failure.
const DEFAULT_BACKOFF_BASE: Duration = Duration::from_secs(30);

/// Maximum backoff cap.
const DEFAULT_BACKOFF_MAX: Duration = Duration::from_secs(300);

/// Cooldown applied when the error suggests the auth is suspended.
const DEFAULT_SUSPEND_COOLDOWN: Duration = Duration::from_secs(3600);

const SUSPEND_KEYWORDS: &[&str] = &[
    "suspended",
    "banned",
    "disabled",
    "account has been",
    "access denied",
    "rate limit exceeded",
    "too many requests",
    "quota exceeded",
];

/// Kiro per-auth rate limiter.
#[derive(Debug, Default)]
pub struct KiroRateLimiter {
    states: HashMap<TokenKey, TokenState>,
}

impl KiroRateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if the auth is currently usable.
    pub fn is_token_available(&self, token_key: &str, now: Instant) -> bool {
        self.required_wait(token_key, now).is_none()
    }

    /// Returns how long the caller should wait before using this auth again.
    pub fn required_wait(&self, token_key: &str, now: Instant) -> Option<Duration> {
        self.states.get(token_key).and_then(|state| {
            state.cooldown_end.and_then(|cooldown_end| {
                if now < cooldown_end {
                    Some(cooldown_end - now)
                } else {
                    None
                }
            })
        })
    }

    /// Marks a failed request and applies exponential backoff.
    pub fn mark_token_failed(&mut self, token_key: &str, now: Instant) {
        let state = self.states.entry(token_key.to_string()).or_default();
        state.fail_count += 1;
        state.cooldown_end = Some(now + calculate_backoff(state.fail_count));
    }

    /// Clears failure backoff after a successful request.
    pub fn mark_token_success(&mut self, token_key: &str) {
        if let Some(state) = self.states.get_mut(token_key) {
            state.fail_count = 0;
            state.cooldown_end = None;
        }
    }

    /// Checks common suspension-like keywords and applies a long cooldown when matched.
    pub fn check_and_mark_suspended(
        &mut self,
        token_key: &str,
        error_msg: &str,
        now: Instant,
    ) -> bool {
        let lower = error_msg.to_lowercase();
        if !SUSPEND_KEYWORDS
            .iter()
            .any(|keyword| lower.contains(keyword))
        {
            return false;
        }

        let state = self.states.entry(token_key.to_string()).or_default();
        state.fail_count = 0;
        state.cooldown_end = Some(now + DEFAULT_SUSPEND_COOLDOWN);
        true
    }
}

fn calculate_backoff(fail_count: usize) -> Duration {
    if fail_count == 0 {
        return Duration::ZERO;
    }

    let multiplier = 1u64
        .checked_shl((fail_count - 1) as u32)
        .unwrap_or(u64::MAX);
    let secs = DEFAULT_BACKOFF_BASE
        .as_secs()
        .saturating_mul(multiplier)
        .min(DEFAULT_BACKOFF_MAX.as_secs());
    Duration::from_secs(secs)
}

// ── Usage checker ─────────────────────────────────────────────────────────────

/// Response from Kiro /getUsageLimits endpoint.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageQuotaResponse {
    usage_breakdown_list: Vec<UsageBreakdown>,
    #[serde(default)]
    subscription_info: Option<SubscriptionInfo>,
    #[serde(default)]
    next_date_reset: f64,
}

/// Subscription information from Kiro.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubscriptionInfo {
    subscription_title: String,
}

/// Usage breakdown for a specific resource type.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageBreakdown {
    #[allow(dead_code)]
    resource_type: String,
    usage_limit_with_precision: f64,
    current_usage_with_precision: f64,
    #[serde(default)]
    free_trial_info: Option<FreeTrialInfo>,
}

/// Free trial usage information.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FreeTrialInfo {
    #[allow(dead_code)]
    free_trial_status: String,
    usage_limit_with_precision: f64,
    current_usage_with_precision: f64,
}

/// Real Kiro usage checker that performs HTTP requests.
#[derive(Debug, Clone)]
pub struct KiroUsageChecker {
    base_url: String,
    client: reqwest::Client,
}

impl KiroUsageChecker {
    /// Create a new usage checker with the given base URL.
    ///
    /// In production, the base URL should be derived from the profile ARN region.
    /// For testing, it can point to a local mock server.
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                // .connect_timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Check quota for the given request metadata.
    ///
    /// Returns `QuotaStatus::Unknown` on any error (network, HTTP, parse).
    /// This ensures probe failures don't block requests.
    pub async fn check(&self, request: &UsageCheckRequest) -> QuotaStatus {
        match self.check_internal(request).await {
            Ok(status) => status,
            Err(e) => {
                tracing::warn!("kiro quota check failed: {}", e);
                QuotaStatus::Unknown
            }
        }
    }

    async fn check_internal(
        &self,
        request: &UsageCheckRequest,
    ) -> Result<QuotaStatus, Box<dyn std::error::Error>> {
        // Build URL with query parameters (matches CLIProxyAPIPlus usage_checker.go)
        let url = format!(
            "{}/getUsageLimits?origin=AI_EDITOR&profileArn={}&resourceType=AGENTIC_REQUEST",
            self.base_url,
            urlencoding::encode(&request.profile_arn)
        );

        let account_key = get_account_key(
            request.client_id.as_deref(),
            request.refresh_token.as_deref(),
        );

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", request.access_token))
            .header("x-amz-user-agent", format!("aws-sdk-js/1.0.0 KiroIDE-0.10.32-{}", account_key))
            .header("User-Agent", "aws-sdk-js/1.0.0 ua/2.1 os/linux#6.8.0 lang/js md/nodejs#22.21.1 api/codewhispererruntime#1.0.0 m/N,E KiroIDE-0.10.32")
            .header("amz-sdk-invocation-id", uuid::Uuid::new_v4().to_string())
            .header("amz-sdk-request", "attempt=1; max=1")
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();

            // Parse suspension errors and return as Exhausted status
            if status == reqwest::StatusCode::FORBIDDEN && body.contains("TEMPORARILY_SUSPENDED") {
                return Ok(QuotaStatus::Exhausted { detail: body });
            }

            return Err(format!("HTTP error {}: {}", status, body).into());
        }

        let usage: UsageQuotaResponse = response.json().await?;

        Ok(compute_quota_status(&usage))
    }
}

/// Compute quota status from usage response.
///
/// Mirrors the logic from:
/// - `GetRemainingQuota` (sum positive remaining across main + free trial)
/// - `IsQuotaExhausted` (true if no breakdown or all fully used)
/// - Captures `nextDateReset` timestamp if present
fn compute_quota_status(usage: &UsageQuotaResponse) -> QuotaStatus {
    if usage.usage_breakdown_list.is_empty() {
        return QuotaStatus::Exhausted {
            detail: "No usage breakdown available".to_string(),
        };
    }

    let mut base_remaining: f64 = 0.0;
    let mut free_trial_remaining: f64 = 0.0;

    for breakdown in &usage.usage_breakdown_list {
        let main_remaining =
            breakdown.usage_limit_with_precision - breakdown.current_usage_with_precision;
        if main_remaining > 0.0 {
            base_remaining += main_remaining;
        }

        if let Some(free_trial) = &breakdown.free_trial_info {
            let free_remaining =
                free_trial.usage_limit_with_precision - free_trial.current_usage_with_precision;
            if free_remaining > 0.0 {
                free_trial_remaining += free_remaining;
            }
        }
    }

    let total_remaining = base_remaining + free_trial_remaining;

    let next_reset = if usage.next_date_reset > 0.0 {
        Some(usage.next_date_reset as i64)
    } else {
        None
    };

    let subscription_title = usage
        .subscription_info
        .as_ref()
        .map(|s| s.subscription_title.clone());

    if total_remaining <= 0.0 {
        QuotaStatus::Exhausted {
            detail: "All quota exhausted".to_string(),
        }
    } else {
        QuotaStatus::Available {
            remaining: Some(total_remaining as u64),
            next_reset,
            breakdown: Some(QuotaBreakdown {
                base_remaining: base_remaining as u64,
                free_trial_remaining: free_trial_remaining as u64,
                subscription_title,
            }),
        }
    }
}

/// Derive account key from client_id > refresh_token > random UUID.
///
/// Mirrors `GetAccountKey` from `CLIProxyAPIPlus/internal/auth/kiro/fingerprint.go`.
fn get_account_key(client_id: Option<&str>, refresh_token: Option<&str>) -> String {
    let seed = client_id.or(refresh_token).unwrap_or("fallback-uuid");

    let mut hasher = Sha256::new();
    hasher.update(seed.as_bytes());
    let hash = hasher.finalize();

    // Return first 16 hex chars (8 bytes)
    format!(
        "{:x}",
        &hash[..8].iter().fold(0u64, |acc, &b| (acc << 8) | b as u64)
    )
}

#[async_trait::async_trait]
impl QuotaChecker for KiroUsageChecker {
    async fn check_quota(&self, request: &UsageCheckRequest) -> QuotaStatus {
        self.check(request).await
    }
}

// ── Refresh manager ───────────────────────────────────────────────────────────

/// Callback invoked after successful token refresh.
/// Receives the auth file ID and the refreshed record.
pub type RefreshCallback = Arc<dyn Fn(String, AuthRecord) + Send + Sync>;

/// Background refresh manager for Kiro tokens.
pub struct KiroRefreshManager {
    account_manager: Arc<AccountManager>,
    interval: Duration,
    callback: Option<RefreshCallback>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl KiroRefreshManager {
    /// Creates a new refresh manager.
    pub fn new(account_manager: Arc<AccountManager>) -> Self {
        Self {
            account_manager,
            interval: Duration::from_secs(60), // 1 minute default
            callback: None,
            shutdown_tx: None,
        }
    }

    /// Sets the refresh interval.
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Registers a callback to be invoked after successful token refresh.
    pub fn with_callback(mut self, callback: RefreshCallback) -> Self {
        self.callback = Some(callback);
        debug!("kiro refresh manager: callback registered");
        self
    }

    /// Starts the background refresh task.
    pub fn start(&mut self) {
        if self.shutdown_tx.is_some() {
            debug!("kiro refresh manager: already started");
            return;
        }

        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);

        let account_manager = self.account_manager.clone();
        let interval_duration = self.interval;
        let callback = self.callback.clone();

        tokio::spawn(async move {
            let mut ticker = interval(interval_duration);
            info!(
                "kiro refresh manager: started with interval {:?}",
                interval_duration
            );

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        Self::refresh_batch(&account_manager, &callback).await;
                    }
                    _ = &mut shutdown_rx => {
                        info!("kiro refresh manager: shutting down");
                        break;
                    }
                }
            }
        });
    }

    /// Stops the background refresh task.
    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
            info!("kiro refresh manager: stopped");
        }
    }

    /// Refreshes a batch of tokens.
    async fn refresh_batch(
        account_manager: &Arc<AccountManager>,
        callback: &Option<RefreshCallback>,
    ) {
        // Reload accounts to get latest state
        if let Err(e) = account_manager.reload().await {
            warn!("kiro refresh manager: failed to reload accounts: {}", e);
            return;
        }

        let accounts_by_provider = account_manager.all_accounts().await;

        // Iterate over all providers and their accounts
        for (_provider, records) in accounts_by_provider {
            for record in records {
                // Only refresh Kiro records
                if record
                    .metadata
                    .get("type")
                    .and_then(|v: &serde_json::Value| v.as_str())
                    != Some("kiro")
                {
                    continue;
                }

                // Check if refresh is needed (within 20 minutes of expiry)
                if let Some(expires_at) = record
                    .metadata
                    .get("expires_at")
                    .and_then(|v: &serde_json::Value| v.as_str())
                {
                    if let Ok(expiry) = chrono::DateTime::parse_from_rfc3339(expires_at) {
                        let now = chrono::Utc::now();
                        let time_until_expiry = expiry.signed_duration_since(now);

                        if time_until_expiry > chrono::Duration::minutes(20) {
                            debug!(
                                "kiro refresh manager: {} not due for refresh yet",
                                record.id
                            );
                            continue;
                        }
                    } else {
                        continue;
                    }
                } else {
                    continue;
                }

                // Attempt refresh (placeholder - actual refresh logic would go here)
                debug!("kiro refresh manager: {} needs refresh", record.id);

                // TODO: Implement actual token refresh logic
                // For now, just invoke callback with existing record
                if let Some(cb) = callback {
                    cb(record.id.clone(), record);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;

    #[test]
    fn failed_token_enters_backoff() {
        let mut limiter = KiroRateLimiter::new();
        let now = Instant::now();

        limiter.mark_token_failed("auth-1", now);

        assert!(!limiter.is_token_available("auth-1", now));
        assert!(limiter.is_token_available("auth-1", now + DEFAULT_BACKOFF_BASE));
    }

    #[test]
    fn success_clears_backoff() {
        let mut limiter = KiroRateLimiter::new();
        let now = Instant::now();

        limiter.mark_token_failed("auth-1", now);
        limiter.mark_token_success("auth-1");

        assert!(limiter.is_token_available("auth-1", now));
    }

    #[test]
    fn test_compute_quota_status_available() {
        let usage = UsageQuotaResponse {
            usage_breakdown_list: vec![UsageBreakdown {
                resource_type: "AGENTIC_REQUEST".to_string(),
                usage_limit_with_precision: 100.0,
                current_usage_with_precision: 30.0,
                free_trial_info: None,
            }],
            subscription_info: None,
            next_date_reset: 0.0,
        };

        let status = compute_quota_status(&usage);
        assert_eq!(status.remaining(), Some(70));
        assert_eq!(status.next_reset(), None);

        if let QuotaStatus::Available { breakdown, .. } = status {
            let bd = breakdown.unwrap();
            assert_eq!(bd.base_remaining, 70);
            assert_eq!(bd.free_trial_remaining, 0);
        } else {
            panic!("Expected Available status");
        }
    }

    #[test]
    fn test_compute_quota_status_exhausted() {
        let usage = UsageQuotaResponse {
            usage_breakdown_list: vec![UsageBreakdown {
                resource_type: "AGENTIC_REQUEST".to_string(),
                usage_limit_with_precision: 100.0,
                current_usage_with_precision: 100.0,
                free_trial_info: None,
            }],
            subscription_info: None,
            next_date_reset: 0.0,
        };

        let status = compute_quota_status(&usage);
        assert!(status.is_exhausted());
    }

    #[test]
    fn test_compute_quota_status_with_free_trial() {
        let usage = UsageQuotaResponse {
            usage_breakdown_list: vec![UsageBreakdown {
                resource_type: "AGENTIC_REQUEST".to_string(),
                usage_limit_with_precision: 100.0,
                current_usage_with_precision: 100.0,
                free_trial_info: Some(FreeTrialInfo {
                    free_trial_status: "ACTIVE".to_string(),
                    usage_limit_with_precision: 50.0,
                    current_usage_with_precision: 10.0,
                }),
            }],
            subscription_info: None,
            next_date_reset: 1710777600000.0,
        };

        let status = compute_quota_status(&usage);
        assert_eq!(status.remaining(), Some(40));
        assert_eq!(status.next_reset(), Some(1710777600000));

        if let QuotaStatus::Available { breakdown, .. } = status {
            let bd = breakdown.unwrap();
            assert_eq!(bd.base_remaining, 0);
            assert_eq!(bd.free_trial_remaining, 40);
        } else {
            panic!("Expected Available status");
        }
    }

    #[test]
    fn test_compute_quota_status_empty_breakdown() {
        let usage = UsageQuotaResponse {
            usage_breakdown_list: vec![],
            subscription_info: None,
            next_date_reset: 0.0,
        };

        let status = compute_quota_status(&usage);
        assert!(status.is_exhausted());
    }

    #[test]
    fn test_get_account_key_prefers_client_id() {
        let key1 = get_account_key(Some("client-123"), Some("refresh-456"));
        let key2 = get_account_key(Some("client-123"), None);
        assert_eq!(key1, key2, "Should use client_id when available");
    }

    #[test]
    fn test_get_account_key_fallback_to_refresh_token() {
        let key1 = get_account_key(None, Some("refresh-456"));
        let key2 = get_account_key(None, Some("refresh-456"));
        assert_eq!(key1, key2, "Should be deterministic with refresh_token");
    }

    #[test]
    fn test_get_account_key_different_inputs_different_keys() {
        let key1 = get_account_key(Some("client-123"), None);
        let key2 = get_account_key(Some("client-456"), None);
        assert_ne!(
            key1, key2,
            "Different client_ids should produce different keys"
        );
    }

    #[tokio::test]
    async fn manager_starts_and_stops() {
        let dir = TempDir::new().unwrap();
        let account_manager = Arc::new(AccountManager::with_dir(dir.path()));
        let mut manager = KiroRefreshManager::new(account_manager);

        manager.start();
        assert!(manager.shutdown_tx.is_some());

        manager.stop();
        assert!(manager.shutdown_tx.is_none());
    }

    #[tokio::test]
    async fn callback_is_invoked_for_expiring_tokens() {
        let dir = TempDir::new().unwrap();
        let account_manager = Arc::new(AccountManager::with_dir(dir.path()));

        // Create a Kiro auth file that expires soon
        let expires_at = (chrono::Utc::now() + chrono::Duration::minutes(10)).to_rfc3339();
        let auth_json = serde_json::json!({
            "type": "kiro",
            "access_token": "test-token",
            "refresh_token": "test-refresh",
            "profile_arn": "arn:aws:iam::123456789012:role/test",
            "expires_at": expires_at,
            "auth_method": "builder-id",
            "provider": "AWS"
        });

        let auth_path = dir.path().join("kiro-test.json");
        std::fs::write(
            &auth_path,
            serde_json::to_string_pretty(&auth_json).unwrap(),
        )
        .unwrap();

        // Reload accounts
        account_manager.reload().await.unwrap();

        // Set up callback counter
        let callback_count = Arc::new(AtomicUsize::new(0));
        let callback_count_clone = callback_count.clone();

        let callback = Arc::new(move |_id: String, _record: AuthRecord| {
            callback_count_clone.fetch_add(1, Ordering::SeqCst);
        });

        let mut manager = KiroRefreshManager::new(account_manager)
            .with_interval(Duration::from_millis(100))
            .with_callback(callback);

        manager.start();

        // Wait for at least one refresh cycle
        tokio::time::sleep(Duration::from_millis(250)).await;

        manager.stop();

        // Callback should have been invoked at least once
        assert!(callback_count.load(Ordering::SeqCst) > 0);
    }
}
