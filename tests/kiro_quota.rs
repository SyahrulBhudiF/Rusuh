//! Tests for `kiro_quota` module.

use rusuh::auth::kiro_runtime::{NoOpQuotaChecker, QuotaChecker, QuotaStatus};

#[test]
fn unknown_is_not_exhausted() {
    let status = QuotaStatus::Unknown;
    assert!(!status.is_exhausted());
    assert_eq!(status.remaining(), None);
}

#[test]
fn available_with_remaining() {
    let status = QuotaStatus::Available {
        remaining: Some(42),
        next_reset: None,
        breakdown: None,
    };
    assert!(!status.is_exhausted());
    assert_eq!(status.remaining(), Some(42));
    assert_eq!(status.next_reset(), None);
}

#[test]
fn available_without_remaining() {
    let status = QuotaStatus::Available {
        remaining: None,
        next_reset: None,
        breakdown: None,
    };
    assert!(!status.is_exhausted());
    assert_eq!(status.remaining(), None);
}

#[test]
fn exhausted_status() {
    let status = QuotaStatus::Exhausted {
        detail: "monthly limit reached".into(),
    };
    assert!(status.is_exhausted());
    assert_eq!(status.remaining(), None);
}

#[test]
fn default_is_unknown() {
    let status = QuotaStatus::default();
    assert_eq!(status, QuotaStatus::Unknown);
}

#[tokio::test]
async fn noop_checker_returns_unknown() {
    use rusuh::auth::kiro_runtime::UsageCheckRequest;
    let checker = NoOpQuotaChecker;
    let req = UsageCheckRequest {
        access_token: "any-token".to_string(),
        profile_arn: "arn:aws:iam::123:role/test".to_string(),
        client_id: None,
        refresh_token: None,
    };
    let result = checker.check_quota(&req).await;
    assert_eq!(result, QuotaStatus::Unknown);
}

// Fake exhausted checker for integration testing
struct FakeExhaustedChecker;

#[async_trait::async_trait]
impl QuotaChecker for FakeExhaustedChecker {
    async fn check_quota(
        &self,
        _request: &rusuh::auth::kiro_runtime::UsageCheckRequest,
    ) -> QuotaStatus {
        QuotaStatus::Exhausted {
            detail: "test exhausted".into(),
        }
    }
}

#[tokio::test]
async fn fake_exhausted_checker_returns_exhausted() {
    use rusuh::auth::kiro_runtime::UsageCheckRequest;
    let checker = FakeExhaustedChecker;
    let req = UsageCheckRequest {
        access_token: "token".to_string(),
        profile_arn: "arn:aws:iam::123:role/test".to_string(),
        client_id: None,
        refresh_token: None,
    };
    let result = checker.check_quota(&req).await;
    assert!(result.is_exhausted());
}

// Fake available checker
struct FakeAvailableChecker {
    remaining: Option<u64>,
}

#[async_trait::async_trait]
impl QuotaChecker for FakeAvailableChecker {
    async fn check_quota(
        &self,
        _request: &rusuh::auth::kiro_runtime::UsageCheckRequest,
    ) -> QuotaStatus {
        QuotaStatus::Available {
            remaining: self.remaining,
            next_reset: None,
            breakdown: None,
        }
    }
}

#[tokio::test]
async fn fake_available_checker_returns_available() {
    use rusuh::auth::kiro_runtime::UsageCheckRequest;
    let checker = FakeAvailableChecker {
        remaining: Some(100),
    };
    let req = UsageCheckRequest {
        access_token: "token".to_string(),
        profile_arn: "arn:aws:iam::123:role/test".to_string(),
        client_id: None,
        refresh_token: None,
    };
    let result = checker.check_quota(&req).await;
    assert!(!result.is_exhausted());
    assert_eq!(result.remaining(), Some(100));
}

// ── Integration with ModelRegistry ───────────────────────────────────────────

use rusuh::providers::model_info::ExtModelInfo;
use rusuh::providers::model_registry::ModelRegistry;

