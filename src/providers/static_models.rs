//! Static model definitions catalog — mirrors Go `model_definitions_static_data.go`.

use super::model_info::{ExtModelInfo, ThinkingSupport};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn thinking(min: i32, max: i32, zero: bool, dynamic: bool) -> Option<ThinkingSupport> {
    Some(ThinkingSupport {
        min,
        max,
        zero_allowed: zero,
        dynamic_allowed: dynamic,
        levels: vec![],
    })
}

fn thinking_levels(levels: &[&str]) -> Option<ThinkingSupport> {
    Some(ThinkingSupport {
        min: 0,
        max: 0,
        zero_allowed: false,
        dynamic_allowed: false,
        levels: levels.iter().map(|s| s.to_string()).collect(),
    })
}

fn thinking_full(
    min: i32,
    max: i32,
    zero: bool,
    dynamic: bool,
    levels: &[&str],
) -> Option<ThinkingSupport> {
    Some(ThinkingSupport {
        min,
        max,
        zero_allowed: zero,
        dynamic_allowed: dynamic,
        levels: levels.iter().map(|s| s.to_string()).collect(),
    })
}

fn m(id: &str, created: i64, owned_by: &str, ptype: &str, display: &str) -> ExtModelInfo {
    ExtModelInfo {
        id: id.into(),
        object: "model".into(),
        created,
        owned_by: owned_by.into(),
        provider_type: ptype.into(),
        display_name: Some(display.into()),
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
    }
}

// ── Claude ────────────────────────────────────────────────────────────────────

pub fn claude_models() -> Vec<ExtModelInfo> {
    let models = vec![
        {
            let mut m = m(
                "claude-haiku-4-5-20251001",
                1759276800,
                "anthropic",
                "claude",
                "Claude 4.5 Haiku",
            );
            m.context_length = 200000;
            m.max_completion_tokens = 64000;
            m.thinking = thinking(1024, 128000, true, false);
            m
        },
        {
            let mut m = m(
                "claude-sonnet-4-5-20250929",
                1759104000,
                "anthropic",
                "claude",
                "Claude 4.5 Sonnet",
            );
            m.context_length = 200000;
            m.max_completion_tokens = 64000;
            m.thinking = thinking(1024, 128000, true, false);
            m
        },
        {
            let mut m = m(
                "claude-sonnet-4-6",
                1771372800,
                "anthropic",
                "claude",
                "Claude 4.6 Sonnet",
            );
            m.context_length = 200000;
            m.max_completion_tokens = 64000;
            m.thinking = thinking(1024, 128000, true, false);
            m
        },
        {
            let mut m = m(
                "claude-opus-4-6",
                1770318000,
                "anthropic",
                "claude",
                "Claude 4.6 Opus",
            );
            m.context_length = 1000000;
            m.max_completion_tokens = 128000;
            m.description = Some(
                "Premium model combining maximum intelligence with practical performance".into(),
            );
            m.thinking = thinking(1024, 128000, true, false);
            m
        },
        {
            let mut m = m(
                "claude-opus-4-5-20251101",
                1761955200,
                "anthropic",
                "claude",
                "Claude 4.5 Opus",
            );
            m.context_length = 200000;
            m.max_completion_tokens = 64000;
            m.description = Some(
                "Premium model combining maximum intelligence with practical performance".into(),
            );
            m.thinking = thinking(1024, 128000, true, false);
            m
        },
        {
            let mut m = m(
                "claude-opus-4-1-20250805",
                1722945600,
                "anthropic",
                "claude",
                "Claude 4.1 Opus",
            );
            m.context_length = 200000;
            m.max_completion_tokens = 32000;
            m.thinking = thinking(1024, 128000, false, false);
            m
        },
        {
            let mut m = m(
                "claude-opus-4-20250514",
                1715644800,
                "anthropic",
                "claude",
                "Claude 4 Opus",
            );
            m.context_length = 200000;
            m.max_completion_tokens = 32000;
            m.thinking = thinking(1024, 128000, false, false);
            m
        },
        {
            let mut m = m(
                "claude-sonnet-4-20250514",
                1715644800,
                "anthropic",
                "claude",
                "Claude 4 Sonnet",
            );
            m.context_length = 200000;
            m.max_completion_tokens = 64000;
            m.thinking = thinking(1024, 128000, false, false);
            m
        },
        {
            let mut m = m(
                "claude-3-7-sonnet-20250219",
                1708300800,
                "anthropic",
                "claude",
                "Claude 3.7 Sonnet",
            );
            m.context_length = 128000;
            m.max_completion_tokens = 8192;
            m.thinking = thinking(1024, 128000, false, false);
            m
        },
        {
            let mut m = m(
                "claude-3-5-haiku-20241022",
                1729555200,
                "anthropic",
                "claude",
                "Claude 3.5 Haiku",
            );
            m.context_length = 128000;
            m.max_completion_tokens = 8192;
            m
        },
    ];
    models
}

