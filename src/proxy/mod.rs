pub mod balancer;
pub mod handlers;
pub mod kiro_oauth;
pub mod management;
pub mod oauth;
pub mod stream;

use std::sync::Arc;

use tokio::sync::RwLock;

use crate::auth::manager::AccountManager;
use crate::config::Config;
use crate::providers::model_registry::ModelRegistry;
use crate::providers::Provider;

use self::balancer::{Balancer, Strategy};
use self::oauth::OAuthSessionStore;

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
        }
    }
}
