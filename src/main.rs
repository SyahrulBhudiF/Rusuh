use rusuh::{auth, config, middleware, providers, proxy, router};
use std::path::PathBuf;
use std::sync::Arc;

use auth::cli::{Cli, Commands};
use auth::manager::AccountManager;
use clap::Parser;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

fn load_config_or_default(path: &str) -> anyhow::Result<config::Config> {
    match config::Config::load_optional(path)? {
        Some(cfg) => Ok(cfg),
        None => Ok(config::Config::default()),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Init tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("rusuh=info,tower_http=debug")),
        )
        .init();

    // Load .env if present
    let _ = dotenvy::dotenv();

    let cli = Cli::parse();

    // Load config (optional — server works with defaults)
    let cfg = load_config_or_default(&cli.config)?;

    match cli.command.unwrap_or(Commands::Serve) {
        Commands::Serve => serve(cfg).await?,
        Commands::Login => {
            println!("Gemini/Google login not yet implemented (milestone 2)");
        }
        Commands::AntigravityLogin => {
            let auth_dir = resolve_auth_dir(&cfg);
            let store = auth::store::FileTokenStore::new(&auth_dir);
            auth::antigravity_login::login(&store).await?
        }
        Commands::KiroLogin {
            provider,
            start_url,
        } => {
            let auth_dir = resolve_auth_dir(&cfg);
            let store = auth::store::FileTokenStore::new(&auth_dir);

            match provider.as_str() {
                "google" | "github" => auth::kiro_login::login(&store, &provider).await?,
                "sso" => {
                    if let Some(url) = start_url {
                        auth::kiro_login::login_sso(&store, &url).await?
                    } else {
                        eprintln!("Error: --start-url is required for SSO login");
                        std::process::exit(1);
                    }
                }
                _ => {
                    eprintln!(
                        "Error: Invalid provider '{}'. Use: google, github, or sso",
                        provider
                    );
                    std::process::exit(1);
                }
            }
        }
        Commands::CodexLogin => {
            let auth_dir = resolve_auth_dir(&cfg);
            let store = auth::store::FileTokenStore::new(&auth_dir);
            let saved = auth::codex_login::login(&store).await?;
            println!("\n✓ Codex credentials saved to: {}", saved.display());
        }
        Commands::CodexDeviceLogin => {
            run_codex_device_login(&cfg).await?;
        }
        Commands::ClaudeLogin => {
            println!("Claude Code login not yet implemented (milestone 2)");
        }
        Commands::QwenLogin => {
            println!("Qwen Code login not yet implemented (milestone 2)");
        }
        Commands::IflowLogin => {
            println!("iFlow login not yet implemented (milestone 2)");
        }
    }

    Ok(())
}

/// Resolve auth directory: config value > `~/.rusuh`
fn resolve_auth_dir(cfg: &config::Config) -> PathBuf {
    if !cfg.auth_dir.is_empty() {
        return PathBuf::from(&cfg.auth_dir);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".rusuh")
}

async fn run_codex_device_login(cfg: &config::Config) -> anyhow::Result<()> {
    let auth_dir = resolve_auth_dir(cfg);
    let store = auth::store::FileTokenStore::new(&auth_dir);
    let login = auth::codex_device::device_login(&store).await?;
    println!("\n✓ Codex device credentials saved to: {}", login.saved_path.display());
    Ok(())
}

/// Generate a random API key in `rsk-<uuid>` format.
fn generate_api_key() -> String {
    format!("rsk-{}", uuid::Uuid::new_v4())
}

/// Ensure at least one API key exists. If `api-keys` is empty or contains only
/// placeholder values, generate a fresh key and inject it into the config.
/// The key is printed to stdout so the operator can grab it.
fn ensure_api_keys(cfg: &mut config::Config) {
    // Filter out placeholder/example keys
    let real_keys: Vec<_> = cfg
        .api_keys
        .iter()
        .filter(|k| {
            let k = k.trim();
            !k.is_empty() && !k.starts_with("your-api-key") && k != "changeme"
        })
        .cloned()
        .collect();

    if !real_keys.is_empty() {
        cfg.api_keys = real_keys;
        info!("loaded {} API key(s) from config", cfg.api_keys.len());
        return;
    }

    let key = generate_api_key();
    warn!("no API keys configured — auto-generated key for this session");
    println!();
    println!("  ╔══════════════════════════════════════════════════════════════╗");
    println!("  ║  Auto-generated API key (not persisted):                    ║");
    println!("  ║  {:<59}║", &key);
    println!("  ║                                                             ║");
    println!("  ║  Add to config.yaml under `api-keys:` to persist.           ║");
    println!("  ╚══════════════════════════════════════════════════════════════╝");
    println!();
    cfg.api_keys = vec![key];
}