// ── Gemini ────────────────────────────────────────────────────────────────────

fn gemini_gen_methods() -> Vec<String> {
    vec![
        "generateContent".into(),
        "countTokens".into(),
        "createCachedContent".into(),
        "batchGenerateContent".into(),
    ]
}

pub fn gemini_models() -> Vec<ExtModelInfo> {
    vec![
        {
            let mut m = m(
                "gemini-2.5-pro",
                1750118400,
                "google",
                "gemini",
                "Gemini 2.5 Pro",
            );
            m.name = Some("models/gemini-2.5-pro".into());
            m.version = Some("2.5".into());
            m.description = Some("Stable release (June 17th, 2025) of Gemini 2.5 Pro".into());
            m.input_token_limit = 1048576;
            m.output_token_limit = 65536;
            m.supported_generation_methods = gemini_gen_methods();
            m.thinking = thinking(128, 32768, false, true);
            m
        },
        {
            let mut m = m(
                "gemini-2.5-flash",
                1750118400,
                "google",
                "gemini",
                "Gemini 2.5 Flash",
            );
            m.name = Some("models/gemini-2.5-flash".into());
            m.version = Some("001".into());
            m.input_token_limit = 1048576;
            m.output_token_limit = 65536;
            m.supported_generation_methods = gemini_gen_methods();
            m.thinking = thinking(0, 24576, true, true);
            m
        },
        {
            let mut m = m(
                "gemini-2.5-flash-lite",
                1753142400,
                "google",
                "gemini",
                "Gemini 2.5 Flash Lite",
            );
            m.name = Some("models/gemini-2.5-flash-lite".into());
            m.version = Some("2.5".into());
            m.input_token_limit = 1048576;
            m.output_token_limit = 65536;
            m.supported_generation_methods = gemini_gen_methods();
            m.thinking = thinking(0, 24576, true, true);
            m
        },
        {
            let mut m = m(
                "gemini-3-pro-preview",
                1737158400,
                "google",
                "gemini",
                "Gemini 3 Pro Preview",
            );
            m.name = Some("models/gemini-3-pro-preview".into());
            m.version = Some("3.0".into());
            m.input_token_limit = 1048576;
            m.output_token_limit = 65536;
            m.supported_generation_methods = gemini_gen_methods();
            m.thinking = thinking_full(128, 32768, false, true, &["low", "high"]);
            m
        },
        {
            let mut m = m(
                "gemini-3.1-pro-preview",
                1771459200,
                "google",
                "gemini",
                "Gemini 3.1 Pro Preview",
            );
            m.name = Some("models/gemini-3.1-pro-preview".into());
            m.version = Some("3.1".into());
            m.input_token_limit = 1048576;
            m.output_token_limit = 65536;
            m.supported_generation_methods = gemini_gen_methods();
            m.thinking = thinking_full(128, 32768, false, true, &["low", "high"]);
            m
        },
        {
            let mut m = m(
                "gemini-3-flash-preview",
                1765929600,
                "google",
                "gemini",
                "Gemini 3 Flash Preview",
            );
            m.name = Some("models/gemini-3-flash-preview".into());
            m.version = Some("3.0".into());
            m.input_token_limit = 1048576;
            m.output_token_limit = 65536;
            m.supported_generation_methods = gemini_gen_methods();
            m.thinking = thinking_full(
                128,
                32768,
                false,
                true,
                &["minimal", "low", "medium", "high"],
            );
            m
        },
        {
            let mut m = m(
                "gemini-3-pro-image-preview",
                1737158400,
                "google",
                "gemini",
                "Gemini 3 Pro Image Preview",
            );
            m.name = Some("models/gemini-3-pro-image-preview".into());
            m.version = Some("3.0".into());
            m.input_token_limit = 1048576;
            m.output_token_limit = 65536;
            m.supported_generation_methods = gemini_gen_methods();
            m.thinking = thinking_full(128, 32768, false, true, &["low", "high"]);
            m
        },
    ]
}

// ── OpenAI/Codex ─────────────────────────────────────────────────────────────

