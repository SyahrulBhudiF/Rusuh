//! Zed native-app login crypto helpers.

use anyhow::{Context, Result};
use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use rsa::pkcs1::EncodeRsaPublicKey;
use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey, LineEnding};
use rsa::rand_core::OsRng;
use rsa::{Oaep, RsaPrivateKey, RsaPublicKey};
use sha2::Sha256;

const ZED_NATIVE_APP_SIGNIN_URL: &str = "https://zed.dev/native_app_signin";

/// Generate a fresh RSA keypair for native-app login.
///
/// Returns `(public_key_base64url, private_key_pem)`.
pub fn generate_keypair() -> Result<(String, String)> {
    let mut rng = OsRng;
    let private_key =
        RsaPrivateKey::new(&mut rng, 2048).context("failed to generate RSA keypair")?;
    let public_key = RsaPublicKey::from(&private_key);

    let public_key_der = public_key
        .to_pkcs1_der()
        .context("failed to encode public key as PKCS#1 DER")?;
    let public_key_b64 = URL_SAFE_NO_PAD.encode(public_key_der.as_bytes());
    let private_key_pem = private_key
        .to_pkcs8_pem(LineEnding::LF)
        .context("failed to encode private key as PKCS#8 PEM")?
        .to_string();

    Ok((public_key_b64, private_key_pem))
}

/// Build the Zed browser login URL for a callback port and public key.
pub fn build_login_url(public_key: &str, port: u16) -> String {
    format!("{ZED_NATIVE_APP_SIGNIN_URL}?native_app_port={port}&native_app_public_key={public_key}")
}

/// Decrypt a callback credential using the session private key.
///
/// Accepts both padded and unpadded base64url ciphertext.
pub fn decrypt_credential(private_key_pem: &str, encrypted_b64: &str) -> Result<String> {
    let private_key = RsaPrivateKey::from_pkcs8_pem(private_key_pem)
        .context("failed to parse private key PEM")?;
    let ciphertext =
        decode_base64url(encrypted_b64).context("failed to decode encrypted credential")?;
    let plaintext = private_key
        .decrypt(Oaep::new::<Sha256>(), &ciphertext)
        .context("failed to decrypt credential")?;

    String::from_utf8(plaintext).context("decrypted credential is not valid UTF-8")
}

fn decode_base64url(input: &str) -> Result<Vec<u8>> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        anyhow::bail!("encrypted credential is empty");
    }

    URL_SAFE_NO_PAD
        .decode(trimmed)
        .or_else(|_| URL_SAFE.decode(trimmed))
        .context("invalid base64url input")
}
