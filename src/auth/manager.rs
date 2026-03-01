//! Account manager — enumerates credentials from the file store and groups them by provider.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tracing::info;

use crate::auth::store::{AuthRecord, FileTokenStore};
use crate::error::AppResult;

/// Manages loaded auth accounts, grouped by provider.
pub struct AccountManager {
    store: Arc<FileTokenStore>,
    /// provider → list of auth records
    accounts: tokio::sync::RwLock<HashMap<String, Vec<AuthRecord>>>,
}

impl AccountManager {
    /// Create a manager backed by the given store.
    pub fn new(store: Arc<FileTokenStore>) -> Self {
        Self {
            store,
            accounts: tokio::sync::RwLock::new(HashMap::new()),
        }
    }

    /// Create a manager with a default store pointing at `auth_dir`.
    pub fn with_dir(auth_dir: impl Into<PathBuf>) -> Self {
        let store = Arc::new(FileTokenStore::new(auth_dir));
        Self::new(store)
    }

    /// Reload all accounts from disk.
    pub async fn reload(&self) -> AppResult<()> {
        let records = self.store.list().await?;
        let mut by_provider: HashMap<String, Vec<AuthRecord>> = HashMap::new();

        for record in records {
            if record.disabled {
                info!(
                    "skipping disabled account: {} ({})",
                    record.id, record.provider
                );
                continue;
            }
            by_provider
                .entry(record.provider.clone())
                .or_default()
                .push(record);
        }

        let total: usize = by_provider.values().map(|v| v.len()).sum();
        let providers: Vec<_> = by_provider
            .iter()
            .map(|(k, v)| format!("{}({})", k, v.len()))
            .collect();

        info!(
            "loaded {} accounts across {} providers: [{}]",
            total,
            by_provider.len(),
            providers.join(", ")
        );

        *self.accounts.write().await = by_provider;
        Ok(())
    }

    /// Get all active accounts for a provider.
    pub async fn accounts_for(&self, provider: &str) -> Vec<AuthRecord> {
        self.accounts
            .read()
            .await
            .get(provider)
            .cloned()
            .unwrap_or_default()
    }

    /// Get all loaded accounts across all providers.
    pub async fn all_accounts(&self) -> HashMap<String, Vec<AuthRecord>> {
        self.accounts.read().await.clone()
    }

    /// Get the backing store (for save/delete operations).
    pub fn store(&self) -> &Arc<FileTokenStore> {
        &self.store
    }
}
