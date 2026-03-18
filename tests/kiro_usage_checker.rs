//! Tests for Kiro usage checker — mirrors
//! `CLIProxyAPIPlus/internal/auth/kiro/usage_checker_test.go` behavior.

use rusuh::auth::kiro_runtime::{KiroUsageChecker, QuotaStatus, UsageCheckRequest};

/// Mock HTTP server response for testing.
fn mock_usage_response_available() -> &'static str {
    r#"{
        "usageBreakdownList": [
            {
                "resourceType": "AGENTIC_REQUEST",
                "usageLimitWithPrecision": 100.0,
                "currentUsageWithPrecision": 30.0
            }
        ],
        "nextDateReset": 1710777600000
    }"#
}

fn mock_usage_response_exhausted() -> &'static str {
    r#"{
        "usageBreakdownList": [
            {
                "resourceType": "AGENTIC_REQUEST",
                "usageLimitWithPrecision": 100.0,
                "currentUsageWithPrecision": 100.0
            }
        ]
    }"#
}

fn mock_usage_response_with_free_trial() -> &'static str {
    r#"{
        "usageBreakdownList": [
            {
                "resourceType": "AGENTIC_REQUEST",
                "usageLimitWithPrecision": 100.0,
                "currentUsageWithPrecision": 100.0,
                "freeTrialInfo": {
                    "freeTrialStatus": "ACTIVE",
                    "usageLimitWithPrecision": 50.0,
                    "currentUsageWithPrecision": 10.0
                }
            }
        ],
        "nextDateReset": 1710777600000
    }"#
}

fn mock_usage_response_empty() -> &'static str {
    r#"{
        "usageBreakdownList": []
    }"#
}

#[tokio::test]
async fn test_usage_checker_available_quota() {
    use axum::{routing::get, Router};

    let app = Router::new().route("/getUsageLimits", get(|| async {
        mock_usage_response_available()
    }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let checker = KiroUsageChecker::new(&base_url);
    let req = UsageCheckRequest {
        access_token: "fake-token".to_string(),
        profile_arn: "arn:aws:iam::123456789012:role/test".to_string(),
        client_id: Some("test-client-id".to_string()),
        refresh_token: Some("test-refresh-token".to_string()),
    };
    let status = checker.check(&req).await;

    match status {
        QuotaStatus::Available { remaining, next_reset, .. } => {
            assert_eq!(remaining, Some(70));
            assert!(next_reset.is_some(), "next_reset should be present");
        }
        _ => panic!("Expected Available status, got {:?}", status),
    }
}

#[tokio::test]
async fn test_usage_checker_exhausted_quota() {
    use axum::{routing::get, Router};

    let app = Router::new().route("/getUsageLimits", get(|| async {
        mock_usage_response_exhausted()
    }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let checker = KiroUsageChecker::new(&base_url);
    let req = UsageCheckRequest {
        access_token: "fake-token".to_string(),
        profile_arn: "arn:aws:iam::123456789012:role/test".to_string(),
        client_id: None,
        refresh_token: None,
    };
    let status = checker.check(&req).await;

    assert!(status.is_exhausted(), "Expected exhausted status");
}

#[tokio::test]
async fn test_usage_checker_with_free_trial() {
    use axum::{routing::get, Router};

    let app = Router::new().route("/getUsageLimits", get(|| async {
        mock_usage_response_with_free_trial()
    }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let checker = KiroUsageChecker::new(&base_url);
    let req = UsageCheckRequest {
        access_token: "fake-token".to_string(),
        profile_arn: "arn:aws:iam::123456789012:role/test".to_string(),
        client_id: Some("client-123".to_string()),
        refresh_token: None,
    };
    let status = checker.check(&req).await;

    match status {
        QuotaStatus::Available { remaining, next_reset, .. } => {
            // Main: 100 - 100 = 0, Free trial: 50 - 10 = 40, Total = 40
            assert_eq!(remaining, Some(40));
            assert!(next_reset.is_some(), "next_reset should be present for free trial");
        }
        _ => panic!("Expected Available status with free trial, got {:?}", status),
    }
}

#[tokio::test]
async fn test_usage_checker_empty_breakdown() {
    use axum::{routing::get, Router};

    let app = Router::new().route("/getUsageLimits", get(|| async {
        mock_usage_response_empty()
    }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let checker = KiroUsageChecker::new(&base_url);
    let req = UsageCheckRequest {
        access_token: "fake-token".to_string(),
        profile_arn: "arn:aws:iam::123456789012:role/test".to_string(),
        client_id: None,
        refresh_token: None,
    };
    let status = checker.check(&req).await;

    // Empty breakdown should be treated as exhausted
    assert!(status.is_exhausted(), "Expected exhausted status for empty breakdown");
}

#[tokio::test]
async fn test_usage_checker_http_error_returns_unknown() {
    use axum::{http::StatusCode, routing::get, Router};

    let app = Router::new().route("/getUsageLimits", get(|| async {
        (StatusCode::INTERNAL_SERVER_ERROR, "Server error")
    }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let checker = KiroUsageChecker::new(&base_url);
    let req = UsageCheckRequest {
        access_token: "fake-token".to_string(),
        profile_arn: "arn:aws:iam::123456789012:role/test".to_string(),
        client_id: None,
        refresh_token: None,
    };
    let status = checker.check(&req).await;

    // HTTP errors should return Unknown, not panic
    assert_eq!(status, QuotaStatus::Unknown);
}

#[tokio::test]
async fn test_usage_checker_network_error_returns_unknown() {
    // Use an invalid URL that will fail to connect
    let checker = KiroUsageChecker::new("http://127.0.0.1:1");
    let req = UsageCheckRequest {
        access_token: "fake-token".to_string(),
        profile_arn: "arn:aws:iam::123456789012:role/test".to_string(),
        client_id: None,
        refresh_token: None,
    };
    let status = checker.check(&req).await;

    // Network errors should return Unknown, not panic
    assert_eq!(status, QuotaStatus::Unknown);
}

#[tokio::test]
async fn test_usage_checker_invalid_json_returns_unknown() {
    use axum::{routing::get, Router};

    let app = Router::new().route("/getUsageLimits", get(|| async {
        "not valid json"
    }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let checker = KiroUsageChecker::new(&base_url);
    let req = UsageCheckRequest {
        access_token: "fake-token".to_string(),
        profile_arn: "arn:aws:iam::123456789012:role/test".to_string(),
        client_id: None,
        refresh_token: None,
    };
    let status = checker.check(&req).await;

    // Parse errors should return Unknown, not panic
    assert_eq!(status, QuotaStatus::Unknown);
}

#[tokio::test]
async fn test_trait_implementation_with_metadata() {
    use axum::{routing::get, Router};
    use rusuh::auth::kiro_runtime::QuotaChecker;

    let app = Router::new().route("/getUsageLimits", get(|| async {
        mock_usage_response_available()
    }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let checker = KiroUsageChecker::new(&base_url);
    let req = UsageCheckRequest {
        access_token: "fake-token".to_string(),
        profile_arn: "arn:aws:iam::123456789012:role/test".to_string(),
        client_id: Some("test-client".to_string()),
        refresh_token: Some("test-refresh".to_string()),
    };

    // Test via trait
    let status = checker.check_quota(&req).await;

    match status {
        QuotaStatus::Available { remaining, .. } => {
            assert_eq!(remaining, Some(70));
        }
        _ => panic!("Expected Available status via trait, got {:?}", status),
    }
}
