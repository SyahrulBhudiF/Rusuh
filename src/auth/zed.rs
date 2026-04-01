//! Zed auth parsing and credential handling.
//!
//! Zed auth files contain:
//! - `user_id`: string identifier for the Zed user
//! - `credential_json`: JSON string containing the actual credential data

use anyhow::{Context, Result};
use serde_json::Value;

/// Parse Zed credential from auth file metadata.
///
/// Extracts `user_id` and `credential_json` fields, both must be strings.
///
/// # Errors
/// Returns error if either field is missing or not a string.
pub fn parse_zed_credential(json: &Value) -> Result<(String, String)> {
    let user_id = json
        .get("user_id")
        .and_then(|v| v.as_str())
        .context("missing or invalid user_id field")?
        .to_string();

    let credential_json = json
        .get("credential_json")
        .and_then(|v| v.as_str())
        .context("missing or invalid credential_json field")?
        .to_string();

    Ok((user_id, credential_json))
}

/// Extract label for Zed auth record.
///
/// Returns the user_id unchanged as the label.
pub fn extract_zed_label(user_id: &str, _credential_json: &str) -> String {
    user_id.to_string()
}

/// Generate canonical filename for Zed login auth file.
///
/// Format: `zed-login-{sanitized_user_id}.json`
///
/// Path-illegal characters (`/`, `\`, `:`, `*`, `?`, `<`, `>`, `|`) are replaced with `-`.
pub fn canonical_zed_login_filename(user_id: &str) -> String {
    let sanitized = user_id
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '<' | '>' | '|' => '-',
            _ => c,
        })
        .collect::<String>();

    format!("zed-login-{}.json", sanitized)
}

/// Check if two Zed user IDs match.
///
/// On Windows, comparison is case-insensitive.
/// On other platforms, comparison is exact.
#[cfg(target_os = "windows")]
pub fn zed_user_ids_match(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

#[cfg(not(target_os = "windows"))]
pub fn zed_user_ids_match(left: &str, right: &str) -> bool {
    left == right
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_valid() {
        let json = json!({
            "user_id": "test-user",
            "credential_json": "{\"token\":\"abc\"}"
        });
        let (user_id, cred) = parse_zed_credential(&json).unwrap();
        assert_eq!(user_id, "test-user");
        assert_eq!(cred, "{\"token\":\"abc\"}");
    }

    #[test]
    fn test_extract_label_returns_user_id() {
        let label = extract_zed_label("user@example.com", "{}");
        assert_eq!(label, "user@example.com");
    }

    #[test]
    fn test_canonical_filename_sanitizes() {
        assert_eq!(
            canonical_zed_login_filename("user@example.com"),
            "zed-login-user@example.com.json"
        );
        assert_eq!(
            canonical_zed_login_filename("user/name\\test:file*name?<>|"),
            "zed-login-user-name-test-file-name----.json"
        );
    }
}
