//! Antigravity OAuth login flow — local callback server + token exchange + project_id fetch.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::oneshot;
use tracing::{debug, info, warn};

use crate::auth::antigravity::*;
use crate::auth::store::{AuthRecord, FileTokenStore};

// ── Token types ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub(crate) access_token: String,
    pub(crate) refresh_token: Option<String>,
    pub(crate) expires_in: Option<i64>,
    #[allow(dead_code)]
    token_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UserInfo {
    email: Option<String>,
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Run the full Antigravity login flow:
/// 1. Start callback server on CALLBACK_PORT
/// 2. Open browser to Google OAuth consent
/// 3. Exchange code for tokens
/// 4. Fetch email + project_id
/// 5. Save to auth-dir
pub async fn login(store: &FileTokenStore) -> anyhow::Result<()> {
    let redirect_uri = format!("http://localhost:{}/oauth-callback", CALLBACK_PORT);
    let state = uuid::Uuid::new_v4().to_string();

    // Build auth URL
    let auth_url = build_auth_url(&state, &redirect_uri);

    println!("\nOpen this URL in your browser to authenticate:\n");
    println!("  {}\n", auth_url);

    // Try to open browser automatically
    if open::that(&auth_url).is_err() {
        println!("(Could not open browser automatically — please open the URL manually)");
    }

    // Wait for callback
    let code = wait_for_callback(&state).await?;
    info!("received auth code, exchanging for tokens...");

    let client = reqwest::Client::new();

    // Exchange code for tokens
    let tokens = exchange_code(&client, &code, &redirect_uri).await?;
    info!("token exchange successful");

    // Fetch user email
    let email = fetch_user_info(&client, &tokens.access_token).await?;
    info!("authenticated as: {}", email);

    // Fetch project_id
    let project_id = match fetch_project_id(&client, &tokens.access_token).await {
        Ok(pid) => {
            info!("project_id: {}", pid);
            Some(pid)
        }
        Err(e) => {
            warn!("could not fetch project_id (will retry at load time): {e}");
            None
        }
    };

    // Compute expiry timestamp
    let expires_at = tokens
        .expires_in
        .map(|secs| chrono::Utc::now().timestamp() + secs);

    // Build metadata
    let mut metadata: HashMap<String, Value> = HashMap::new();
    metadata.insert("type".into(), json!("antigravity"));
    metadata.insert("email".into(), json!(email));
    metadata.insert("access_token".into(), json!(tokens.access_token));
    if let Some(ref rt) = tokens.refresh_token {
        metadata.insert("refresh_token".into(), json!(rt));
    }
    if let Some(exp) = expires_at {
        metadata.insert("expires_at".into(), json!(exp));
    }
    if let Some(ref pid) = project_id {
        metadata.insert("project_id".into(), json!(pid));
    }
    metadata.insert("disabled".into(), json!(false));

    // Save
    let filename = credential_filename(&email);
    let record = AuthRecord {
        id: filename.clone(),
        provider: "antigravity".into(),
        provider_key: "antigravity".into(),
        label: email.clone(),
        disabled: false,
        status: crate::auth::store::AuthStatus::Active,
        status_message: None,
        last_refreshed_at: Some(chrono::Utc::now()),
        path: PathBuf::from(&filename),
        metadata,
        updated_at: chrono::Utc::now(),
    };

    let saved_path = store.save(&record).await?;
    println!(
        "\n✓ Antigravity credentials saved to: {}",
        saved_path.display()
    );
    println!("  Account: {}", email);
    if let Some(pid) = project_id {
        println!("  Project: {}", pid);
    }

    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn credential_filename(email: &str) -> String {
    let email = email.trim();
    if email.is_empty() {
        "antigravity.json".to_string()
    } else {
        format!("antigravity-{}.json", email)
    }
}

fn build_auth_url(state: &str, redirect_uri: &str) -> String {
    let scopes = SCOPES.join(" ");
    let params = [
        ("access_type", "offline"),
        ("client_id", CLIENT_ID),
        ("prompt", "consent"),
        ("redirect_uri", redirect_uri),
        ("response_type", "code"),
        ("scope", &scopes),
        ("state", state),
    ];
    let query = serde_urlencoded::to_string(params).expect("encode params");
    format!("{}?{}", AUTH_ENDPOINT, query)
}

/// Start a tiny axum server on CALLBACK_PORT, wait for the OAuth callback.
async fn wait_for_callback(expected_state: &str) -> anyhow::Result<String> {
    let (tx, rx) = oneshot::channel::<String>();
    let tx = Arc::new(tokio::sync::Mutex::new(Some(tx)));
    let expected = expected_state.to_string();

    let app = axum::Router::new().route(
        "/oauth-callback",
        axum::routing::get(move |query: axum::extract::Query<CallbackQuery>| {
            let tx = tx.clone();
            let expected = expected.clone();
            async move {
                if query.state != expected {
                    return axum::response::Html(
                        "<h1>Error</h1><p>State mismatch — possible CSRF attack.</p>"
                            .to_string(),
                    );
                }
                if let Some(sender) = tx.lock().await.take() {
                    let _ = sender.send(query.code.clone());
                }
                axum::response::Html(
                    "<h1>✓ Authenticated!</h1><p>You can close this tab and return to the terminal.</p>"
                        .to_string(),
                )
            }
        }),
    );

    let addr = format!("127.0.0.1:{}", CALLBACK_PORT);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("OAuth callback server listening on {}", addr);

    // Serve until we get the code
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("callback server failed");
    });

    let code = rx
        .await
        .map_err(|_| anyhow::anyhow!("callback channel closed without receiving code"))?;

    // Shut down the server
    server.abort();
    let _ = server.await;

    Ok(code)
}

