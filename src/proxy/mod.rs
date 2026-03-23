pub mod balancer;
pub mod handlers;
pub mod management;
pub mod oauth;
pub mod stream;
pub mod zed_import;

use std::sync::Arc;

use tokio::sync::RwLock;

use crate::auth::kiro_runtime::{CooldownManager, KiroRateLimiter, KiroUsageChecker, QuotaChecker};
use crate::auth::zed_session::{new_session_store, ZedLoginSessionStore};
use crate::auth::manager::AccountManager;
use crate::config::Config;
use crate::providers::model_registry::ModelRegistry;
use crate::providers::Provider;

use self::balancer::{Balancer, Strategy};
use self::oauth::OAuthSessionStore;

/// Kiro-specific runtime state owned by the proxy.
#[derive(Clone)]
pub struct KiroRuntimeState {
    pub cooldown: Arc<RwLock<CooldownManager>>,
    pub rate_limiter: Arc<RwLock<KiroRateLimiter>>,
    pub quota_checker: Arc<dyn QuotaChecker>,
}

impl Default for KiroRuntimeState {
    fn default() -> Self {
        Self {
            cooldown: Arc::new(RwLock::new(CooldownManager::new())),
            rate_limiter: Arc::new(RwLock::new(KiroRateLimiter::new())),
            quota_checker: Arc::new(KiroUsageChecker::new("https://codewhisperer.us-east-1.amazonaws.com")),
        }
    }
}

/// Shared application state — injected into all route handlers.
pub struct ProxyState {
    pub config: RwLock<Config>,
    /// Registered upstream providers (populated at startup from config)
    pub providers: Vec<Arc<dyn Provider>>,
    /// Account manager — loaded from auth-dir at startup
    pub accounts: Arc<AccountManager>,
    /// Global model registry with ref counting, quota, suspension
    pub model_registry: Arc<ModelRegistry>,
    /// Load balancer for distributing requests across providers
    pub balancer: Balancer,
    /// In-memory OAuth session tracker for web-triggered flows
    pub oauth_sessions: OAuthSessionStore,
    /// In-memory Zed native login session tracker
    pub zed_login_sessions: ZedLoginSessionStore,
    /// Kiro-specific cooldown, rate-limit, and quota probing state
    pub kiro_runtime: KiroRuntimeState,
}

impl ProxyState {
    pub fn new(
        config: Config,
        accounts: Arc<AccountManager>,
        model_registry: Arc<ModelRegistry>,
        provider_count: usize,
    ) -> Self {
        let strategy = Strategy::parse(&config.routing.strategy);
        Self {
            config: RwLock::new(config),
            providers: Vec::new(),
            accounts,
            model_registry,
            balancer: Balancer::new(strategy, provider_count),
            oauth_sessions: OAuthSessionStore::new(),
            zed_login_sessions: new_session_store(),
            kiro_runtime: KiroRuntimeState::default(),
        }
    }
}
