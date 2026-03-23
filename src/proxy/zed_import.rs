//! Zed credential import and validation.

use anyhow::{Context, Result};
use serde_json::json;
use std::path::Path;

/// Validate Zed credential fields.
///
/// Ensures both `user_id` and `credential_json` are present and non-empty.
///
/// # Errors
/// Returns error if either field is missing or empty.
pub fn validate_zed_credential(user_id: Option<&str>, credential_json: Option<&str>) -> Result<()> {
    if user_id.is_none() || user_id.unwrap().trim().is_empty() {
        anyhow::bail!("user_id is required");
    }
    if credential_json.is_none() || credential_json.unwrap().trim().is_empty() {
        anyhow::bail!("credential_json is required");
    }
    Ok(())
}

/// Import Zed credential to auth file.
///
/// Creates a JSON file with structure:
/// ```json
/// {
///   "type": "zed",
///   "user_id": "...",
///   "credential_json": "..."
/// }
/// ```
///
/// # Errors
/// Returns error if file already exists or write fails.
pub fn import_zed_credential(
    auth_dir: &Path,
    name: &str,
    user_id: &str,
    credential_json: &str,
) -> Result<String> {
    // Auto-add .json extension if missing
    let filename = if name.to_lowercase().ends_with(".json") {
        name.to_string()
    } else {
        format!("{}.json", name)
    };

    let file_path = auth_dir.join(&filename);

    // Prevent overwrite
    if file_path.exists() {
        anyhow::bail!("file already exists: {}", filename);
    }

    let auth_data = json!({
        "type": "zed",
        "user_id": user_id.trim(),
        "credential_json": credential_json.trim(),
    });

    std::fs::write(&file_path, serde_json::to_string_pretty(&auth_data)?)
        .context("failed to write auth file")?;

    Ok(filename)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tempfile::TempDir;

    #[test]
    fn validate_rejects_missing_user_id() {
        assert!(validate_zed_credential(None, Some("cred")).is_err());
        assert!(validate_zed_credential(Some(""), Some("cred")).is_err());
        assert!(validate_zed_credential(Some("  "), Some("cred")).is_err());
    }

    #[test]
    fn validate_rejects_missing_credential_json() {
        assert!(validate_zed_credential(Some("user"), None).is_err());
        assert!(validate_zed_credential(Some("user"), Some("")).is_err());
        assert!(validate_zed_credential(Some("user"), Some("  ")).is_err());
    }

    #[test]
    fn validate_accepts_valid_credentials() {
        assert!(validate_zed_credential(Some("user"), Some("cred")).is_ok());
    }

    #[test]
    fn import_writes_correct_structure() {
        let dir = TempDir::new().unwrap();
        let filename = import_zed_credential(
            dir.path(),
            "test-zed.json",
            "test-user",
            "{\"token\":\"abc\"}",
        )
        .unwrap();

        assert_eq!(filename, "test-zed.json");

        let content: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("test-zed.json")).unwrap(),
        )
        .unwrap();

        assert_eq!(content["type"], "zed");
        assert_eq!(content["user_id"], "test-user");
        assert_eq!(content["credential_json"], "{\"token\":\"abc\"}");
    }

    #[test]
    fn import_auto_adds_json_extension() {
        let dir = TempDir::new().unwrap();
        let filename = import_zed_credential(dir.path(), "test-zed", "user", "cred").unwrap();

        assert_eq!(filename, "test-zed.json");
        assert!(dir.path().join("test-zed.json").exists());
    }

    #[test]
    fn import_prevents_overwrite() {
        let dir = TempDir::new().unwrap();
        import_zed_credential(dir.path(), "test-zed.json", "user1", "cred1").unwrap();

        let result = import_zed_credential(dir.path(), "test-zed.json", "user2", "cred2");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }
}