async fn serve(mut cfg: config::Config) -> anyhow::Result<()> {
    let addr = cfg.listen_addr();
    let auth_dir = resolve_auth_dir(&cfg);
    // Auto-generate API key if none configured
    ensure_api_keys(&mut cfg);
    info!("auth directory: {}", auth_dir.display());

    // Load accounts from auth-dir
    let account_mgr = Arc::new(AccountManager::with_dir(&auth_dir));
    if let Err(e) = account_mgr.reload().await {
        tracing::warn!("failed to load accounts: {e}");
    }

    // Build model registry and Kiro runtime before providers so providers can share them.
    let model_registry = Arc::new(providers::model_registry::ModelRegistry::new());
    let mut kiro_runtime = proxy::KiroRuntimeState::default();
    match auth::kiro_runtime::KiroUsageChecker::new("https://codewhisperer.us-east-1.amazonaws.com")
    {
        Ok(checker) => {
            kiro_runtime.quota_checker = Arc::new(checker);
        }
        Err(error) => {
            tracing::warn!("failed to initialize Kiro usage checker: {error}");
        }
    }

    // Build providers from loaded accounts + config
    let providers = providers::registry::build_providers(
        &cfg,
        &account_mgr,
        model_registry.clone(),
        kiro_runtime.clone(),
    )
    .await;
    // Register all provider models
    for provider in &providers {
        let client_id = provider.client_id().to_string();
        match provider.list_models().await {
            Ok(models) => {
                let ext_models: Vec<_> = models
                    .iter()
                    .map(|m| providers::model_info::ExtModelInfo {
                        id: m.id.clone(),
                        object: m.object.clone(),
                        created: m.created,
                        owned_by: m.owned_by.clone(),
                        provider_type: provider.name().to_string(),
                        display_name: Some(m.id.clone()),
                        name: None,
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
                    })
                    .collect();
                model_registry
                    .register_client(&client_id, provider.name(), ext_models)
                    .await;
            }
            Err(e) => {
                tracing::warn!("failed to list models from {}: {e}", provider.name());
            }
        }
    }

    let provider_count = providers.len();
    let mut state = proxy::ProxyState::new(cfg, account_mgr, model_registry, provider_count);
    state.kiro_runtime = kiro_runtime;
    {
        let mut providers_guard = state.providers.write().await;
        *providers_guard = providers;
    }
    let state = Arc::new(state);

    info!("Rusuh starting on http://{}", addr);

    let app = router::build_router(state.clone())
        .layer(axum::middleware::from_fn_with_state(
            state,
            middleware::auth::api_key_auth,
        ))
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("Listening on {}", addr);
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{load_config_or_default, run_codex_device_login};
    use tempfile::{NamedTempFile, TempDir};

    #[test]
    fn load_config_or_default_uses_defaults_when_file_is_missing() {
        let cfg = load_config_or_default("/tmp/nonexistent_rusuh_main_config_xyz.yaml").unwrap();

        assert_eq!(cfg.listen_addr(), "0.0.0.0:8317");
        assert!(cfg.api_keys.is_empty());
    }

    #[test]
    fn load_config_or_default_loads_existing_config_file() {
        let file = NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "host: 127.0.0.1\nport: 9000\n").unwrap();

        let cfg = load_config_or_default(file.path().to_str().unwrap()).unwrap();

        assert_eq!(cfg.listen_addr(), "127.0.0.1:9000");
    }

    #[test]
    fn load_config_or_default_returns_error_for_invalid_yaml() {
        let file = NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "host: [\n").unwrap();

        let error = load_config_or_default(file.path().to_str().unwrap()).unwrap_err();
        let message = error.to_string();

        assert!(message.contains("host:"));
        assert!(message.contains("expected a string"));
    }

    #[tokio::test]
    async fn codex_device_login_command_persists_credentials_from_real_device_endpoints() {
        let temp = TempDir::new().expect("create temp dir");
        let cfg = rusuh::config::Config {
            auth_dir: temp.path().to_string_lossy().to_string(),
            ..Default::default()
        };

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

        std::env::set_var("RUSUH_CODEX_AUTH_BASE_URL", format!("http://{addr}"));

        let login_result = run_codex_device_login(&cfg).await;

        std::env::remove_var("RUSUH_CODEX_AUTH_BASE_URL");

        login_result.expect("device login command should succeed");

        let saved_count = std::fs::read_dir(temp.path())
            .expect("read auth directory")
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .path()
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("codex-") && name.ends_with(".json"))
            })
            .count();

        assert_eq!(saved_count, 1);
    }
}
