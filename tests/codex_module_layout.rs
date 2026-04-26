use rusuh::auth::{codex, codex_device, codex_login, codex_runtime};

#[test]
fn codex_auth_surface_is_exposed_via_four_modules() {
    let _ = codex::CLIENT_ID;
    let _ = codex::REDIRECT_URI;
    let _ = codex_login::generate_auth_url;
    let _ = codex_login::parse_manual_callback_url;
    let _ = codex_login::generate_pkce_codes;
    let _ = codex_device::request_codex_device_user_code;
    let _ = codex_runtime::parse_codex_retry_after_seconds;
}