#[derive(Debug, Deserialize)]
struct CallbackQuery {
    code: String,
    state: String,
}

async fn exchange_code(
    client: &reqwest::Client,
    code: &str,
    redirect_uri: &str,
) -> anyhow::Result<TokenResponse> {
    let params = [
        ("code", code),
        ("client_id", CLIENT_ID),
        ("client_secret", CLIENT_SECRET),
        ("redirect_uri", redirect_uri),
        ("grant_type", "authorization_code"),
    ];

    let resp = client.post(TOKEN_ENDPOINT).form(&params).send().await?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("token exchange failed ({}): {}", status, body);
    }

    Ok(resp.json::<TokenResponse>().await?)
}

async fn fetch_user_info(client: &reqwest::Client, access_token: &str) -> anyhow::Result<String> {
    let resp = client
        .get(USERINFO_ENDPOINT)
        .bearer_auth(access_token)
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("userinfo failed ({}): {}", status, body);
    }

    let info: UserInfo = resp.json().await?;
    info.email
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty())
        .ok_or_else(|| anyhow::anyhow!("userinfo response missing email"))
}

pub async fn fetch_project_id(
    client: &reqwest::Client,
    access_token: &str,
) -> anyhow::Result<String> {
    let url = format!("{}/{}:loadCodeAssist", API_ENDPOINT, API_VERSION);
    let body = json!({
        "metadata": {
            "ideType": "ANTIGRAVITY",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI",
        }
    });

    let resp = client
        .post(&url)
        .bearer_auth(access_token)
        .header("Content-Type", "application/json")
        .header("User-Agent", API_USER_AGENT)
        .header("X-Goog-Api-Client", API_CLIENT)
        .header("Client-Metadata", CLIENT_METADATA)
        .json(&body)
        .send()
        .await?;

    let status = resp.status();
    let data: Value = resp.json().await?;

    if !status.is_success() {
        anyhow::bail!("loadCodeAssist failed ({}): {}", status, data);
    }

    // Try direct string
    if let Some(pid) = data["cloudaicompanionProject"].as_str() {
        let pid = pid.trim();
        if !pid.is_empty() {
            return Ok(pid.to_string());
        }
    }

    // Try nested object
    if let Some(pid) = data["cloudaicompanionProject"]["id"].as_str() {
        let pid = pid.trim();
        if !pid.is_empty() {
            return Ok(pid.to_string());
        }
    }

    // Try onboarding
    let tier_id = data["allowedTiers"]
        .as_array()
        .and_then(|tiers| {
            tiers.iter().find_map(|t| {
                if t["isDefault"].as_bool() == Some(true) {
                    t["id"].as_str().map(|s| s.trim().to_string())
                } else {
                    None
                }
            })
        })
        .unwrap_or_else(|| "legacy-tier".to_string());

    onboard_user(client, access_token, &tier_id).await
}

async fn onboard_user(
    client: &reqwest::Client,
    access_token: &str,
    tier_id: &str,
) -> anyhow::Result<String> {
    info!("onboarding user with tier: {}", tier_id);

    let url = format!("{}/{}:onboardUser", API_ENDPOINT, API_VERSION);
    let body = json!({
        "tierId": tier_id,
        "metadata": {
            "ideType": "ANTIGRAVITY",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI",
        }
    });

    for attempt in 1..=5 {
        debug!("onboard polling attempt {}/5", attempt);

        let resp = client
            .post(&url)
            .bearer_auth(access_token)
            .header("Content-Type", "application/json")
            .header("User-Agent", API_USER_AGENT)
            .header("X-Goog-Api-Client", API_CLIENT)
            .header("Client-Metadata", CLIENT_METADATA)
            .json(&body)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await?;

        let status = resp.status();
        let data: Value = resp.json().await?;

        if !status.is_success() {
            anyhow::bail!("onboardUser failed ({}): {}", status, data);
        }

        if data["done"].as_bool() == Some(true) {
            // Try response.cloudaicompanionProject
            if let Some(pid) = data["response"]["cloudaicompanionProject"].as_str() {
                let pid = pid.trim();
                if !pid.is_empty() {
                    info!("successfully fetched project_id: {}", pid);
                    return Ok(pid.to_string());
                }
            }
            if let Some(pid) = data["response"]["cloudaicompanionProject"]["id"].as_str() {
                let pid = pid.trim();
                if !pid.is_empty() {
                    info!("successfully fetched project_id: {}", pid);
                    return Ok(pid.to_string());
                }
            }
            anyhow::bail!("no project_id in onboard response");
        }

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    anyhow::bail!("onboard polling timed out after 5 attempts")
}

/// Refresh an Antigravity access token using the refresh token.
pub async fn refresh_access_token(
    client: &reqwest::Client,
    refresh_token: &str,
) -> anyhow::Result<TokenResponse> {
    let params = [
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", CLIENT_ID),
        ("client_secret", CLIENT_SECRET),
    ];

    let resp = client.post(TOKEN_ENDPOINT).form(&params).send().await?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("token refresh failed ({}): {}", status, body);
    }

    Ok(resp.json::<TokenResponse>().await?)
}
