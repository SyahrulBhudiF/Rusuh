//! Filesystem-backed token store.
//!
//! Reads/writes auth JSON files from `auth-dir` per provider.
//! Each file is a JSON object with at minimum a `"type"` field identifying the provider.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;
use tracing::{debug, warn};

use crate::error::{AppError, AppResult};

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
    /// Whether this credential is disabled
    pub disabled: bool,
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

    async fn read_auth_file(
        &self,
        path: &Path,
        base_dir: &Path,
    ) -> AppResult<Option<AuthRecord>> {
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
