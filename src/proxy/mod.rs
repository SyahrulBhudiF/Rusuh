pub mod balancer;
pub mod execution_session;
pub mod handlers;
pub mod management;
pub mod oauth;
pub mod stream;
pub mod zed_import;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::auth::kiro_runtime::{CooldownManager, KiroRateLimiter, NoOpQuotaChecker, QuotaChecker};
use crate::auth::manager::AccountManager;
use crate::auth::zed_session::{new_session_store, ZedLoginSessionStore};
use crate::config::Config;
use crate::providers::model_info::ExtModelInfo;
use crate::providers::model_registry::ModelRegistry;
use crate::providers::Provider;

use self::balancer::{Balancer, Strategy};
use self::execution_session::ExecutionSessionStore;
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
            quota_checker: Arc::new(NoOpQuotaChecker),
        }
    }
}

/// Shared application state — injected into all route handlers.
pub struct ProxyState {
    pub config: RwLock<Config>,
    /// Registered upstream providers (populated at startup from config)
    pub providers: RwLock<Vec<Arc<dyn Provider>>>,
    /// Account manager — loaded from auth-dir at startup
    pub accounts: Arc<AccountManager>,
    /// Global model registry with ref counting, quota, suspension
    pub model_registry: Arc<ModelRegistry>,
    /// Load balancer for distributing requests across providers
    pub balancer: RwLock<Balancer>,
    /// In-memory OAuth session tracker for web-triggered flows
    pub oauth_sessions: OAuthSessionStore,
    /// In-memory Zed native login session tracker
    pub zed_login_sessions: ZedLoginSessionStore,
    /// In-memory execution-session to selected-auth mapping for sticky routing
    pub execution_sessions: ExecutionSessionStore,
    /// Kiro-specific cooldown, rate-limit, and quota probing state
    pub kiro_runtime: KiroRuntimeState,
}

impl ProxyState {
    pub async fn refresh_provider_runtime(&self) -> anyhow::Result<()> {
        let previous_client_ids = {
            let providers = self.providers.read().await;
            providers
                .iter()
                .enumerate()
                .map(|(idx, provider)| format!("{}_{}", provider.name(), idx))
                .collect::<Vec<_>>()
        };

        let config = self.config.read().await.clone();
        let providers = crate::providers::registry::build_providers(
            &config,
            &self.accounts,
            self.model_registry.clone(),
            self.kiro_runtime.clone(),
        )
        .await;

        let replacement_client_ids: HashSet<String> = providers
            .iter()
            .enumerate()
            .map(|(idx, provider)| format!("{}_{}", provider.name(), idx))
            .collect();
        let mut replacement_models: HashMap<String, (String, Vec<ExtModelInfo>)> = HashMap::new();

        for (idx, provider) in providers.iter().enumerate() {
            let client_id = format!("{}_{}", provider.name(), idx);
            let models = provider.list_models().await.map_err(|error| {
                anyhow::anyhow!("list models from {}: {error}", provider.name())
            })?;

            if models.is_empty() {
                tracing::warn!("provider {} returned no models during refresh", provider.name());
                continue;
            }

            let ext_models: Vec<ExtModelInfo> = models
                .into_iter()
                .map(|model| ExtModelInfo {
                    id: model.id.clone(),
                    object: model.object,
                    created: model.created,
                    owned_by: model.owned_by,
                    provider_type: provider.name().to_string(),
                    display_name: Some(model.id),
                    name: None,
                    version: None,
                    description: None,
                    input_token_limit: 0,
                    output_token_limit: 0,
                    supported_generation_methods: vec![],
                    context_length: 0,
                    max_completion_tokens: 0,
                    supported_parameters: vec![],
                    thinking: None,
                    user_defined: false,
                })
                .collect();
            replacement_models.insert(client_id, (provider.name().to_string(), ext_models));
        }

        let provider_count = providers.len();

        {
            let mut providers_guard = self.providers.write().await;
            *providers_guard = providers;
        }

        {
            let mut balancer = self.balancer.write().await;
            let strategy = {
                let cfg = self.config.read().await;
                Strategy::parse(&cfg.routing.strategy)
            };
            *balancer = Balancer::new(strategy, provider_count);
        }

        for (client_id, (provider_name, ext_models)) in replacement_models {
            self.model_registry
                .register_client(&client_id, &provider_name, ext_models)
                .await;
        }

        for client_id in &previous_client_ids {
            if !replacement_client_ids.contains(client_id) {
                self.model_registry.unregister_client(client_id).await;
            }
        }

        Ok(())
    }

    pub fn new(
        config: Config,
        accounts: Arc<AccountManager>,
        model_registry: Arc<ModelRegistry>,
        provider_count: usize,
    ) -> Self {
        let strategy = Strategy::parse(&config.routing.strategy);
        Self {
            config: RwLock::new(config),
            providers: RwLock::new(Vec::new()),
            accounts,
            model_registry,
            balancer: RwLock::new(Balancer::new(strategy, provider_count)),
            oauth_sessions: OAuthSessionStore::new(),
            zed_login_sessions: new_session_store(),
            execution_sessions: ExecutionSessionStore::new(),
            kiro_runtime: KiroRuntimeState::default(),
        }
    }
}
