//! Provider registry — builds providers from config + auth store at startup,
//! provides model→provider lookup.

use std::sync::Arc;

use tracing::info;

use crate::auth::manager::AccountManager;
use crate::config::Config;
use crate::providers::antigravity::AntigravityProvider;
use crate::providers::Provider;

/// Build all providers from loaded accounts and config.
pub async fn build_providers(
    _config: &Config,
    accounts: &AccountManager,
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