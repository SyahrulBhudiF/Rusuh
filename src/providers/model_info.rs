//! Extended model info types matching Go `registry.ModelInfo` and `registry.ThinkingSupport`.

use serde::{Deserialize, Serialize};

/// Extended model metadata, matching the Go ModelInfo struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtModelInfo {
    pub id: String,
    #[serde(default = "default_object")]
    pub object: String,
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub owned_by: String,
    /// Provider type key (e.g. "antigravity", "claude", "gemini", "openai")
    #[serde(rename = "type", default)]
    pub provider_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub input_token_limit: i32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub output_token_limit: i32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supported_generation_methods: Vec<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub context_length: i32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub max_completion_tokens: i32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supported_parameters: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingSupport>,
    /// True if defined via config file (not static).
    #[serde(skip)]
    pub user_defined: bool,
}

fn default_object() -> String {
    "model".into()
}
fn is_zero(v: &i32) -> bool {
    *v == 0
}

impl ExtModelInfo {
    /// Convert to the simple `ModelInfo` used in OpenAI-compatible responses.
    pub fn to_simple(&self) -> crate::models::ModelInfo {
        crate::models::ModelInfo {
            id: self.id.clone(),
            object: "model".into(),
            created: self.created,
            owned_by: self.owned_by.clone(),
        }
    }
}

/// Thinking/reasoning budget capabilities for a model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingSupport {
    #[serde(default)]
    pub min: i32,
    #[serde(default)]
    pub max: i32,
    #[serde(default)]
    pub zero_allowed: bool,
    #[serde(default)]
    pub dynamic_allowed: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub levels: Vec<String>,
}
