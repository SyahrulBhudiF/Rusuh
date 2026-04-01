//! GitHub Copilot auth constants, persisted storage helpers, and auth-record builders.

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auth::store::{AuthRecord, AuthStatus, FileTokenStore};
use crate::error::{AppError, AppResult};

/// GitHub OAuth client ID used for Copilot device flow.
pub const CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";
/// GitHub device code endpoint.
pub const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
/// GitHub OAuth token endpoint.
pub const TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
/// GitHub user API endpoint.
pub const USER_URL: &str = "https://api.github.com/user";
/// Copilot API token exchange endpoint.
pub const COPILOT_API_TOKEN_URL: &str = "https://api.github.com/copilot_internal/v2/token";

/// Persisted GitHub OAuth token fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubOAuthTokenData {
    /// GitHub OAuth access token.
    pub access_token: String,
    /// OAuth token type.
    pub token_type: String,
    /// Granted OAuth scopes.
    pub scope: String,
}

/// Persisted GitHub user info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubUserInfo {
    /// GitHub numeric user ID.
    pub id: u64,
    /// GitHub login/username.
    pub login: String,
    /// Primary email when available.
    pub email: Option<String>,
    /// Display name when available.
    pub name: Option<String>,
}

/// Complete auth bundle saved after successful login.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubCopilotAuthBundle {
    /// GitHub OAuth token payload.
    pub token_data: GithubOAuthTokenData,
    /// GitHub account metadata.
    pub user_info: GithubUserInfo,
}

/// Persisted auth file storage shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GithubCopilotTokenStorage {
    /// GitHub OAuth access token.
    pub access_token: String,
    /// OAuth token type.
    pub token_type: String,
    /// Granted OAuth scopes.
    pub scope: String,
    /// GitHub numeric user ID.
    pub user_id: u64,
    /// GitHub login/username.
    pub username: String,
    /// Email when available.
    pub email: Option<String>,
    /// Display name when available.
    pub name: Option<String>,
}

/// Build the canonical filename for a GitHub Copilot auth file.
pub fn credential_file_name(username: &str) -> String {
    format!("github-copilot-{}.json", sanitize_filename_component(username))
}

/// Prefer email for labels, falling back to username.
pub fn preferred_label(user: &GithubUserInfo) -> String {
    user.email
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| user.login.trim())
        .to_string()
}

/// Create persisted storage payload from an auth bundle.
pub fn create_token_storage(bundle: &GithubCopilotAuthBundle) -> GithubCopilotTokenStorage {
    GithubCopilotTokenStorage {
        access_token: bundle.token_data.access_token.clone(),
        token_type: bundle.token_data.token_type.clone(),
        scope: bundle.token_data.scope.clone(),
        user_id: bundle.user_info.id,
        username: bundle.user_info.login.trim().to_string(),
        email: bundle
            .user_info
            .email
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        name: bundle
            .user_info
            .name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
    }
}

/// Build canonical GitHub Copilot auth record for persistence.
pub fn build_github_copilot_auth_record(bundle: &GithubCopilotAuthBundle) -> AppResult<AuthRecord> {
    let username = bundle.user_info.login.trim();
    if username.is_empty() {
        return Err(AppError::Auth(
            "github copilot auth bundle missing username".into(),
        ));
    }

    let label = preferred_label(&bundle.user_info);
    if label.is_empty() {
        return Err(AppError::Auth(
            "github copilot auth bundle missing label".into(),
        ));
    }

    let storage = create_token_storage(bundle);
    let id = credential_file_name(username);
    let now = Utc::now();

    let mut metadata: HashMap<String, Value> = HashMap::from([
        ("type".to_string(), json!("github-copilot")),
        ("provider_key".to_string(), json!("github-copilot")),
        ("label".to_string(), json!(label)),
        ("access_token".to_string(), json!(storage.access_token)),
        ("token_type".to_string(), json!(storage.token_type)),
        ("scope".to_string(), json!(storage.scope)),
        ("user_id".to_string(), json!(storage.user_id)),
        ("username".to_string(), json!(storage.username)),
        ("status".to_string(), json!(AuthStatus::Active.to_string())),
        ("disabled".to_string(), json!(false)),
    ]);

    if let Some(email) = storage.email {
        metadata.insert("email".to_string(), json!(email));
    }

    if let Some(name) = storage.name {
        metadata.insert("name".to_string(), json!(name));
    }

    Ok(AuthRecord {
        id: id.clone(),
        provider: "github-copilot".to_string(),
        provider_key: "github-copilot".to_string(),
        label,
        disabled: false,
        status: AuthStatus::Active,
        status_message: None,
        last_refreshed_at: None,
        path: PathBuf::from(id),
        metadata,
        updated_at: now,
    })
}

/// Persist a GitHub Copilot auth bundle to the file store.
pub async fn save_auth_bundle(
    store: &FileTokenStore,
    bundle: &GithubCopilotAuthBundle,
) -> AppResult<PathBuf> {
    let record = build_github_copilot_auth_record(bundle)?;
    store.save(&record).await
}

fn sanitize_filename_component(value: &str) -> String {
    value.trim()
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            _ => character,
        })
        .collect()
}
