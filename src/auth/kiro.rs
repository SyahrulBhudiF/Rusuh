//! KIRO (AWS CodeWhisperer) OAuth constants and types.
//!
//! KIRO uses AWS SSO OIDC authentication with multiple auth methods:
//! - Builder ID: Device code flow via AWS SSO OIDC
//! - Social: Google/GitHub OAuth via Kiro auth endpoint
//! - Enterprise IDC: Authorization code flow via AWS Identity Center
//!
//! Mirrors Go implementation from CLIProxyAPIPlus internal/auth/kiro.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── OAuth Endpoints ──────────────────────────────────────────────────────────

/// AWS SSO OIDC endpoint for Builder ID and Enterprise IDC auth
pub const SSO_OIDC_ENDPOINT: &str = "https://oidc.us-east-1.amazonaws.com";

/// Kiro auth endpoint for social OAuth (Google/GitHub)
pub const KIRO_AUTH_ENDPOINT: &str = "https://prod.us-east-1.auth.desktop.kiro.dev";

/// Builder ID start URL for device code flow
pub const BUILDER_ID_START_URL: &str = "https://view.awsapps.com/start";

/// Default AWS region for OIDC operations
pub const DEFAULT_REGION: &str = "us-east-1";

/// Callback port for OAuth flows
pub const CALLBACK_PORT: u16 = 9876;

/// Refresh skew — refresh token 50 minutes before expiry (matching antigravity)
pub const REFRESH_SKEW_SECS: i64 = 3000;

// ── OAuth Scopes ─────────────────────────────────────────────────────────────

/// CodeWhisperer OAuth scopes
pub const SCOPES: &[&str] = &[
    "codewhisperer:completions",
    "codewhisperer:analysis",
    "codewhisperer:conversations",
    "codewhisperer:transformations",
    "codewhisperer:taskassist",
];

// ── Token Data ───────────────────────────────────────────────────────────────

/// KIRO token data — runtime representation of OAuth tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroTokenData {
    /// OAuth2 access token for API access
    pub access_token: String,
    /// Refresh token for obtaining new access tokens
    pub refresh_token: String,
    /// AWS CodeWhisperer profile ARN
    pub profile_arn: String,
    /// Token expiry timestamp (RFC3339)
    pub expires_at: String,
    /// Authentication method: "builder-id", "social", or "idc"
    pub auth_method: String,
    /// OAuth provider: "AWS", "Google", "GitHub", or "Enterprise"
    pub provider: String,
    /// OIDC client ID (for IDC auth)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// OIDC client secret (for IDC auth)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    /// AWS region for OIDC operations
    #[serde(default = "default_region")]
    pub region: String,
    /// IDC start URL (for Enterprise IDC auth)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_url: Option<String>,
    /// User email address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

fn default_region() -> String {
    DEFAULT_REGION.to_string()
}

impl KiroTokenData {
    /// Check whether the token needs refreshing (expired or within skew window).
    pub fn needs_refresh(&self) -> bool {
        if self.access_token.is_empty() {
            return true;
        }
        let expires_at = match parse_expiry_str(&self.expires_at) {
            Some(dt) => dt,
            None => return true,
        };
        let now = Utc::now();
        let deadline = expires_at - chrono::Duration::seconds(REFRESH_SKEW_SECS);
        now >= deadline
    }
}

// ── Auth Bundle ──────────────────────────────────────────────────────────────

/// Complete authentication bundle after OAuth flow completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroAuthBundle {
    /// OAuth tokens and metadata
    pub token_data: KiroTokenData,
    /// Last successful refresh timestamp (RFC3339)
    pub last_refresh: String,
}

// ── Helper Functions ─────────────────────────────────────────────────────────

/// Parse expiry timestamp from RFC3339 string.
pub fn parse_expiry_str(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Parse expiry from metadata HashMap (checks "expires_at" field).
pub fn parse_expiry(metadata: &HashMap<String, Value>) -> DateTime<Utc> {
    if let Some(Value::String(s)) = metadata.get("expires_at") {
        if let Some(dt) = parse_expiry_str(s) {
            return dt;
        }
    }
    // Default to epoch if missing/invalid
    DateTime::from_timestamp(0, 0).unwrap_or_else(Utc::now)
}
