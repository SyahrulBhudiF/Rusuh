use std::collections::BTreeMap;
use std::sync::Arc;

use axum::{routing::get, Json, Router};
use serde::Serialize;
use serde_json::{json, Value};

use crate::auth::store::{AuthRecord, AuthStatus};
use crate::config::{OpenAICompatProvider, ProviderKeyEntry};
use crate::proxy::ProxyState;

#[derive(Debug, Clone, Serialize)]
pub struct DashboardHealth {
    pub status: String,
    pub service: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DashboardOverviewCard {
    pub label: String,
    pub value: String,
    pub hint: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DashboardAccountSummary {
    pub provider: String,
    pub total: usize,
    pub active: usize,
    pub refreshing: usize,
    pub pending: usize,
    pub error: usize,
    pub disabled: usize,
    pub unknown: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DashboardOverview {
    pub health: DashboardHealth,
    pub cards: Vec<DashboardOverviewCard>,
    pub account_summaries: Vec<DashboardAccountSummary>,
    pub available_model_count: usize,
    pub provider_names: Vec<String>,
    pub routing_strategy: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DashboardAuthRecord {
    pub id: String,
    pub provider: String,
    pub label: String,
    pub status: String,
    pub disabled: bool,
    pub status_message: Option<String>,
    pub last_refreshed_at: Option<String>,
    pub updated_at: String,
    pub email: Option<String>,
    pub project_id: Option<String>,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DashboardAccountsPayload {
    pub items: Vec<DashboardAuthRecord>,
    pub grouped_counts: Vec<DashboardAccountSummary>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DashboardApiKeysPayload {
    pub items: Vec<String>,
    pub total: usize,
    pub generated_only: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DashboardManagementConfig {
    pub enabled: bool,
    pub allow_remote: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DashboardProviderKeyEntry {
    pub prefix: Option<String>,
    pub base_url: Option<String>,
    pub model_count: usize,
    pub excluded_model_count: usize,
    pub has_proxy_url: bool,
    pub header_count: usize,
    pub has_api_key: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DashboardOpenAiCompatProvider {
    pub name: String,
    pub prefix: Option<String>,
    pub base_url: String,
    pub model_count: usize,
    pub api_key_entry_count: usize,
    pub header_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DashboardConfigPayload {
    pub host: String,
    pub port: u16,
    pub listen_addr: String,
    pub auth_dir: String,
    pub debug: bool,
    pub request_retry: u32,
    pub routing_strategy: String,
    pub api_key_count: usize,
    pub oauth_alias_channel_count: usize,
    pub oauth_alias_count: usize,
    pub provider_count: usize,
    pub provider_names: Vec<String>,
    pub management: DashboardManagementConfig,
    pub gemini_api_keys: Vec<DashboardProviderKeyEntry>,
    pub codex_api_keys: Vec<DashboardProviderKeyEntry>,
    pub claude_api_keys: Vec<DashboardProviderKeyEntry>,
    pub openai_compat: Vec<DashboardOpenAiCompatProvider>,
}

pub fn router() -> Router<Arc<ProxyState>> {
    Router::new()
        .route("/health", get(health))
        .route("/overview", get(overview))
        .route("/accounts", get(accounts))
        .route("/api-keys", get(api_keys))
        .route("/config", get(config))
}

async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "rusuh"
    }))
}

async fn overview(
    axum::extract::State(state): axum::extract::State<Arc<ProxyState>>,
) -> Json<Value> {
    Json(serde_json::to_value(build_overview(state).await).unwrap_or_else(|_| json!([])))
}

async fn accounts(
    axum::extract::State(state): axum::extract::State<Arc<ProxyState>>,
) -> Json<Value> {
    Json(serde_json::to_value(build_accounts_payload(state).await).unwrap_or_else(|_| json!([])))
}

async fn api_keys(
    axum::extract::State(state): axum::extract::State<Arc<ProxyState>>,
) -> Json<Value> {
    Json(serde_json::to_value(build_api_keys_payload(state).await).unwrap_or_else(|_| json!([])))
}

async fn config(axum::extract::State(state): axum::extract::State<Arc<ProxyState>>) -> Json<Value> {
    Json(serde_json::to_value(build_config_payload(state).await).unwrap_or_else(|_| json!([])))
}

async fn build_overview(state: Arc<ProxyState>) -> DashboardOverview {
    let cfg = state.config.read().await.clone();
    let auth_records = state.accounts.store().list().await.unwrap_or_default();
    let account_summaries = summarize_accounts(&auth_records);
    let total_accounts = auth_records.len();
    let available_model_count = state
        .model_registry
        .get_available_models("openai")
        .await
        .len();
    let provider_names = state
        .current_runtime_snapshot()
        .await
        .providers()
        .iter()
        .map(|provider| provider.name().to_string())
        .collect::<Vec<_>>();
    let routing_strategy = cfg.routing.strategy.clone();
    let api_key_count = cfg.api_keys.len();

    let cards = vec![
        DashboardOverviewCard {
            label: "Server health".to_string(),
            value: "ok".to_string(),
            hint: "Axum server ready".to_string(),
        },
        DashboardOverviewCard {
            label: "Accounts".to_string(),
            value: total_accounts.to_string(),
            hint: "Auth records found on disk".to_string(),
        },
        DashboardOverviewCard {
            label: "API keys".to_string(),
            value: api_key_count.to_string(),
            hint: "Configured client access keys".to_string(),
        },
        DashboardOverviewCard {
            label: "Routing".to_string(),
            value: routing_strategy.clone(),
            hint: format!(
                "{} provider(s), {} model(s)",
                provider_names.len(),
                available_model_count
            ),
        },
    ];

    DashboardOverview {
        health: DashboardHealth {
            status: "ok".to_string(),
            service: "rusuh".to_string(),
        },
        cards,
        account_summaries,
        available_model_count,
        provider_names,
        routing_strategy,
    }
}

async fn build_accounts_payload(state: Arc<ProxyState>) -> DashboardAccountsPayload {
    let auth_records = state.accounts.store().list().await.unwrap_or_default();
    let grouped_counts = summarize_accounts(&auth_records);
    let items = auth_records
        .into_iter()
        .map(map_auth_record)
        .collect::<Vec<_>>();
    let total = items.len();

    DashboardAccountsPayload {
        items,
        grouped_counts,
        total,
    }
}

async fn build_api_keys_payload(state: Arc<ProxyState>) -> DashboardApiKeysPayload {
    let cfg = state.config.read().await;
    let items = cfg.api_keys.clone();
    let generated_only = items.iter().all(|key| key.starts_with("rsk-"));
    let total = items.len();

    DashboardApiKeysPayload {
        items,
        total,
        generated_only,
    }
}

async fn build_config_payload(state: Arc<ProxyState>) -> DashboardConfigPayload {
    let cfg = state.config.read().await.clone();
    let provider_names = state
        .current_runtime_snapshot()
        .await
        .providers()
        .iter()
        .map(|provider| provider.name().to_string())
        .collect::<Vec<_>>();

    DashboardConfigPayload {
        host: cfg.host.clone(),
        port: cfg.port,
        listen_addr: cfg.listen_addr(),
        auth_dir: cfg.auth_dir.clone(),
        debug: cfg.debug,
        request_retry: cfg.request_retry,
        routing_strategy: cfg.routing.strategy.clone(),
        api_key_count: cfg.api_keys.len(),
        oauth_alias_channel_count: cfg.oauth_model_alias.len(),
        oauth_alias_count: cfg.oauth_model_alias.values().map(Vec::len).sum(),
        provider_count: provider_names.len(),
        provider_names,
        management: DashboardManagementConfig {
            enabled: !cfg.remote_management.secret_key.is_empty(),
            allow_remote: cfg.remote_management.allow_remote,
        },
        gemini_api_keys: map_provider_key_entries(&cfg.gemini_api_keys),
        codex_api_keys: map_provider_key_entries(&cfg.codex_api_keys),
        claude_api_keys: map_provider_key_entries(&cfg.claude_api_keys),
        openai_compat: map_openai_compat_providers(&cfg.openai_compat),
    }
}

fn map_auth_record(record: AuthRecord) -> DashboardAuthRecord {
    let status = record.effective_status().to_string();
    let email = record.email().map(ToString::to_string);
    let project_id = record.project_id().map(ToString::to_string);

    DashboardAuthRecord {
        id: record.id,
        provider: record.provider,
        label: record.label,
        status,
        disabled: record.disabled,
        status_message: record.status_message,
        last_refreshed_at: record
            .last_refreshed_at
            .as_ref()
            .map(chrono::DateTime::to_rfc3339),
        updated_at: record.updated_at.to_rfc3339(),
        email,
        project_id,
        path: record.path.display().to_string(),
    }
}

fn summarize_accounts(records: &[AuthRecord]) -> Vec<DashboardAccountSummary> {
    let mut grouped = BTreeMap::<String, DashboardAccountSummary>::new();

    for record in records {
        let summary =
            grouped
                .entry(record.provider.clone())
                .or_insert_with(|| DashboardAccountSummary {
                    provider: record.provider.clone(),
                    total: 0,
                    active: 0,
                    refreshing: 0,
                    pending: 0,
                    error: 0,
                    disabled: 0,
                    unknown: 0,
                });

        summary.total += 1;
        match record.effective_status() {
            AuthStatus::Active => summary.active += 1,
            AuthStatus::Refreshing => summary.refreshing += 1,
            AuthStatus::Pending => summary.pending += 1,
            AuthStatus::Error => summary.error += 1,
            AuthStatus::Disabled => summary.disabled += 1,
            AuthStatus::Unknown => summary.unknown += 1,
        }
    }

    grouped.into_values().collect()
}

fn map_provider_key_entries(entries: &[ProviderKeyEntry]) -> Vec<DashboardProviderKeyEntry> {
    entries
        .iter()
        .map(|entry| DashboardProviderKeyEntry {
            prefix: entry.prefix.clone(),
            base_url: entry.base_url.clone(),
            model_count: entry.models.len(),
            excluded_model_count: entry.excluded_models.len(),
            has_proxy_url: entry.proxy_url.is_some(),
            header_count: entry.headers.len(),
            has_api_key: !entry.api_key.trim().is_empty(),
        })
        .collect()
}

fn map_openai_compat_providers(
    entries: &[OpenAICompatProvider],
) -> Vec<DashboardOpenAiCompatProvider> {
    entries
        .iter()
        .map(|entry| DashboardOpenAiCompatProvider {
            name: entry.name.clone(),
            prefix: entry.prefix.clone(),
            base_url: entry.base_url.clone(),
            model_count: entry.models.len(),
            api_key_entry_count: entry.api_key_entries.len(),
            header_count: entry.headers.len(),
        })
        .collect()
}
