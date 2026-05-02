use axum::http::StatusCode;
use axum::response::IntoResponse;
use rusuh::error::AppError;

#[test]
fn transient_errors_detected() {
    assert!(AppError::Upstream("502 Bad Gateway".into()).is_transient());
    assert!(AppError::Upstream("503 Service Unavailable".into()).is_transient());
    assert!(AppError::Upstream("504 Gateway Timeout".into()).is_transient());
    assert!(AppError::Upstream("timeout waiting for response".into()).is_transient());
    assert!(AppError::Upstream("connection reset by peer".into()).is_transient());
    assert!(AppError::Upstream("connection refused".into()).is_transient());
    assert!(AppError::Upstream("stream read error: broken".into()).is_transient());
}

#[test]
fn non_transient_errors() {
    assert!(!AppError::Upstream("400 Bad Request".into()).is_transient());
    assert!(!AppError::Upstream("invalid model".into()).is_transient());
    assert!(!AppError::Auth("bad token".into()).is_transient());
    assert!(!AppError::BadRequest("missing field".into()).is_transient());
}

#[test]
fn account_errors_detected() {
    assert!(AppError::Auth("unauthorized".into()).is_account_error());
    assert!(AppError::QuotaExceeded("gemini".into()).is_account_error());
    assert!(AppError::Upstream("401 Unauthorized".into()).is_account_error());
    assert!(AppError::Upstream("429 Too Many Requests".into()).is_account_error());
    assert!(AppError::Upstream("rate limit exceeded".into()).is_account_error());
    assert!(AppError::Upstream("quota exceeded for user".into()).is_account_error());
    assert!(AppError::Upstream("403 no_access_on_free_plan".into()).is_account_error());
}

#[test]
fn non_account_errors() {
    assert!(!AppError::Upstream("500 Internal Server Error".into()).is_account_error());
    assert!(!AppError::BadRequest("bad".into()).is_account_error());
    assert!(!AppError::Config("missing".into()).is_account_error());
}

#[test]
fn error_response_status_codes() {
    let resp = AppError::Auth("x".into()).into_response();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let resp = AppError::BadRequest("x".into()).into_response();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let resp = AppError::NotFound("x".into()).into_response();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let resp = AppError::NoAccounts("x".into()).into_response();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

    let resp = AppError::QuotaExceeded("x".into()).into_response();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

    let resp = AppError::Upstream("x".into()).into_response();
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[test]
fn error_display_messages() {
    let e = AppError::Upstream("test".into());
    assert_eq!(e.to_string(), "upstream error: test");

    let e = AppError::Auth("bad".into());
    assert_eq!(e.to_string(), "authentication error: bad");
}
