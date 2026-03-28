//! Codex shared auth types, constants, storage helpers, and auth-record builders.

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::auth::store::{AuthRecord, AuthStatus, FileTokenStore};
use crate::error::{AppError, AppResult};

/// OAuth token endpoint.
pub const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
/// Codex OAuth client ID.
pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
/// Default callback URL used by local login flow.
pub const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
/// Refresh policy lead time for Codex credentials (5 days).
pub const REFRESH_LEAD_DAYS: i64 = 5;

/// Runtime token data returned from OAuth exchange/refresh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexTokenData {
    /// ID token used for account claim extraction.
    pub id_token: String,
    /// Access token used for upstream API calls.
    pub access_token: String,
    /// Refresh token used to rotate access tokens.
    pub refresh_token: String,
    /// Codex/ChatGPT account identifier.
    pub account_id: String,
    /// Account email.
    pub email: String,
    /// Absolute expiry string (upstream-compatible field name).
    pub expired: String,
}

/// Login/exchange bundle with token payload and refresh timestamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexAuthBundle {
    /// Token payload.
    pub token_data: CodexTokenData,
    /// Last refresh timestamp (RFC3339).
    pub last_refresh: String,
}

/// Persisted token storage shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexTokenStorage {
    /// ID token.
    pub id_token: String,
    /// Access token.
    pub access_token: String,
    /// Refresh token.
    pub refresh_token: String,
    /// Account identifier.
    pub account_id: String,
    /// Last refresh timestamp.
    pub last_refresh: String,
    /// Email address.
    pub email: String,
    /// Absolute expiry timestamp string.
    pub expired: String,
}

/// Create storage payload from a login bundle.
pub fn create_token_storage(bundle: &CodexAuthBundle) -> CodexTokenStorage {
    CodexTokenStorage {
        id_token: bundle.token_data.id_token.clone(),
        access_token: bundle.token_data.access_token.clone(),
        refresh_token: bundle.token_data.refresh_token.clone(),
        account_id: bundle.token_data.account_id.clone(),
        last_refresh: bundle.last_refresh.clone(),
        email: bundle.token_data.email.clone(),
        expired: bundle.token_data.expired.clone(),
    }
}

/// Update existing storage with refreshed token values.
pub fn update_token_storage(
    storage: &mut CodexTokenStorage,
    token_data: &CodexTokenData,
    last_refresh: &str,
) {
    storage.id_token = token_data.id_token.clone();
    storage.access_token = token_data.access_token.clone();
    storage.refresh_token = token_data.refresh_token.clone();
    storage.account_id = token_data.account_id.clone();
    storage.email = token_data.email.clone();
    storage.expired = token_data.expired.clone();
    storage.last_refresh = last_refresh.to_string();
}

/// Build canonical Codex auth record for persistence.
pub fn build_codex_auth_record(
    bundle: &CodexAuthBundle,
    plan_type: Option<&str>,
    account_hash: Option<&str>,
    include_provider_prefix: bool,
) -> AppResult<AuthRecord> {
    let token = &bundle.token_data;
    let email = token.email.trim();
    if email.is_empty() {
        return Err(AppError::Auth("codex auth bundle missing email".into()));
    }

    let normalized_plan = plan_type
        .map(normalize_plan_type_for_filename)
        .filter(|value| !value.is_empty());
    let hash = account_hash
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| hash_account_id_short(&token.account_id));

    let id = credential_file_name(
        email,
        normalized_plan.as_deref().unwrap_or(""),
        &hash,
        include_provider_prefix,
    );

    let now = Utc::now();
    let last_refreshed_at = DateTime::parse_from_rfc3339(&bundle.last_refresh)
        .ok()
        .map(|value| value.with_timezone(&Utc))
        .or(Some(now));

    let mut metadata: HashMap<String, Value> = HashMap::from([
        ("type".to_string(), json!("codex")),
        ("provider_key".to_string(), json!("codex")),
        ("id_token".to_string(), json!(token.id_token)),
        ("access_token".to_string(), json!(token.access_token)),
        ("refresh_token".to_string(), json!(token.refresh_token)),
        ("account_id".to_string(), json!(token.account_id)),
        ("email".to_string(), json!(email)),
        ("expired".to_string(), json!(token.expired)),
        ("last_refresh".to_string(), json!(bundle.last_refresh)),
        ("status".to_string(), json!(AuthStatus::Active.to_string())),
        ("disabled".to_string(), json!(false)),
    ]);

    if let Some(plan) = normalized_plan {
        metadata.insert("plan_type".to_string(), json!(plan));
    }

    Ok(AuthRecord {
        id: id.clone(),
        provider: "codex".to_string(),
        provider_key: "codex".to_string(),
        label: email.to_string(),
        disabled: false,
        status: AuthStatus::Active,
        status_message: None,
        last_refreshed_at,
        path: PathBuf::from(id),
        metadata,
        updated_at: now,
    })
}

/// Persist a Codex auth bundle to the file store.
pub async fn save_auth_bundle(
    store: &FileTokenStore,
    bundle: &CodexAuthBundle,
    include_provider_prefix: bool,
) -> AppResult<std::path::PathBuf> {
    let claims = crate::auth::codex_login::parse_jwt_token(&bundle.token_data.id_token).ok();
    let plan_type = claims
        .as_ref()
        .and_then(crate::auth::codex_login::get_plan_type);
    let account_hash = claims
        .as_ref()
        .and_then(crate::auth::codex_login::get_account_id)
        .map(hash_account_id_short);

    let fallback_email = bundle.token_data.email.trim();
    let chosen_email = claims
        .as_ref()
        .and_then(crate::auth::codex_login::get_user_email)
        .unwrap_or(fallback_email)
        .trim()
        .to_string();

    if chosen_email.is_empty() {
        return Err(AppError::Auth("codex auth bundle missing email".into()));
    }

    let mut adjusted_bundle = bundle.clone();
    adjusted_bundle.token_data.email = chosen_email;

    let record = build_codex_auth_record(
        &adjusted_bundle,
        plan_type,
        account_hash.as_deref(),
        include_provider_prefix,
    )?;

    store.save(&record).await
}

/// Compute short hash used for team-plan filename disambiguation.
pub fn hash_account_id_short(account_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(account_id.trim().as_bytes());
    let digest = hasher.finalize();

    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }

    hex.chars().take(8).collect()
}

/// Build Codex credential filename.
pub fn credential_file_name(
    email: &str,
    plan_type: &str,
    hash_account_id: &str,
    include_provider_prefix: bool,
) -> String {
    let email = email.trim();
    let plan = normalize_plan_type_for_filename(plan_type);

    let prefix = if include_provider_prefix { "codex" } else { "" };
    let with_prefix = |rest: &str| {
        if prefix.is_empty() {
            rest.to_string()
        } else {
            format!("{prefix}-{rest}")
        }
    };

    if plan.is_empty() {
        return with_prefix(&format!("{email}.json"));
    }

    if plan == "team" {
        return with_prefix(&format!("{hash_account_id}-{email}-{plan}.json"));
    }

    with_prefix(&format!("{email}-{plan}.json"))
}

/// Normalize plan type for filename usage.
pub fn normalize_plan_type_for_filename(plan_type: &str) -> String {
    let mut parts = Vec::new();
    let mut buf = String::new();

    for ch in plan_type.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            buf.push(ch.to_ascii_lowercase());
        } else if !buf.is_empty() {
            parts.push(std::mem::take(&mut buf));
        }
    }

    if !buf.is_empty() {
        parts.push(buf);
    }

    parts.join("-")
}
