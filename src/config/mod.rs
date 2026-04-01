use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Top-level server configuration (mirrors config.example.yaml)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Host to bind (empty = all interfaces)
    pub host: String,
    /// Port to listen on
    pub port: u16,
    /// Auth directory for storing OAuth tokens
    #[serde(rename = "auth-dir")]
    pub auth_dir: String,
    /// API keys for incoming request authentication
    #[serde(rename = "api-keys")]
    pub api_keys: Vec<String>,
    /// Enable debug logging
    pub debug: bool,
    /// Number of request retries
    #[serde(rename = "request-retry")]
    pub request_retry: u32,
    /// Routing strategy
    pub routing: RoutingConfig,
    /// Proxy URL (socks5/http/https)
    #[serde(rename = "proxy-url")]
    pub proxy_url: Option<String>,
    /// TLS configuration
    pub tls: TlsConfig,
    /// Management API settings
    #[serde(rename = "remote-management")]
    pub remote_management: ManagementConfig,

    // Provider configurations
    #[serde(rename = "gemini-api-key")]
    pub gemini_api_keys: Vec<ProviderKeyEntry>,
    #[serde(rename = "codex-api-key")]
    pub codex_api_keys: Vec<ProviderKeyEntry>,
    #[serde(rename = "claude-api-key")]
    pub claude_api_keys: Vec<ProviderKeyEntry>,
    #[serde(rename = "openai-compatibility")]
    pub openai_compat: Vec<OpenAICompatProvider>,
    #[serde(rename = "oauth-model-alias", default = "default_oauth_model_alias")]
    pub oauth_model_alias: HashMap<String, Vec<ModelAlias>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TlsConfig {
    pub enable: bool,
    pub cert: String,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ManagementConfig {
    #[serde(rename = "allow-remote")]
    pub allow_remote: bool,
    #[serde(rename = "secret-key")]
    pub secret_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RoutingConfig {
    /// "round-robin" (default) or "fill-first"
    pub strategy: String,
}

/// A provider API key entry with optional prefix, base URL, model aliases.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ProviderKeyEntry {
    #[serde(rename = "api-key")]
    pub api_key: String,
    /// Optional routing prefix (e.g. "test" → "test/model-name")
    pub prefix: Option<String>,
    #[serde(rename = "base-url")]
    pub base_url: Option<String>,
    pub models: Vec<ModelEntry>,
    #[serde(rename = "excluded-models")]
    pub excluded_models: Vec<String>,
    #[serde(rename = "proxy-url")]
    pub proxy_url: Option<String>,
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelEntry {
    pub name: String,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct OpenAICompatProvider {
    pub name: String,
    pub prefix: Option<String>,
    #[serde(rename = "base-url")]
    pub base_url: String,
    pub headers: HashMap<String, String>,
    #[serde(rename = "api-key-entries")]
    pub api_key_entries: Vec<OpenAICompatKeyEntry>,
    pub models: Vec<ModelEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct OpenAICompatKeyEntry {
    #[serde(rename = "api-key")]
    pub api_key: String,
    #[serde(rename = "proxy-url")]
    pub proxy_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelAlias {
    pub name: String,
    pub alias: String,
    #[serde(default)]
    pub fork: bool,
}

fn default_oauth_model_alias() -> HashMap<String, Vec<ModelAlias>> {
    HashMap::from([(
        "github-copilot".to_string(),
        vec![
            ModelAlias {
                name: "claude-sonnet-4-5".to_string(),
                alias: "claude-sonnet-4.5".to_string(),
                fork: false,
            },
            ModelAlias {
                name: "claude-sonnet-4-6".to_string(),
                alias: "claude-sonnet-4.6".to_string(),
                fork: false,
            },
            ModelAlias {
                name: "claude-opus-4-5".to_string(),
                alias: "claude-opus-4.5".to_string(),
                fork: false,
            },
            ModelAlias {
                name: "claude-opus-4-6".to_string(),
                alias: "claude-opus-4.6".to_string(),
                fork: false,
            },
        ],
    )])
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 0,
            auth_dir: String::new(),
            api_keys: Vec::new(),
            debug: false,
            request_retry: 0,
            routing: RoutingConfig::default(),
            proxy_url: None,
            tls: TlsConfig::default(),
            remote_management: ManagementConfig::default(),
            gemini_api_keys: Vec::new(),
            codex_api_keys: Vec::new(),
            claude_api_keys: Vec::new(),
            openai_compat: Vec::new(),
            oauth_model_alias: default_oauth_model_alias(),
        }
    }
}

impl Config {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let cfg: Config = serde_yaml::from_str(&content)?;
        Ok(cfg)
    }

    pub fn load_optional(path: &str) -> anyhow::Result<Option<Self>> {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                let cfg: Config = serde_yaml::from_str(&content)?;
                Ok(Some(cfg))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn listen_addr(&self) -> String {
        let host = if self.host.is_empty() {
            "0.0.0.0"
        } else {
            &self.host
        };
        let port = if self.port == 0 { 8317 } else { self.port };
        format!("{}:{}", host, port)
    }
}
