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
    /// Login to Claude Code via OAuth
    ClaudeLogin,
    /// Login to Qwen Code via OAuth
    QwenLogin,
    /// Login to iFlow via OAuth
    IflowLogin,
}
