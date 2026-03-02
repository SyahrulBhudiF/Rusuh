# Rusuh

Rust rewrite of [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) — proxies Gemini CLI, Antigravity, Codex, Claude Code, Qwen Code, iFlow behind OpenAI/Claude/Gemini-compatible API.

## Quick Reference

```bash
cargo build                    # build
cargo run                      # run (serves on :8317)
cargo run -- --config cfg.yaml # run with custom config
cargo clippy                   # lint
cargo test                     # all tests (80 tests across 8 files)
cargo test test_name           # single test
cargo test --test balancer     # single test file
cargo fmt --check              # format check
```

No rustfmt.toml/clippy.toml — use defaults.

## Project Structure

```
src/
├── main.rs              # Entry, CLI dispatch, server bootstrap, API key auto-generation
├── lib.rs               # Module re-exports
├── error.rs             # AppError (thiserror), AppResult<T>, IntoResponse
├── auth/
│   ├── cli.rs           # Clap CLI definitions
│   ├── manager.rs       # AccountManager — loads/reloads auth records, project_id auto-fetch
│   ├── store.rs         # FileTokenStore, AuthRecord, AuthStatus enum
│   ├── antigravity.rs   # OAuth constants
│   └── antigravity_login.rs  # Full OAuth flow (login, refresh, exchange)
├── config/mod.rs        # YAML config loading (serde_yaml), Config struct
├── models/mod.rs        # OpenAI-compatible request/response types
├── providers/
│   ├── mod.rs           # trait Provider (name, list_models, chat_completion, chat_completion_stream)
│   ├── antigravity.rs   # AntigravityProvider — Gemini API via Vertex AI, RwLock token refresh
│   ├── registry.rs      # build_providers() — creates providers from accounts
│   ├── model_registry.rs # ModelRegistry — tracks availability, suspension, cooldown
│   ├── model_info.rs    # Model metadata types
│   └── static_models.rs # Hardcoded model lists per provider channel
├── proxy/
│   ├── mod.rs           # ProxyState { config, providers, accounts, balancer, model_registry }
│   ├── handlers.rs      # HTTP handlers (health, list_models, chat_completions, route_chat, etc.)
│   ├── balancer.rs      # Load balancer (round-robin / least-connections / fill-first)
│   ├── stream.rs        # SSE streaming (buffered_sse_stream, antigravity_to_openai_transform)
│   └── management.rs    # Management API (/v0/management/*) with secret-key auth middleware
├── router/mod.rs        # Axum route table (build_router)
└── middleware/
    └── auth.rs          # API key auth middleware (Bearer header / x-api-key)
tests/
├── antigravity.rs      # Token expiry parsing, refresh skew, int64 helpers
├── auth_status.rs      # AuthStatus enum, effective_status, record metadata helpers
├── balancer.rs         # Round-robin, fill-first, counters
├── config.rs           # YAML parsing, defaults, listen_addr
├── error.rs            # AppError variants, IntoResponse, is_transient, is_account_error
├── http.rs             # Integration: health, models, chat routing
├── model_registry.rs   # Registration, suspension, quota tracking
└── stream.rs           # SSE buffering, antigravity transform, passthrough
```

## Commits

Semantic commits: `feat:`, `fix:`, `refactor:`, `docs:`, `test:`, `chore:`.

## Code Style

- **Errors:** `thiserror` for `AppError` enum, `anyhow` in main/CLI only. Handler return: `Result<Response, AppError>`.
- **Type alias:** `AppResult<T> = Result<T, AppError>` (in `error.rs`).
- **State:** `Arc<ProxyState>` via Axum `State` extractor.
- **Providers:** `#[async_trait] trait Provider` — one file per provider in `providers/`.
- **Streaming:** `BoxStream = Pin<Box<dyn Stream<Item = AppResult<Bytes>> + Send>>`.
- **Serde:** `#[serde(rename = "kebab-case")]` for YAML config fields, `#[serde(skip_serializing_if = "Option::is_none")]` for optional API fields.
- **No `unwrap()`** in library code. `expect()` only with a clear reason string.
- **Imports:** group `std` → external crates → `crate::` with blank lines between.
- **Modules:** one file per provider, one file per handler group. Target files < 300 lines.
- **Tests:** Test case must be to /tests folder and use the bestpractice way
- Don't add tests for what the type system already guarantees.

## Architecture Notes

- Config from `config.yaml` (optional — defaults work). Hot-reloadable via `RwLock<Config>`.
- `ProxyState.providers: Vec<Arc<dyn Provider>>` — built at startup from discovered accounts.
- Request flow: `middleware::auth` → handler → `route_chat` → `resolve_oauth_model_alias` → `balancer.pick` → `Provider::chat_completion[_stream]`.
- Routes mirror Go original: `/v1/chat/completions`, `/v1/messages`, `/v1beta/models/...`, `/api/provider/:provider/v1/...`.
- Management API under `/v0/management/` — gated by `remote-management.secret-key`.
- Antigravity provider auto-refreshes tokens with 50min skew (matching Go `refreshSkew = 3000s`), persists to disk.
- AuthRecord tracks lifecycle status (`AuthStatus` enum: Active/Disabled/Error/Pending/Refreshing/Unknown).
- AccountManager auto-fetches `project_id` for antigravity accounts missing it during `reload()`.
- Currently only **Antigravity** provider implemented. Claude Messages and Gemini generate are stubs.

## Auth Architecture

Two separate auth layers:

| Layer | Scope | Config | Purpose |
|---|---|---|---|
| `middleware::auth` | `/v1/*`, `/v1beta/*`, `/api/provider/*` | `api-keys` | Client access to proxy endpoints |
| `management_auth` | `/v0/management/*` | `remote-management.secret-key` | Admin access to management API |

- **API key auto-generation:** If no `api-keys` configured at startup, a `rsk-<uuid>` key is generated and printed (session-only, not persisted).
- **Management API key CRUD:** `GET/PUT/PATCH/DELETE /v0/management/api-keys` — generate, list, add, update, delete API keys at runtime via `RwLock<Config>`.
- **Localhost restriction:** When `remote-management.allow-remote: false` (default), management routes only accept loopback connections.

## Skills & Guidelines

Pi has skills that **must** be loaded when the task matches:

| When | Skill |
|---|---|
| Building a feature | `build-feature` |
| Bug / test failure / unexpected behavior | `debugging` |
| Tracing root cause of deep errors | `root-cause-tracing` |
| Writing an implementation plan | `plan-write` |
| Executing a written plan | `plan-execute` |
| Design/spec refinement before coding | `plan-design` |
| Converting plans to PRDs | `prd` |
| Code review (project-wide) | `code-review` |
| Code review (git diff) | `code-review-git` |
| Opinion on code quality / smells | `wydt` |

Always load the skill file before starting the task.

## PRD

Implementation plan tracked in [`.claude/plans/prd.json`](.claude/plans/prd.json).
