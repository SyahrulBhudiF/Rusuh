//! Filesystem-backed token store.
//!
//! Reads/writes auth JSON files from `auth-dir` per provider.
//! Each file is a JSON object with at minimum a `"type"` field identifying the provider.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;
use tracing::{debug, warn};

use crate::error::{AppError, AppResult};

// ── AuthStatus ───────────────────────────────────────────────────────────────

/// Lifecycle state of an auth entry (matches Go `cliproxyauth.Status`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AuthStatus {
    /// State could not be determined.
    Unknown,
    /// Valid and ready for execution.
    #[default]
    Active,
    /// Waiting for external action (e.g. MFA).
    Pending,
    /// Undergoing a refresh flow.
    Refreshing,
    /// Temporarily unavailable due to errors.
    Error,
    /// Intentionally disabled by the user.
    Disabled,
}

impl fmt::Display for AuthStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown => write!(f, "unknown"),
            Self::Active => write!(f, "active"),
            Self::Pending => write!(f, "pending"),
            Self::Refreshing => write!(f, "refreshing"),
            Self::Error => write!(f, "error"),
            Self::Disabled => write!(f, "disabled"),
        }
    }
}

impl AuthStatus {
    /// Parse from string (case-insensitive). Unknown values → `Unknown`.
    pub fn from_str_loose(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "active" => Self::Active,
            "pending" => Self::Pending,
            "refreshing" => Self::Refreshing,
            "error" => Self::Error,
            "disabled" => Self::Disabled,
            _ => Self::Unknown,
        }
    }

    /// Whether this status allows the credential to be used for requests.
    pub fn is_usable(&self) -> bool {
        matches!(self, Self::Active | Self::Refreshing)
    }
}

// ── AuthRecord ───────────────────────────────────────────────────────────────

/// A single credential loaded from an auth JSON file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthRecord {
    /// Relative path from auth-dir (e.g. `antigravity-user@gmail.com.json`)
    pub id: String,
    /// Provider key: `antigravity`, `gemini`, `codex`, `claude`, `qwen`, `iflow`
    pub provider: String,
    /// Human-readable label (email, project_id, etc.)
    pub label: String,
    /// Whether this credential is disabled (legacy field, prefer `status`)
    pub disabled: bool,
    /// Lifecycle status of this credential
    pub status: AuthStatus,
    /// Optional status message (e.g. error reason)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
    /// Last successful token refresh time (UTC)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_refreshed_at: Option<DateTime<Utc>>,
    /// File path on disk
    pub path: PathBuf,
    /// Raw metadata from the JSON file
    pub metadata: HashMap<String, Value>,
    /// File modification time
    pub updated_at: DateTime<Utc>,
}

