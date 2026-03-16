//! Antigravity OAuth constants — mirrors Go `internal/auth/antigravity/constants.go`.

pub const CLIENT_ID: &str =
    "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
pub const CLIENT_SECRET: &str = "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";
pub const CALLBACK_PORT: u16 = 51121;

pub const SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/cloud-platform",
    "https://www.googleapis.com/auth/userinfo.email",
    "https://www.googleapis.com/auth/userinfo.profile",
    "https://www.googleapis.com/auth/cclog",
    "https://www.googleapis.com/auth/experimentsandconfigs",
];

pub const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
pub const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const USERINFO_ENDPOINT: &str = "https://www.googleapis.com/oauth2/v1/userinfo?alt=json";

pub const API_ENDPOINT: &str = "https://cloudcode-pa.googleapis.com";
pub const API_VERSION: &str = "v1internal";
pub const API_USER_AGENT: &str = "google-api-nodejs-client/9.15.1";
pub const API_CLIENT: &str = "google-cloud-sdk vscode_cloudshelleditor/0.1";
pub const CLIENT_METADATA: &str =
    r#"{"ideType":"IDE_UNSPECIFIED","platform":"PLATFORM_UNSPECIFIED","pluginType":"GEMINI"}"#;
