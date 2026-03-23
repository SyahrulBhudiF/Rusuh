use std::collections::HashMap;
use std::sync::Arc;

use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use chrono::{Duration, Utc};
use rsa::pkcs1::DecodeRsaPublicKey;
use rsa::rand_core::OsRng;
use rsa::traits::PublicKeyParts;
use rsa::{Oaep, RsaPublicKey};
use sha2::Sha256;

use rusuh::auth::zed_callback::start_callback_server;
use rusuh::auth::zed_login::{build_login_url, decrypt_credential, generate_keypair};
use rusuh::auth::zed_session::{cleanup_expired_sessions, ZedLoginSession, ZedLoginSessionStatus};

#[test]
fn login_url_contains_port_and_public_key() {
    let (public_key, _private_key) = generate_keypair().unwrap();
    let url = build_login_url(&public_key, 43123);
    assert!(url.contains("native_app_port=43123"));
    assert!(url.contains("native_app_public_key="));
}

#[test]
fn public_key_is_base64url_without_padding() {
    let (public_key, _private_key) = generate_keypair().unwrap();
    assert!(!public_key.contains('='));
    assert!(URL_SAFE_NO_PAD.decode(public_key.as_bytes()).is_ok());
}

#[test]
fn public_key_decodes_to_pkcs1_der_bytes_not_pem_text() {
    let (public_key, _private_key) = generate_keypair().unwrap();
    let der = URL_SAFE_NO_PAD.decode(public_key.as_bytes()).unwrap();

    assert!(!String::from_utf8_lossy(&der).contains("BEGIN PUBLIC KEY"));
    let parsed = RsaPublicKey::from_pkcs1_der(&der).unwrap();
    assert!(parsed.size() > 0);
}

#[test]
fn decrypt_accepts_unpadded_base64url() {
    let (public_key_b64, private_key_pem) = generate_keypair().unwrap();
    let public_key_der = URL_SAFE_NO_PAD.decode(public_key_b64.as_bytes()).unwrap();
    let public_key = RsaPublicKey::from_pkcs1_der(&public_key_der).unwrap();

    let plaintext = r#"{"token":"abc"}"#;
    let ciphertext = public_key
        .encrypt(&mut OsRng, Oaep::new::<Sha256>(), plaintext.as_bytes())
        .unwrap();
    let encrypted = URL_SAFE_NO_PAD.encode(ciphertext);

    let decrypted = decrypt_credential(&private_key_pem, &encrypted).unwrap();
    assert_eq!(decrypted, plaintext);
}

#[test]
fn decrypt_accepts_padded_base64url() {
    let (public_key_b64, private_key_pem) = generate_keypair().unwrap();
    let public_key_der = URL_SAFE_NO_PAD.decode(public_key_b64.as_bytes()).unwrap();
    let public_key = RsaPublicKey::from_pkcs1_der(&public_key_der).unwrap();

    let plaintext = r#"{"token":"abc"}"#;
    let ciphertext = public_key
        .encrypt(&mut OsRng, Oaep::new::<Sha256>(), plaintext.as_bytes())
        .unwrap();
    let encrypted = URL_SAFE.encode(ciphertext);

    let decrypted = decrypt_credential(&private_key_pem, &encrypted).unwrap();
    assert_eq!(decrypted, plaintext);
}

#[tokio::test]
async fn callback_server_binds_localhost_and_stores_user_id_and_access_token() {
    let (state, port, handle) = start_callback_server(0).await.unwrap();

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = client
        .get(format!(
            "http://127.0.0.1:{port}/?user_id=test-user&access_token=test-token"
        ))
        .send()
        .await
        .unwrap();

    assert!(response.status().is_redirection());
    assert_eq!(response.headers()[reqwest::header::LOCATION], "https://zed.dev/native_app_signin_succeeded");
    assert_eq!(state.user_id.lock().await.as_deref(), Some("test-user"));
    assert_eq!(state.access_token.lock().await.as_deref(), Some("test-token"));
    assert!(state.is_completed());

    handle.abort();
    let _ = handle.await;
}

#[tokio::test]
async fn callback_handler_redirects_to_zed_success_page() {
    let (_state, port, handle) = start_callback_server(0).await.unwrap();

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = client
        .get(format!(
            "http://127.0.0.1:{port}/?user_id=test-user&access_token=test-token"
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), reqwest::StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(response.headers()[reqwest::header::LOCATION], "https://zed.dev/native_app_signin_succeeded");

    handle.abort();
    let _ = handle.await;
}

#[tokio::test]
async fn callback_rejects_missing_fields() {
    let (_state, port, handle) = start_callback_server(0).await.unwrap();

    let response = reqwest::get(format!("http://127.0.0.1:{port}/?user_id=test-user"))
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    let body = response.text().await.unwrap();
    assert!(body.contains("access_token"));

    handle.abort();
    let _ = handle.await;
}

#[tokio::test]
async fn session_boundary_at_exactly_ten_minutes_is_not_expired() {
    let (callback_state, _port, handle) = start_callback_server(0).await.unwrap();
    let callback_state = Arc::new(callback_state);
    let now = Utc::now();

    let session = ZedLoginSession {
        name: "boundary".to_string(),
        private_key: "priv".to_string(),
        port: 1234,
        status: ZedLoginSessionStatus::Waiting,
        created_at: now - Duration::minutes(10),
        callback_state,
        server_handle: handle,
    };

    assert!(!session.is_expired(now));

    session.server_handle.abort();
    let _ = session.server_handle.await;
}

#[tokio::test]
async fn session_store_cleanup_evicts_sessions_older_than_ten_minutes() {
    let (callback_state_old, _port_old, old_handle) = start_callback_server(0).await.unwrap();
    let (callback_state_new, _port_new, new_handle) = start_callback_server(0).await.unwrap();

    let old_arc = Arc::new(callback_state_old);
    let new_arc = Arc::new(callback_state_new);

    let mut sessions = HashMap::new();
    sessions.insert(
        "old".to_string(),
        ZedLoginSession {
            name: "old".to_string(),
            private_key: "priv".to_string(),
            port: 1234,
            status: ZedLoginSessionStatus::Waiting,
            created_at: Utc::now() - Duration::minutes(11),
            callback_state: old_arc,
            server_handle: old_handle,
        },
    );
    sessions.insert(
        "new".to_string(),
        ZedLoginSession {
            name: "new".to_string(),
            private_key: "priv".to_string(),
            port: 1235,
            status: ZedLoginSessionStatus::Waiting,
            created_at: Utc::now() - Duration::minutes(5),
            callback_state: new_arc,
            server_handle: new_handle,
        },
    );

    cleanup_expired_sessions(&mut sessions);

    assert!(!sessions.contains_key("old"));
    assert!(sessions.contains_key("new"));

    if let Some(session) = sessions.remove("new") {
        session.server_handle.abort();
        let _ = session.server_handle.await;
    }
}
