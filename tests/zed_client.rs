use rusuh::providers::zed::ZedClient;

#[test]
fn test_token_endpoint() {
    let client = ZedClient;
    assert_eq!(
        client.token_endpoint(),
        "https://cloud.zed.dev/client/llm_tokens"
    );
}

#[test]
fn test_completions_endpoint() {
    let client = ZedClient;
    assert_eq!(
        client.completions_endpoint(),
        "https://cloud.zed.dev/completions"
    );
}

#[test]
fn test_models_endpoint() {
    let client = ZedClient;
    assert_eq!(client.models_endpoint(), "https://cloud.zed.dev/models");
}

#[test]
fn test_users_me_endpoint() {
    let client = ZedClient;
    assert_eq!(
        client.users_me_endpoint(),
        "https://cloud.zed.dev/client/users/me"
    );
}

#[test]
fn test_is_stale_token_response_with_expired_header() {
    let client = ZedClient;
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("x-zed-expired-token", "true".parse().unwrap());

    assert!(client.is_stale_token_response(401, &headers));
}

#[test]
fn test_is_stale_token_response_with_outdated_header() {
    let client = ZedClient;
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("x-zed-outdated-token", "true".parse().unwrap());

    assert!(client.is_stale_token_response(401, &headers));
}

#[test]
fn test_is_stale_token_response_401_without_headers() {
    let client = ZedClient;
    let headers = reqwest::header::HeaderMap::new();

    assert!(!client.is_stale_token_response(401, &headers));
}

#[test]
fn test_is_stale_token_response_non_401() {
    let client = ZedClient;
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("x-zed-expired-token", "true".parse().unwrap());

    assert!(!client.is_stale_token_response(403, &headers));
}
