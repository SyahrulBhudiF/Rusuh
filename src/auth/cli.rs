use clap::{Parser, Subcommand};

/// Rusuh — Rust reimplementation of CLIProxyAPI
#[derive(Debug, Parser)]
#[command(name = "rusuh", version, about)]
pub struct Cli {
    /// Path to config.yaml
    #[arg(short, long, default_value = "config.yaml")]
    pub config: String,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Start the proxy server (default when no subcommand given)
    Serve,
    /// Login to Google / Gemini
    Login,
    /// Login to Antigravity via OAuth
    AntigravityLogin,
    /// Login to Codex via OAuth
    CodexLogin,
    /// Login to Codex via device code flow
    CodexDeviceLogin,
    /// Login to GitHub Copilot via GitHub.com device flow
    #[command(name = "github-copilot-login")]
    GithubCopilotLogin,
    /// Login to Claude Code via OAuth
    ClaudeLogin,
    /// Login to Qwen Code via OAuth
    QwenLogin,
    /// Login to iFlow via OAuth
    IflowLogin,
    /// Login to KIRO (AWS CodeWhisperer) via social OAuth or SSO
    #[command(name = "kiro-login")]
    KiroLogin {
        /// OAuth provider: google, github, or sso
        #[arg(long, default_value = "google")]
        provider: String,
        /// AWS SSO start URL (required for --provider=sso)
        #[arg(long)]
        start_url: Option<String>,
    },
}