pub fn openai_models() -> Vec<ExtModelInfo> {
    vec![
        {
            let mut m = m("gpt-5", 1754524800, "openai", "openai", "GPT 5");
            m.context_length = 400000;
            m.max_completion_tokens = 128000;
            m.supported_parameters = vec!["tools".into()];
            m.thinking = thinking_levels(&["minimal", "low", "medium", "high"]);
            m
        },
        {
            let mut m = m("gpt-5-codex", 1757894400, "openai", "openai", "GPT 5 Codex");
            m.context_length = 400000;
            m.max_completion_tokens = 128000;
            m.supported_parameters = vec!["tools".into()];
            m.thinking = thinking_levels(&["low", "medium", "high"]);
            m
        },
        {
            let mut m = m("gpt-5.2", 1765440000, "openai", "openai", "GPT 5.2");
            m.context_length = 400000;
            m.max_completion_tokens = 128000;
            m.supported_parameters = vec!["tools".into()];
            m.thinking = thinking_levels(&["none", "low", "medium", "high", "xhigh"]);
            m
        },
        {
            let mut m = m(
                "gpt-5.2-codex",
                1765440000,
                "openai",
                "openai",
                "GPT 5.2 Codex",
            );
            m.context_length = 400000;
            m.max_completion_tokens = 128000;
            m.supported_parameters = vec!["tools".into()];
            m.thinking = thinking_levels(&["low", "medium", "high", "xhigh"]);
            m
        },
        {
            let mut m = m(
                "gpt-5.3-codex",
                1770307200,
                "openai",
                "openai",
                "GPT 5.3 Codex",
            );
            m.context_length = 400000;
            m.max_completion_tokens = 128000;
            m.supported_parameters = vec!["tools".into()];
            m.thinking = thinking_levels(&["low", "medium", "high", "xhigh"]);
            m
        },
        {
            let mut m = m(
                "gpt-5.3-codex-spark",
                1770912000,
                "openai",
                "openai",
                "GPT 5.3 Codex Spark",
            );
            m.context_length = 128000;
            m.max_completion_tokens = 128000;
            m.supported_parameters = vec!["tools".into()];
            m.thinking = thinking_levels(&["low", "medium", "high", "xhigh"]);
            m
        },
    ]
}

// ── Qwen ──────────────────────────────────────────────────────────────────────

pub fn qwen_models() -> Vec<ExtModelInfo> {
    vec![
        {
            let mut m = m(
                "qwen3-coder-plus",
                1753228800,
                "qwen",
                "qwen",
                "Qwen3 Coder Plus",
            );
            m.context_length = 32768;
            m.max_completion_tokens = 8192;
            m
        },
        {
            let mut m = m(
                "qwen3-coder-flash",
                1753228800,
                "qwen",
                "qwen",
                "Qwen3 Coder Flash",
            );
            m.context_length = 8192;
            m.max_completion_tokens = 2048;
            m
        },
        {
            let mut m = m("coder-model", 1771171200, "qwen", "qwen", "Qwen 3.5 Plus");
            m.context_length = 1048576;
            m.max_completion_tokens = 65536;
            m
        },
    ]
}

// ── Antigravity static config ────────────────────────────────────────────────

pub struct AntigravityModelConfig {
    pub thinking: Option<ThinkingSupport>,
    pub max_completion_tokens: i32,
}

