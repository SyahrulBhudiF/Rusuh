use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("upstream error: {0}")]
    Upstream(String),

    #[error("authentication error: {0}")]
    Auth(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("quota exceeded for provider: {0}")]
    QuotaExceeded(String),

    #[error("no available accounts for provider: {0}")]
    NoAccounts(String),

    #[error("internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::Auth(_) => (StatusCode::UNAUTHORIZED, self.to_string()),
            AppError::NotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            AppError::QuotaExceeded(_) | AppError::NoAccounts(_) => {
                (StatusCode::TOO_MANY_REQUESTS, self.to_string())
            }
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = Json(json!({
            "error": {
                "message": message,
                "type": "api_error",
                "code": status.as_u16(),
            }
        }));

        (status, body).into_response()
    }
}

impl AppError {
    /// Whether this error is transient (5xx, timeout, connection) and worth retrying
    /// on the *same* provider.
    pub fn is_transient(&self) -> bool {
        match self {
            AppError::Upstream(msg) => {
                let m = msg.to_lowercase();
                // 5xx status codes
                m.contains("500") || m.contains("502") || m.contains("503")
                    || m.contains("504")
                    // Connection / timeout errors
                    || m.contains("timeout") || m.contains("timed out")
                    || m.contains("connection reset") || m.contains("connection refused")
                    || m.contains("broken pipe") || m.contains("stream read")
            }
            _ => false,
        }
    }

    /// Whether this error is account-specific (401, 429) and should skip to next account.
    pub fn is_account_error(&self) -> bool {
        match self {
            AppError::Auth(_) | AppError::QuotaExceeded(_) => true,
            AppError::Upstream(msg) => {
                let m = msg.to_lowercase();
                m.contains("401")
                    || m.contains("429")
                    || m.contains("unauthorized")
                    || m.contains("rate limit")
                    || m.contains("quota")
            }
            _ => false,
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;
