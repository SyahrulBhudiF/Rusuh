//! Account manager — enumerates credentials from the file store and groups them by provider.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;
use tracing::{info, warn};

use crate::auth::store::{AuthRecord, AuthStatus, FileTokenStore};
use crate::error::AppResult;

/// Manages loaded auth accounts, grouped by provider.
pub struct AccountManager {
    store: Arc<FileTokenStore>,
    /// provider_key → list of auth records
    /// provider_key → list of auth records
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
    ///
    /// For antigravity accounts missing `project_id`, auto-fetches it.
    pub async fn reload(&self) -> AppResult<()> {
        let mut records = self.store.list().await?;

        // Auto-fetch project_id for antigravity accounts that don't have one
        let client = reqwest::Client::new();
        for record in &mut records {
            if record.provider == "antigravity"
                && record.project_id().is_none()
                && record.access_token().is_some()
            {
                let token = record.access_token().unwrap_or_default().to_string();
                match crate::auth::antigravity_login::fetch_project_id(&client, &token).await {
                    Ok(pid) => {
                        info!("auto-fetched project_id for {}: {}", record.id, pid);
                        record.metadata.insert("project_id".into(), json!(pid));
                        // Update label if empty
                        if record.label.is_empty() {
                            record.label = pid.clone();
                        }
                        // Persist to disk
                        if let Err(e) = self.store.save(record).await {
                            warn!("failed to persist project_id for {}: {e}", record.id);
                        }
                    }
                    Err(e) => {
                        warn!(
                            "could not auto-fetch project_id for {} (will retry next reload): {e}",
                            record.id
                        );
                    }
                }
            }
        }

        let mut by_provider: HashMap<String, Vec<AuthRecord>> = HashMap::new();
        for record in records {
            if !record.effective_status().is_usable() {
                info!(
                    "skipping {} account: {} (status: {})",
                    record.provider_key,
                    record.id,
                    record.effective_status()
                );
                continue;
            }
            by_provider
                .entry(record.provider_key.clone())
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

    /// Look up a single account by its id (filename).
    pub async fn get_by_id(&self, id: &str) -> Option<AuthRecord> {
        let accounts = self.accounts.read().await;
        for records in accounts.values() {
            if let Some(record) = records.iter().find(|r| r.id == id) {
                return Some(record.clone());
            }
        }
        None
    }

    /// Update an account in memory and persist to disk.
    ///
    /// The `mutate` closure receives a mutable reference to the record.
    /// Returns `true` if the record was found and updated.
    pub async fn update<F>(&self, id: &str, mutate: F) -> AppResult<bool>
    where
        F: FnOnce(&mut AuthRecord),
    {
        let mut accounts = self.accounts.write().await;
        for records in accounts.values_mut() {
            if let Some(record) = records.iter_mut().find(|r| r.id == id) {
                mutate(record);
                // Persist
                self.store.save(record).await?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Update the status of an account by id.
    pub async fn set_status(
        &self,
        id: &str,
        status: AuthStatus,
        message: Option<String>,
    ) -> AppResult<bool> {
        self.update(id, |record| {
            record.status = status;
            record.status_message = message;
            // Sync disabled flag
            record.disabled = record.status == AuthStatus::Disabled;
        })
        .await
    }

    /// Get the backing store (for save/delete operations).
    pub fn store(&self) -> &Arc<FileTokenStore> {
        &self.store
    }
}