impl AuthRecord {
    /// Extract the access token from metadata.
    /// Checks top-level `access_token`, then `token.access_token`.
    pub fn access_token(&self) -> Option<&str> {
        if let Some(Value::String(s)) = self.metadata.get("access_token") {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
        if let Some(Value::Object(token)) = self.metadata.get("token") {
            if let Some(Value::String(s)) = token.get("access_token") {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed);
                }
            }
        }
        None
    }

    /// Extract email from metadata.
    pub fn email(&self) -> Option<&str> {
        if let Some(Value::String(s)) = self.metadata.get("email") {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
        None
    }

    /// Extract project_id from metadata.
    pub fn project_id(&self) -> Option<&str> {
        if let Some(Value::String(s)) = self.metadata.get("project_id") {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
        None
    }

    /// Derive effective status from the `disabled` flag and `status` field.
    /// If `disabled` is true, overrides to `Disabled` regardless of `status`.
    pub fn effective_status(&self) -> AuthStatus {
        if self.disabled {
            AuthStatus::Disabled
        } else {
            self.status.clone()
        }
    }
}

// ── FileTokenStore ───────────────────────────────────────────────────────────

/// Persists and enumerates auth records using the filesystem.
pub struct FileTokenStore {
    base_dir: RwLock<PathBuf>,
}

impl FileTokenStore {
    /// Create a new store. `base_dir` is typically `~/.rusuh` or config `auth-dir`.
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: RwLock::new(base_dir.into()),
        }
    }

    /// Update the base directory.
    pub async fn set_base_dir(&self, dir: impl Into<PathBuf>) {
        *self.base_dir.write().await = dir.into();
    }

    /// Get the current base directory.
    pub async fn base_dir(&self) -> PathBuf {
        self.base_dir.read().await.clone()
    }

    /// List all auth records found under `base_dir`.
    pub async fn list(&self) -> AppResult<Vec<AuthRecord>> {
        let dir = self.base_dir.read().await.clone();
        if !dir.exists() {
            debug!("auth dir does not exist: {}", dir.display());
            return Ok(Vec::new());
        }

        let mut records = Vec::new();
        let mut entries = tokio::fs::read_dir(&dir)
            .await
            .map_err(|e| AppError::Config(format!("read auth dir: {e}")))?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| AppError::Config(format!("read auth entry: {e}")))?
        {
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            match self.read_auth_file(&path, &dir).await {
                Ok(Some(record)) => records.push(record),
                Ok(None) => {}
                Err(e) => {
                    warn!("skip auth file {}: {e}", path.display());
                }
            }
        }

        Ok(records)
    }

    /// Save an auth record (metadata) to the resolved path.
    pub async fn save(&self, record: &AuthRecord) -> AppResult<PathBuf> {
        let dir = self.base_dir.read().await.clone();
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|e| AppError::Config(format!("create auth dir: {e}")))?;

        let path = if record.path.is_absolute() {
            record.path.clone()
        } else {
            dir.join(&record.id)
        };

        let json = serde_json::to_string_pretty(&record.metadata)
            .map_err(|e| AppError::Config(format!("serialize auth: {e}")))?;

        tokio::fs::write(&path, json)
            .await
            .map_err(|e| AppError::Config(format!("write auth file: {e}")))?;

        // Restrict permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            tokio::fs::set_permissions(&path, perms)
                .await
                .map_err(|e| AppError::Config(format!("chmod auth file: {e}")))?;
        }

        debug!("saved auth file: {}", path.display());
        Ok(path)
    }

    /// Delete an auth file by id (relative filename).
    pub async fn delete(&self, id: &str) -> AppResult<()> {
        let dir = self.base_dir.read().await.clone();
        let path = dir.join(id);
        if path.exists() {
            tokio::fs::remove_file(&path)
                .await
                .map_err(|e| AppError::Config(format!("delete auth file: {e}")))?;
        }
        Ok(())
    }

    // ── Internal ─────────────────────────────────────────────────────────

    async fn read_auth_file(&self, path: &Path, base_dir: &Path) -> AppResult<Option<AuthRecord>> {
        let data = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| AppError::Config(format!("read {}: {e}", path.display())))?;

        if data.trim().is_empty() {
            return Ok(None);
        }

        let metadata: HashMap<String, Value> = serde_json::from_str(&data)
            .map_err(|e| AppError::Config(format!("parse {}: {e}", path.display())))?;

        let provider = metadata
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let disabled = metadata
            .get("disabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Derive status from metadata or disabled flag
        let status = if disabled {
            AuthStatus::Disabled
        } else {
            metadata
                .get("status")
                .and_then(|v| v.as_str())
                .map(AuthStatus::from_str_loose)
                .unwrap_or(AuthStatus::Active)
        };

        let status_message = metadata
            .get("status_message")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let last_refreshed_at = metadata
            .get("last_refreshed_at")
            .and_then(|v| v.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        let label = Self::extract_label(&metadata);

        let id = path
            .strip_prefix(base_dir)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        let file_meta = tokio::fs::metadata(path)
            .await
            .map_err(|e| AppError::Config(format!("stat {}: {e}", path.display())))?;

        let updated_at = file_meta
            .modified()
            .map(DateTime::<Utc>::from)
            .unwrap_or_else(|_| Utc::now());

        Ok(Some(AuthRecord {
            id,
            provider,
            label,
            disabled,
            status,
            status_message,
            last_refreshed_at,
            path: path.to_path_buf(),
            metadata,
            updated_at,
        }))
    }

    fn extract_label(metadata: &HashMap<String, Value>) -> String {
        for key in &["label", "email", "project_id"] {
            if let Some(Value::String(s)) = metadata.get(*key) {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
        }
        String::new()
    }
}
