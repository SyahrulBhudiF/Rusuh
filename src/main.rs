use rusuh::{auth, config, middleware, providers, proxy, router};
use std::path::PathBuf;
use std::sync::Arc;

use auth::cli::{Cli, Commands};
use auth::manager::AccountManager;
use clap::Parser;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

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
    let cfg = config::Config::load_optional(&cli.config)
        .unwrap_or_default()
        .unwrap_or_default();

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
        Commands::CodexLogin => {
            println!("Codex OAuth login not yet implemented (milestone 2)");
        }
        Commands::CodexDeviceLogin => {
            println!("Codex device-code login not yet implemented (milestone 2)");
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
            !k.is_empty()
                && !k.starts_with("your-api-key")
                && k != "changeme"
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

    // Build providers from loaded accounts + config
    let providers = providers::registry::build_providers(&cfg, &account_mgr).await;
    // Build model registry and register all provider models
    let model_registry = Arc::new(providers::model_registry::ModelRegistry::new());
    for (idx, provider) in providers.iter().enumerate() {
        let client_id = format!("{}_{}", provider.name(), idx);
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
    state.providers = providers;
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
