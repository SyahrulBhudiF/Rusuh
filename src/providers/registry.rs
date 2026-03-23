//! Provider registry — builds providers from config + auth store at startup,
//! provides model→provider lookup.

use std::sync::Arc;

use tracing::{info, warn};

use crate::auth::manager::AccountManager;
use crate::config::Config;
use crate::providers::antigravity::AntigravityProvider;
use crate::providers::kiro::KiroProvider;
use crate::providers::model_registry::ModelRegistry;
use crate::providers::zed::ZedProvider;
use crate::providers::Provider;
use crate::proxy::KiroRuntimeState;

/// Build all providers from loaded accounts and config.
pub async fn build_providers(
    _config: &Config,
    accounts: &AccountManager,
    model_registry: Arc<ModelRegistry>,
    kiro_runtime: KiroRuntimeState,
) -> Vec<Arc<dyn Provider>> {
    let mut providers: Vec<Arc<dyn Provider>> = Vec::new();

    // ── Antigravity (OAuth) ──────────────────────────────────────────────
    for record in accounts.accounts_for("antigravity").await {
        info!(
            "registering antigravity provider: {} ({})",
            record.label, record.id
        );
        providers.push(Arc::new(AntigravityProvider::new(record)));
    }

    // ── Kiro (AWS CodeWhisperer) ───────────────────────────────────────────
    for record in accounts.accounts_for("kiro").await {
        let client_id = format!("kiro_{}", providers.len());
        match KiroProvider::new_with_runtime(
            record.clone(),
            client_id,
            model_registry.clone(),
            kiro_runtime.clone(),
        ) {
            Ok(provider) => {
                info!(
                    "registering kiro provider: {} ({})",
                    record.label, record.id
                );
                providers.push(Arc::new(provider));
            }
            Err(e) => {
                warn!(
                    "skipping kiro account {} ({}): {e}",
                    record.label, record.id
                );
            }
        }
    }

    // ── Zed Cloud ────────────────────────────────────────────────────────
    for record in accounts.accounts_for("zed").await {
        match ZedProvider::new(record.clone()) {
            Ok(provider) => {
                info!(
                    "registering zed provider: {} ({})",
                    record.label, record.id
                );
                providers.push(Arc::new(provider));
            }
            Err(e) => {
                warn!(
                    "skipping zed account {} ({}): {e}",
                    record.label, record.id
                );
            }
        }
    }

    // ── Gemini CLI (OAuth) — future ──────────────────────────────────────
    let gemini_count = accounts.accounts_for("gemini").await.len();
    if gemini_count > 0 {
        info!(
            "found {} gemini account(s) — provider not yet implemented",
            gemini_count
        );
    }

    // ── API-key providers from config — future ───────────────────────────
    // gemini-api-key, codex-api-key, claude-api-key, openai-compatibility
    // will be built here when those providers are implemented.

    info!("registered {} total provider(s)", providers.len());
    providers
}