pub fn antigravity_model_config() -> Vec<(&'static str, AntigravityModelConfig)> {
    vec![
        (
            "gemini-2.5-flash",
            AntigravityModelConfig {
                thinking: thinking(0, 24576, true, true),
                max_completion_tokens: 0,
            },
        ),
        (
            "gemini-2.5-flash-lite",
            AntigravityModelConfig {
                thinking: thinking(0, 24576, true, true),
                max_completion_tokens: 0,
            },
        ),
        (
            "gemini-3-pro-high",
            AntigravityModelConfig {
                thinking: thinking_full(128, 32768, false, true, &["low", "high"]),
                max_completion_tokens: 0,
            },
        ),
        (
            "gemini-3-pro-image",
            AntigravityModelConfig {
                thinking: thinking_full(128, 32768, false, true, &["low", "high"]),
                max_completion_tokens: 0,
            },
        ),
        (
            "gemini-3.1-pro-high",
            AntigravityModelConfig {
                thinking: thinking_full(128, 32768, false, true, &["low", "high"]),
                max_completion_tokens: 0,
            },
        ),
        (
            "gemini-3.1-flash-image",
            AntigravityModelConfig {
                thinking: thinking_full(128, 32768, false, true, &["minimal", "high"]),
                max_completion_tokens: 0,
            },
        ),
        (
            "gemini-3-flash",
            AntigravityModelConfig {
                thinking: thinking_full(
                    128,
                    32768,
                    false,
                    true,
                    &["minimal", "low", "medium", "high"],
                ),
                max_completion_tokens: 0,
            },
        ),
        (
            "claude-sonnet-4-5-thinking",
            AntigravityModelConfig {
                thinking: thinking(1024, 128000, true, true),
                max_completion_tokens: 64000,
            },
        ),
        (
            "claude-opus-4-5-thinking",
            AntigravityModelConfig {
                thinking: thinking(1024, 128000, true, true),
                max_completion_tokens: 64000,
            },
        ),
        (
            "claude-opus-4-6-thinking",
            AntigravityModelConfig {
                thinking: thinking(1024, 128000, true, true),
                max_completion_tokens: 64000,
            },
        ),
        (
            "claude-sonnet-4-5",
            AntigravityModelConfig {
                thinking: None,
                max_completion_tokens: 64000,
            },
        ),
        (
            "claude-sonnet-4-6",
            AntigravityModelConfig {
                thinking: None,
                max_completion_tokens: 64000,
            },
        ),
        (
            "claude-sonnet-4-6-thinking",
            AntigravityModelConfig {
                thinking: thinking(1024, 128000, true, true),
                max_completion_tokens: 64000,
            },
        ),
    ]
}

// ── Zed ───────────────────────────────────────────────────────────────────────

pub fn zed_models() -> Vec<ExtModelInfo> {
    vec![
        m("claude-sonnet-4-6", 0, "zed", "zed", "Claude Sonnet 4.6"),
        m("claude-sonnet-4-5", 0, "zed", "zed", "Claude Sonnet 4.5"),
        m("claude-haiku-4-5", 0, "zed", "zed", "Claude Haiku 4.5"),
        m("gpt-5.4", 0, "zed", "zed", "GPT 5.4"),
        m("gpt-5.3-codex", 0, "zed", "zed", "GPT 5.3 Codex"),
        m("gpt-5.2", 0, "zed", "zed", "GPT 5.2"),
        m("gpt-5.2-codex", 0, "zed", "zed", "GPT 5.2 Codex"),
        m("gpt-5-mini", 0, "zed", "zed", "GPT 5 Mini"),
        m("gpt-5-nano", 0, "zed", "zed", "GPT 5 Nano"),
        m(
            "gemini-3.1-pro-preview",
            0,
            "zed",
            "zed",
            "Gemini 3.1 Pro Preview",
        ),
        m(
            "gemini-3-pro-preview",
            0,
            "zed",
            "zed",
            "Gemini 3 Pro Preview",
        ),
        m("gemini-3-flash", 0, "zed", "zed", "Gemini 3 Flash"),
    ]
}

// ── Lookup ────────────────────────────────────────────────────────────────────

/// Get static model definitions for a given provider channel.
pub fn static_models_by_channel(channel: &str) -> Vec<ExtModelInfo> {
    match channel.to_lowercase().trim() {
        "claude" => claude_models(),
        "gemini" | "gemini-cli" | "aistudio" | "vertex" => gemini_models(),
        "codex" | "openai" => openai_models(),
        "qwen" => qwen_models(),
        "zed" => zed_models(),
        "antigravity" => antigravity_model_config()
            .into_iter()
            .filter_map(|(id, cfg)| {
                if cfg.thinking.is_none() && cfg.max_completion_tokens == 0 {
                    return None;
                }
                let mut info = m(id, 0, "antigravity", "antigravity", id);
                info.thinking = cfg.thinking;
                info.max_completion_tokens = cfg.max_completion_tokens;
                Some(info)
            })
            .collect(),
        _ => vec![],
    }
}

/// Lookup a model by ID across all static definitions.
pub fn lookup_static_model(model_id: &str) -> Option<ExtModelInfo> {
    if model_id.is_empty() {
        return None;
    }
    let all_channels = ["claude", "gemini", "codex", "qwen", "zed"];
    for channel in all_channels {
        for model in static_models_by_channel(channel) {
            if model.id == model_id {
                return Some(model);
            }
        }
    }
    // Check antigravity config
    for (id, cfg) in antigravity_model_config() {
        if id == model_id {
            let mut info = m(id, 0, "antigravity", "antigravity", id);
            info.thinking = cfg.thinking;
            info.max_completion_tokens = cfg.max_completion_tokens;
            return Some(info);
        }
    }
    None
}
