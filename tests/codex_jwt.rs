use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rusuh::auth::codex_login::{get_account_id, get_plan_type, get_user_email, parse_jwt_token};
use serde_json::json;

fn jwt_with_payload(payload: serde_json::Value) -> String {
    let header = json!({"alg": "none", "typ": "JWT"});
    let header = URL_SAFE_NO_PAD.encode(header.to_string());
    let payload = URL_SAFE_NO_PAD.encode(payload.to_string());
    format!("{header}.{payload}.sig")
}

#[test]
fn parse_jwt_extracts_email_and_account_id() {
    let token = jwt_with_payload(json!({
        "email": "user@example.com",
        "https://api.openai.com/auth": {
            "chatgpt_account_id": "acct_123",
            "chatgpt_plan_type": "Team Plan"
        }
    }));

    let claims = parse_jwt_token(&token).expect("jwt should parse");

    assert_eq!(get_user_email(&claims), Some("user@example.com"));
    assert_eq!(get_account_id(&claims), Some("acct_123"));
    assert_eq!(get_plan_type(&claims), Some("Team Plan"));
}

#[test]
fn parse_jwt_handles_missing_optional_fields() {
    let token = jwt_with_payload(json!({"sub": "user"}));

    let claims = parse_jwt_token(&token).expect("jwt should parse");

    assert_eq!(get_user_email(&claims), None);
    assert_eq!(get_account_id(&claims), None);
    assert_eq!(get_plan_type(&claims), None);
}

#[test]
fn parse_jwt_rejects_malformed_token_parts() {
    let err = parse_jwt_token("not-a-jwt").expect_err("token should be rejected");
    assert!(err.to_string().contains("expected 3 parts"));
}

#[test]
fn parse_jwt_rejects_invalid_payload_base64() {
    let token = "a.!invalid!.c";
    let err = parse_jwt_token(token).expect_err("token should be rejected");
    assert!(err.to_string().contains("decode"));
}