#[tokio::test]
async fn exhausted_checker_marks_quota_exceeded_in_registry() {
    let registry = ModelRegistry::new();
    let client_id = "kiro_0";
    let model_id = "claude-sonnet-4";

    // Register client with model
    let models = vec![ExtModelInfo {
        id: model_id.to_string(),
        object: "model".to_string(),
        created: 0,
        owned_by: "kiro".to_string(),
        provider_type: "kiro".to_string(),
        display_name: None,
        name: Some(model_id.to_string()),
        version: None,
        description: None,
        input_token_limit: 0,
        output_token_limit: 0,
        supported_generation_methods: vec![],
        context_length: 0,
        max_completion_tokens: 0,
        supported_parameters: vec![],
        thinking: None,
        user_defined: false,
    }];
    registry.register_client(client_id, "kiro", models).await;

    // Verify client is initially available
    assert!(
        registry
            .client_is_effectively_available(client_id, model_id)
            .await
    );

    // Simulate quota check returning exhausted
    use rusuh::auth::kiro_runtime::UsageCheckRequest;
    let checker = FakeExhaustedChecker;
    let req = UsageCheckRequest {
        access_token: "token".to_string(),
        profile_arn: "arn:aws:iam::123:role/test".to_string(),
        client_id: None,
        refresh_token: None,
    };
    let status = checker.check_quota(&req).await;

    // If exhausted, mark in registry
    if status.is_exhausted() {
        registry.set_quota_exceeded(client_id, model_id).await;
    }

    // Verify client is now unavailable
    assert!(
        !registry
            .client_is_effectively_available(client_id, model_id)
            .await
    );
}

#[tokio::test]
async fn available_checker_clears_stale_quota_exceeded() {
    let registry = ModelRegistry::new();
    let client_id = "kiro_0";
    let model_id = "claude-sonnet-4";

    // Register client with model
    let models = vec![ExtModelInfo {
        id: model_id.to_string(),
        object: "model".to_string(),
        created: 0,
        owned_by: "kiro".to_string(),
        provider_type: "kiro".to_string(),
        display_name: None,
        name: Some(model_id.to_string()),
        version: None,
        description: None,
        input_token_limit: 0,
        output_token_limit: 0,
        supported_generation_methods: vec![],
        context_length: 0,
        max_completion_tokens: 0,
        supported_parameters: vec![],
        thinking: None,
        user_defined: false,
    }];
    registry.register_client(client_id, "kiro", models).await;

    // Mark as quota-exceeded
    registry.set_quota_exceeded(client_id, model_id).await;
    assert!(
        !registry
            .client_is_effectively_available(client_id, model_id)
            .await
    );

    // Simulate quota check returning available
    use rusuh::auth::kiro_runtime::UsageCheckRequest;
    let checker = FakeAvailableChecker {
        remaining: Some(50),
    };
    let req = UsageCheckRequest {
        access_token: "token".to_string(),
        profile_arn: "arn:aws:iam::123:role/test".to_string(),
        client_id: None,
        refresh_token: None,
    };
    let status = checker.check_quota(&req).await;

    // If available, clear quota-exceeded state
    if !status.is_exhausted() {
        registry.clear_quota_exceeded(client_id, model_id).await;
    }

    // Verify client is now available again
    assert!(
        registry
            .client_is_effectively_available(client_id, model_id)
            .await
    );
}

#[tokio::test]
async fn noop_checker_does_not_block_requests() {
    let registry = ModelRegistry::new();
    let client_id = "kiro_0";
    let model_id = "claude-sonnet-4";

    // Register client with model
    let models = vec![ExtModelInfo {
        id: model_id.to_string(),
        object: "model".to_string(),
        created: 0,
        owned_by: "kiro".to_string(),
        provider_type: "kiro".to_string(),
        display_name: None,
        name: Some(model_id.to_string()),
        version: None,
        description: None,
        input_token_limit: 0,
        output_token_limit: 0,
        supported_generation_methods: vec![],
        context_length: 0,
        max_completion_tokens: 0,
        supported_parameters: vec![],
        thinking: None,
        user_defined: false,
    }];
    registry.register_client(client_id, "kiro", models).await;

    // Use NoOp checker
    use rusuh::auth::kiro_runtime::UsageCheckRequest;
    let checker = NoOpQuotaChecker;
    let req = UsageCheckRequest {
        access_token: "token".to_string(),
        profile_arn: "arn:aws:iam::123:role/test".to_string(),
        client_id: None,
        refresh_token: None,
    };
    let status = checker.check_quota(&req).await;

    // NoOp checker returns Unknown
    assert_eq!(status, QuotaStatus::Unknown);

    // Unknown status should not affect availability
    // (we don't mark quota-exceeded for Unknown status)
    assert!(
        registry
            .client_is_effectively_available(client_id, model_id)
            .await
    );
}
