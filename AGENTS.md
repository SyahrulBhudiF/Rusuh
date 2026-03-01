# Rusuh

Rust rewrite of [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) — proxies Gemini CLI, Antigravity, Codex, Claude Code, Qwen Code, iFlow behind OpenAI/Claude/Gemini-compatible API.

## Quick Reference

```bash
cargo build                    # build
cargo run                      # run (serves on :8317)
cargo run -- --config cfg.yaml # run with custom config
cargo clippy                   # lint
cargo test                     # all tests
cargo test test_name           # single test
cargo fmt --check              # format check
```

No tests exist yet. No CI. No rustfmt.toml/clippy.toml — use defaults.

## Project Structure

```
src/
├── main.rs          # Entry, CLI dispatch, server bootstrap
├── error.rs         # AppError enum (thiserror) + IntoResponse
├── auth/            # OAuth flows & CLI subcommands (clap)
├── config/          # YAML config loading (serde_yaml)
├── models/          # OpenAI-compatible request/response types
├── providers/       # Provider trait + upstream implementations
├── proxy/           # ProxyState, handlers, management API
├── router/          # Axum route table
└── middleware/      # Tower middleware (empty)
```

## Commits

Semantic commits only:

```
feat: add gemini provider
fix: correct token refresh race
refactor: extract route_chat helper
docs: update config.example.yaml
test: add round-robin tests
chore: update dependencies
```

## Code Style

- **Errors:** `thiserror` for `AppError` enum, `anyhow` in main/CLI only. Handler return type: `Result<Response, AppError>`.
- **Type alias:** `AppResult<T> = Result<T, AppError>` (defined in `error.rs`).
- **State:** `Arc<ProxyState>` injected via Axum's `State` extractor.
- **Providers:** implement `#[async_trait] trait Provider` (one file per provider in `providers/`).
- **Streaming:** return `BoxStream` (`Pin<Box<dyn Stream<Item = AppResult<Bytes>> + Send>>`).
- **Serde:** use `#[serde(rename = "kebab-case")]` for YAML config fields, `#[serde(skip_serializing_if = "Option::is_none")]` for optional API fields.
- **No `unwrap()`** in library code. `expect()` only with a clear reason.
- **Imports:** group std → external crates → `crate::` with blank lines between.
- **Modules:** one file per provider, one file per handler group. Keep files < 300 lines.
- Don't add tests for what the type system already guarantees.

## Architecture Notes

- Config loaded from `config.yaml` (optional — defaults work).
- `ProxyState.providers: Vec<Arc<dyn Provider>>` — populated at startup, queried per-request.
- `ProxyState.config: RwLock<Config>` — hot-reloadable via management API.
- Routes mirror Go original: `/v1/chat/completions`, `/v1/messages`, `/v1beta/models/...`, `/api/provider/:provider/v1/...`.
- Management API nested under `/v0/management/` (localhost-only).

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
