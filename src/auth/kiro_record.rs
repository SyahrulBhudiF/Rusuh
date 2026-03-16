use std::collections::HashMap;
use std::path::PathBuf;

use chrono::Utc;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::auth::kiro::{KiroTokenData, KiroTokenSource};
use crate::auth::store::{AuthRecord, AuthStatus};

pub const KIRO_PROVIDER_KEY: &str = "kiro";

/// Canonical persisted Kiro auth metadata shape.
#[derive(Debug, Clone)]
pub struct KiroRecordInput {
    pub token_data: KiroTokenData,
    pub label: Option<String>,
    pub source: KiroTokenSource,
}

impl KiroRecordInput {
    pub fn into_auth_record(self) -> AuthRecord {
        let now = Utc::now();
        let metadata = build_kiro_metadata(&self.token_data, &self.label, self.source);
        let label = self
            .label
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| default_label(&self.token_data));

        AuthRecord {
            id: build_record_id(&self.token_data.auth_method),
            provider: KIRO_PROVIDER_KEY.to_string(),
            provider_key: KIRO_PROVIDER_KEY.to_string(),
            label,
            disabled: false,
            status: AuthStatus::Active,
            status_message: None,
            last_refreshed_at: Some(now),
            updated_at: now,
            path: PathBuf::new(),
            metadata,
        }
    }
}

pub fn build_kiro_metadata(
    token_data: &KiroTokenData,
    label: &Option<String>,
    source: KiroTokenSource,
) -> HashMap<String, Value> {
    let mut metadata = HashMap::from([
        ("type".to_string(), json!(KIRO_PROVIDER_KEY)),
        ("provider_key".to_string(), json!(KIRO_PROVIDER_KEY)),
        ("access_token".to_string(), json!(token_data.access_token)),
        ("refresh_token".to_string(), json!(token_data.refresh_token)),
        ("profile_arn".to_string(), json!(token_data.profile_arn)),
        ("expires_at".to_string(), json!(token_data.expires_at)),
        ("auth_method".to_string(), json!(token_data.auth_method)),
        ("provider".to_string(), json!(token_data.provider)),
        ("region".to_string(), json!(token_data.region)),
        ("last_refresh".to_string(), json!(Utc::now().to_rfc3339())),
        ("status".to_string(), json!(AuthStatus::Active.to_string())),
        ("disabled".to_string(), json!(false)),
        ("source".to_string(), json!(source.as_str())),
    ]);

    if let Some(client_id) = token_data
        .client_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        metadata.insert("client_id".to_string(), json!(client_id));
    }
    if let Some(client_secret) = token_data
        .client_secret
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        metadata.insert("client_secret".to_string(), json!(client_secret));
    }
    if let Some(start_url) = token_data
        .start_url
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        metadata.insert("start_url".to_string(), json!(start_url));
    }
    if let Some(email) = token_data
        .email
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        metadata.insert("email".to_string(), json!(email));
    }
    if let Some(label) = label.as_deref().filter(|value| !value.trim().is_empty()) {
        metadata.insert("label".to_string(), json!(label.trim()));
    }

    metadata
}

fn build_record_id(auth_method: &str) -> String {
    format!("kiro-{auth_method}-{}.json", Uuid::new_v4())
}

fn default_label(token_data: &KiroTokenData) -> String {
    token_data
        .email
        .as_deref()
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            token_data
                .start_url
                .as_deref()
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| format!("Kiro ({})", token_data.auth_method))
}
