use rusuh::auth::codex::{
    build_codex_auth_record, create_token_storage, credential_file_name, hash_account_id_short,
    normalize_plan_type_for_filename, save_auth_bundle, update_token_storage, CodexAuthBundle,
    CodexTokenData,
};
use rusuh::auth::codex_device::{device_login_with_endpoints, CodexDeviceEndpoints};
use rusuh::auth::codex_login::{
    derive_code_challenge, exchange_code_for_tokens_with_redirect, generate_auth_url,
    generate_pkce_codes, is_safe_platform_url, parse_manual_callback_url,
    resolve_callback_code_from_url, validate_callback_state, OAuthServer, PKCECodes,
};
use rusuh::auth::codex_runtime::is_non_retryable_refresh_error;
use rusuh::auth::store::FileTokenStore;

#[test]
fn pkce_verifier_and_challenge_are_valid() {
    let pkce = generate_pkce_codes();

    assert!((43..=128).contains(&pkce.code_verifier.len()));
    assert!(pkce
        .code_verifier
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~')));
    assert_eq!(
        pkce.code_challenge,
        derive_code_challenge(&pkce.code_verifier)
    );
}

#[test]
fn auth_url_contains_pkce_state_and_redirect() {
    let pkce = generate_pkce_codes();
    let url = generate_auth_url("state123", &pkce, "http://localhost:1455/auth/callback")
        .expect("auth url should be generated");

    assert!(url.contains("code_challenge="));
    assert!(url.contains("code_challenge_method=S256"));
    assert!(url.contains("state=state123"));
    assert!(url.contains("redirect_uri="));
}

#[test]
fn callback_state_validation_rejects_mismatch() {
    let err = validate_callback_state("expected", "actual").expect_err("must reject mismatch");
    assert!(err.to_string().contains("state mismatch"));
}

#[test]
fn manual_callback_url_parses_code_state_and_error() {
    let parsed = parse_manual_callback_url(
        "http://localhost:1455/auth/callback?code=abc&state=xyz&error_description=nope",
    )
    .expect("manual callback should parse");

    assert_eq!(parsed.code.as_deref(), Some("abc"));
    assert_eq!(parsed.state.as_deref(), Some("xyz"));
    assert_eq!(parsed.error_description.as_deref(), Some("nope"));
}

#[test]
fn setup_notice_url_only_accepts_http_or_https() {
    assert!(is_safe_platform_url("https://platform.example.com/setup"));
    assert!(is_safe_platform_url("http://localhost:3000/setup"));
    assert!(!is_safe_platform_url("javascript:alert(1)"));
    assert!(!is_safe_platform_url("file:///tmp/setup"));
}

#[tokio::test]
async fn oauth_server_running_state_and_timeout_behavior() {
    let server = OAuthServer::new(0);
    assert!(!server.is_running());

    server.start().expect("server start should succeed");
    assert!(server.is_running());

    let addr = server
        .address()
        .expect("server should expose bound address");
    assert_ne!(addr.port(), 0);

    tokio::net::TcpStream::connect(addr)
        .await
        .expect("client should connect to bound oauth listener");

    let timeout_result = server
        .wait_for_callback(std::time::Duration::from_millis(10))
        .await;
    assert!(timeout_result.is_err());

    server.stop().expect("server stop should succeed");
    assert!(!server.is_running());
}

#[test]
fn codex_token_storage_and_update_preserve_field_names() {
    let bundle = CodexAuthBundle {
        token_data: CodexTokenData {
            id_token: "id1".into(),
            access_token: "acc1".into(),
            refresh_token: "ref1".into(),
            account_id: "acct1".into(),
            email: "user@example.com".into(),
            expired: "2030-01-01T00:00:00Z".into(),
        },
        last_refresh: "2026-03-18T00:00:00Z".into(),
    };

    let mut storage = create_token_storage(&bundle);
    assert_eq!(storage.expired, "2030-01-01T00:00:00Z");
    assert_eq!(storage.last_refresh, "2026-03-18T00:00:00Z");

    let next = CodexTokenData {
        id_token: "id2".into(),
        access_token: "acc2".into(),
        refresh_token: "ref2".into(),
        account_id: "acct2".into(),
        email: "next@example.com".into(),
        expired: "2031-01-01T00:00:00Z".into(),
    };

    update_token_storage(&mut storage, &next, "2026-03-19T00:00:00Z");

    assert_eq!(storage.id_token, "id2");
    assert_eq!(storage.access_token, "acc2");
    assert_eq!(storage.refresh_token, "ref2");
    assert_eq!(storage.account_id, "acct2");
    assert_eq!(storage.email, "next@example.com");
    assert_eq!(storage.expired, "2031-01-01T00:00:00Z");
    assert_eq!(storage.last_refresh, "2026-03-19T00:00:00Z");
}

#[test]
fn codex_filename_uses_plan_and_team_hash_rules() {
    assert_eq!(
        normalize_plan_type_for_filename(" Team Plan "),
        "team-plan".to_string()
    );

    let short_hash = hash_account_id_short("account-123");
    assert_eq!(short_hash.len(), 8);

    let plus = credential_file_name("user@example.com", "plus", "ignored", true);
    assert_eq!(plus, "codex-user@example.com-plus.json");

    let team = credential_file_name("user@example.com", "team", &short_hash, true);
    assert_eq!(
        team,
        format!("codex-{short_hash}-user@example.com-team.json")
    );
}

#[test]
fn refresh_token_reused_is_non_retryable() {
    assert!(is_non_retryable_refresh_error("refresh_token_reused"));
    assert!(is_non_retryable_refresh_error(
        "oauth error: refresh_token_reused detected"
    ));
    assert!(!is_non_retryable_refresh_error("timeout"));
}

#[test]
fn build_codex_auth_record_contains_required_metadata_fields() {
    let bundle = CodexAuthBundle {
        token_data: CodexTokenData {
            id_token: "idtoken".into(),
            access_token: "access".into(),
            refresh_token: "refresh".into(),
            account_id: "acct_123".into(),
            email: "user@example.com".into(),
            expired: "2030-01-01T00:00:00Z".into(),
        },
        last_refresh: "2026-03-18T00:00:00Z".into(),
    };

    let record = build_codex_auth_record(&bundle, Some("team"), Some("abcd1234"), true)
        .expect("record should build");

    assert_eq!(record.provider, "codex");
    assert_eq!(record.provider_key, "codex");
    assert_eq!(
        record.metadata.get("type").and_then(|v| v.as_str()),
        Some("codex")
    );
    assert_eq!(
        record.metadata.get("provider_key").and_then(|v| v.as_str()),
        Some("codex")
    );
    assert_eq!(
        record.metadata.get("access_token").and_then(|v| v.as_str()),
        Some("access")
    );
    assert_eq!(
        record.metadata.get("id_token").and_then(|v| v.as_str()),
        Some("idtoken")
    );
    assert_eq!(
        record.metadata.get("expired").and_then(|v| v.as_str()),
        Some("2030-01-01T00:00:00Z")
    );
    assert_eq!(
        record.metadata.get("last_refresh").and_then(|v| v.as_str()),
        Some("2026-03-18T00:00:00Z")
    );
    assert!(record.id.ends_with(".json"));
}

#[tokio::test]
async fn exchange_code_for_tokens_with_redirect_rejects_empty_code() {
    let client = reqwest::Client::new();
    let pkce = PKCECodes {
        code_verifier: "verifier".into(),
        code_challenge: derive_code_challenge("verifier"),
    };

    let err = exchange_code_for_tokens_with_redirect(
        &client,
        "   ",
        "http://localhost:1455/auth/callback",
        &pkce,
    )
    .await
    .expect_err("empty code should fail");
    assert!(err.to_string().contains("code must not be empty"));
}

#[test]
fn resolve_callback_code_from_url_extracts_code_and_validates_state() {
    let code = resolve_callback_code_from_url(
        "http://localhost:1455/auth/callback?code=abc123&state=expected",
        "expected",
    )
    .expect("callback url should be accepted");

    assert_eq!(code, "abc123");
}

#[test]
fn resolve_callback_code_from_url_rejects_state_mismatch() {
    let err = resolve_callback_code_from_url(
        "http://localhost:1455/auth/callback?code=abc123&state=actual",
        "expected",
    )
    .expect_err("state mismatch should be rejected");

    assert!(err.to_string().contains("state mismatch"));
}

#[tokio::test]
async fn device_login_persists_canonical_codex_record_from_explicit_endpoints() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let store = FileTokenStore::new(tmp.path());

    let app = axum::Router::new()
        .route(
            "/api/accounts/deviceauth/usercode",
            axum::routing::post(|| async {
                (
                    axum::http::StatusCode::OK,
                    axum::Json(serde_json::json!({
                        "device_auth_id": "dev_123",
                        "user_code": "ABC-123",
                        "interval": 1
                    })),
                )
            }),
        )
        .route(
            "/api/accounts/deviceauth/token",
            axum::routing::post(|| async {
                (
                    axum::http::StatusCode::OK,
                    axum::Json(serde_json::json!({
                        "authorization_code": "auth_code",
                        "code_verifier": "verifier",
                        "code_challenge": "challenge"
                    })),
                )
            }),
        )
        .route(
            "/oauth/token",
            axum::routing::post(|| async {
                (
                    axum::http::StatusCode::OK,
                    axum::Json(serde_json::json!({
                        "access_token": "device_access",
                        "refresh_token": "device_refresh",
                        "id_token": "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.eyJlbWFpbCI6InVzZXJAZXhhbXBsZS5jb20iLCJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjdF8xMjMiLCJjaGF0Z3B0X3BsYW5fdHlwZSI6IlRlYW0ifX0.sig",
                        "expires_in": 3600
                    })),
                )
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock server");
    let addr = listener.local_addr().expect("read mock server addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let endpoints = CodexDeviceEndpoints::from_auth_base_url(&format!("http://{addr}"))
        .expect("mock endpoint base url should be accepted");
    let client = reqwest::Client::new();

    let login = device_login_with_endpoints(&store, &client, &endpoints)
        .await
        .expect("device login should succeed");
    let saved = login.saved_path;

    assert_eq!(login.user_code.user_code, "ABC-123");
    assert_eq!(login.user_code.countdown_start_secs, 600);
    assert!(saved
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .contains("codex-"));

    let listed = store.list().await.expect("list auth files");
    assert_eq!(listed.len(), 1);

    let record = &listed[0];
    assert_eq!(record.provider, "codex");
    assert_eq!(
        record.metadata.get("type").and_then(|v| v.as_str()),
        Some("codex")
    );
    assert_eq!(
        record.metadata.get("provider_key").and_then(|v| v.as_str()),
        Some("codex")
    );
    assert_eq!(
        record.metadata.get("access_token").and_then(|v| v.as_str()),
        Some("device_access")
    );
    assert_eq!(
        record
            .metadata
            .get("refresh_token")
            .and_then(|v| v.as_str()),
        Some("device_refresh")
    );
}

#[tokio::test]
async fn save_auth_bundle_writes_canonical_codex_auth_file() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let store = FileTokenStore::new(tmp.path());

    let bundle = CodexAuthBundle {
        token_data: CodexTokenData {
            id_token: "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.eyJlbWFpbCI6InVzZXJAZXhhbXBsZS5jb20iLCJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjdF8xMjMiLCJjaGF0Z3B0X3BsYW5fdHlwZSI6IlRlYW0ifX0.sig".into(),
            access_token: "access".into(),
            refresh_token: "refresh".into(),
            account_id: "acct_123".into(),
            email: "fallback@example.com".into(),
            expired: "2030-01-01T00:00:00Z".into(),
        },
        last_refresh: "2026-03-18T00:00:00Z".into(),
    };

    let saved = save_auth_bundle(&store, &bundle, true)
        .await
        .expect("save should succeed");

    assert!(saved
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .contains("codex-"));
}
