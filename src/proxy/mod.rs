pub mod balancer;
pub mod execution_session;
pub mod handlers;
pub mod management;
pub mod oauth;
pub mod stream;
pub mod zed_import;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};

use crate::auth::kiro_runtime::{CooldownManager, KiroRateLimiter, NoOpQuotaChecker, QuotaChecker};
use crate::auth::manager::AccountManager;
use crate::auth::zed_session::{new_session_store, ZedLoginSessionStore};
use crate::config::Config;
use crate::error::{AppError, AppResult};
use crate::providers::model_info::ExtModelInfo;
use crate::providers::model_registry::ModelRegistry;
use crate::providers::Provider;

use self::balancer::{Balancer, Strategy};
use self::execution_session::ExecutionSessionStore;
use self::oauth::OAuthSessionStore;

pub(crate) struct RuntimeSnapshot {
    providers: Vec<Arc<dyn Provider>>,
    balancer: Balancer,
    clients_by_model: HashMap<String, Vec<String>>,
    providers_by_model: HashMap<String, Vec<String>>,
    models_by_client: HashMap<String, HashSet<String>>,
}

impl RuntimeSnapshot {
    fn new(
        providers: Vec<Arc<dyn Provider>>,
        strategy: Strategy,
        replacement_models: &HashMap<String, (String, Vec<ExtModelInfo>)>,
    ) -> Self {
        let provider_count = providers.len();
        let mut clients_by_model: HashMap<String, Vec<String>> = HashMap::new();
        let mut providers_by_model: HashMap<String, HashSet<String>> = HashMap::new();
        let mut models_by_client: HashMap<String, HashSet<String>> = HashMap::new();

        for (client_id, (provider_name, ext_models)) in replacement_models {
            let client_models = models_by_client.entry(client_id.clone()).or_default();
            for ext_model in ext_models {
                client_models.insert(ext_model.id.clone());
                clients_by_model
                    .entry(ext_model.id.clone())
                    .or_default()
                    .push(client_id.clone());
                providers_by_model
                    .entry(ext_model.id.clone())
                    .or_default()
                    .insert(provider_name.clone());
            }
        }

        let providers_by_model = providers_by_model
            .into_iter()
            .map(|(model_id, providers)| {
                let mut providers = providers.into_iter().collect::<Vec<_>>();
                providers.sort();
                (model_id, providers)
            })
            .collect();

        Self {
            providers,
            balancer: Balancer::new(strategy, provider_count),
            clients_by_model,
            providers_by_model,
            models_by_client,
        }
    }

    pub(crate) fn providers(&self) -> &[Arc<dyn Provider>] {
        &self.providers
    }

    pub(crate) fn balancer(&self) -> &Balancer {
        &self.balancer
    }

    pub(crate) fn provider_count(&self) -> usize {
        self.providers.len()
    }

    pub(crate) fn model_providers(&self, model_id: &str) -> &[String] {
        self.providers_by_model
            .get(model_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub(crate) fn available_clients_for_model(&self, model_id: &str) -> &[String] {
        self.clients_by_model
            .get(model_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub(crate) fn client_supports_model(&self, client_id: &str, model_id: &str) -> bool {
        self.models_by_client
            .get(client_id)
            .map(|models| models.iter().any(|id| id.eq_ignore_ascii_case(model_id)))
            .unwrap_or(false)
    }
}

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
    /// Compatibility/testing mirror of the last published providers. Routing should use runtime_snapshot.
    pub providers: RwLock<Vec<Arc<dyn Provider>>>,
    runtime_snapshot: RwLock<Arc<RuntimeSnapshot>>,
    /// Account manager — loaded from auth-dir at startup
    pub accounts: Arc<AccountManager>,
    /// Global model registry with ref counting, quota, suspension
    pub model_registry: Arc<ModelRegistry>,
    /// In-memory OAuth session tracker for web-triggered flows
    pub oauth_sessions: OAuthSessionStore,
    /// In-memory Zed native login session tracker
    pub zed_login_sessions: ZedLoginSessionStore,
    /// In-memory execution-session to selected-auth mapping for sticky routing
    pub execution_sessions: ExecutionSessionStore,
    /// Kiro-specific cooldown, rate-limit, and quota probing state
    pub kiro_runtime: KiroRuntimeState,
    /// Serializes provider runtime refreshes so runtime state is swapped atomically.
    runtime_refresh_lock: Mutex<()>,
}

impl ProxyState {
    pub(crate) async fn current_runtime_snapshot(&self) -> Arc<RuntimeSnapshot> {
        self.runtime_snapshot.read().await.clone()
    }

    async fn publish_runtime_snapshot(&self, snapshot: RuntimeSnapshot) {
        let published_providers = snapshot.providers().to_vec();
        let mut providers = self.providers.write().await;
        let mut runtime_snapshot = self.runtime_snapshot.write().await;
        *providers = published_providers;
        *runtime_snapshot = Arc::new(snapshot);
    }

    pub async fn publish_runtime_from_providers(
        &self,
        providers: Vec<Arc<dyn Provider>>,
    ) -> AppResult<HashMap<String, (String, Vec<ExtModelInfo>)>> {
        let replacement_models = Self::collect_runtime_models(&providers).await?;
        let strategy = {
            let config = self.config.read().await;
            Strategy::parse(&config.routing.strategy)
        };
        let snapshot = RuntimeSnapshot::new(providers, strategy, &replacement_models);
        self.publish_runtime_snapshot(snapshot).await;
        Ok(replacement_models)
    }

    async fn collect_runtime_models(
        providers: &[Arc<dyn Provider>],
    ) -> AppResult<HashMap<String, (String, Vec<ExtModelInfo>)>> {
        let mut replacement_models: HashMap<String, (String, Vec<ExtModelInfo>)> = HashMap::new();

        for provider in providers {
            let client_id = provider.client_id().to_string();
            let models =
                provider
                    .list_models()
                    .await
                    .map_err(|error| AppError::ProviderOperation {
                        op: "list_models",
                        provider: provider.provider_type().to_string(),
                        source: error.into(),
                    })?;

            if models.is_empty() {
                tracing::warn!(
                    "provider {} returned no models during refresh",
                    provider.provider_type()
                );
                continue;
            }

            let ext_models: Vec<ExtModelInfo> = models
                .into_iter()
                .map(|model| ExtModelInfo {
                    id: model.id.clone(),
                    object: model.object,
                    created: model.created,
                    owned_by: model.owned_by,
                    provider_type: provider.provider_type().to_string(),
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
            replacement_models.insert(
                client_id,
                (provider.provider_type().to_string(), ext_models),
            );
        }

        Ok(replacement_models)
    }

    pub async fn rebuild_runtime_snapshot(
        &self,
    ) -> AppResult<(Vec<Arc<dyn Provider>>, HashMap<String, (String, Vec<ExtModelInfo>)>)> {
        let config = self.config.read().await.clone();
        let providers = crate::providers::registry::build_providers(
            &config,
            &self.accounts,
            self.model_registry.clone(),
            self.kiro_runtime.clone(),
        )
        .await;
        let replacement_models = Self::collect_runtime_models(&providers).await?;
        Ok((providers, replacement_models))
    }

    pub async fn refresh_provider_runtime(&self) -> AppResult<()> {
        let _refresh_guard = self.runtime_refresh_lock.lock().await;

        let previous_client_ids = self
            .current_runtime_snapshot()
            .await
            .providers
            .iter()
            .map(|provider| provider.client_id().to_string())
            .collect::<Vec<_>>();

        let (providers, replacement_models) = self.rebuild_runtime_snapshot().await?;
        let replacement_client_ids: HashSet<String> = replacement_models.keys().cloned().collect();

        for (client_id, (provider_name, ext_models)) in &replacement_models {
            self.model_registry
                .register_client(client_id, provider_name, ext_models.clone())
                .await;
        }

        for client_id in &previous_client_ids {
            if !replacement_client_ids.contains(client_id) {
                self.model_registry.unregister_client(client_id).await;
                self.execution_sessions
                    .invalidate_selected_auth(client_id)
                    .await;
            }
        }
        self.execution_sessions
            .invalidate_unknown_selected_auths(&replacement_client_ids)
            .await;

        self.publish_runtime_from_providers(providers).await?;

        Ok(())
    }

    pub fn new(
        config: Config,
        accounts: Arc<AccountManager>,
        model_registry: Arc<ModelRegistry>,
        provider_count: usize,
    ) -> Self {
        let strategy = Strategy::parse(&config.routing.strategy);
        let initial_snapshot = Arc::new(RuntimeSnapshot::new(
            Vec::with_capacity(provider_count),
            strategy,
            &HashMap::new(),
        ));
        Self {
            config: RwLock::new(config),
            providers: RwLock::new(Vec::with_capacity(provider_count)),
            runtime_snapshot: RwLock::new(initial_snapshot),
            accounts,
            model_registry,
            oauth_sessions: OAuthSessionStore::new(),
            zed_login_sessions: new_session_store(),
            execution_sessions: ExecutionSessionStore::new(),
            kiro_runtime: KiroRuntimeState::default(),
            runtime_refresh_lock: Mutex::new(()),
        }
    }
}
