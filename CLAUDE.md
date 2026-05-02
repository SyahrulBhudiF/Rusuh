# Rusuh Rewrite Notes

Keep this file short and focused on parity gaps. Do not re-document behavior already implemented in `Rusuh` unless it affects current work.

## Goal

Recreate the behavioral contract of `CLIProxyAPIPlus` in Rust, not just the API surface.

Prioritize:
- provider-specific auth/login behavior
- token persistence and refresh lifecycle
- auth-aware model registration and routing
- management OAuth callback/session behavior
- request translation differences between chat, responses, and provider-native APIs

Prefer preserving runtime behavior over exact file-format parity.

## Already in place in `Rusuh`

These areas exist and usually do **not** need more context here unless you are changing them:
- filesystem auth store and auth manager foundations
- Antigravity auth/runtime flow
- Kiro auth/runtime integration, registration, quota, cooldown, and auth-aware routing
- Zed cloud provider integration (Track A: opencode-zed-auth parity, Track B: zed2api foundation)
- management auth-file CRUD plus existing Antigravity/Kiro web flows
- model registry, balancing, alias rewrite foundation, dashboard read APIs

## Highest-priority reference files in `CLIProxyAPIPlus`

### Auth/store
- `sdk/auth/interfaces.go`
- `sdk/auth/manager.go`
- `sdk/auth/filestore.go`

### GitHub Copilot
- `sdk/auth/github_copilot.go`
- `internal/auth/copilot/copilot_auth.go`
- `internal/auth/copilot/oauth.go`
- `internal/auth/copilot/token.go`
- `internal/runtime/executor/github_copilot_executor.go`

### Codex / OpenAI OAuth
- `sdk/auth/codex.go`
- `sdk/auth/codex_device.go`
- `internal/auth/codex/openai_auth.go`
- `internal/auth/codex/pkce.go`
- `internal/auth/codex/oauth_server.go`
- `internal/auth/codex/jwt_parser.go`
- `internal/runtime/executor/codex_executor.go`
- `internal/runtime/executor/codex_websockets_executor.go`

### GitLab Duo
- `sdk/auth/gitlab.go`
- `internal/auth/gitlab/gitlab.go`
- `internal/runtime/executor/gitlab_executor.go`

### OAuth session/callback flow
- `internal/api/handlers/management/oauth_sessions.go`
- `internal/api/handlers/management/oauth_callback.go`
- `internal/api/server.go`

### Registry/routing
- `internal/registry/model_registry.go`
- `internal/util/provider.go`
- `internal/config/oauth_model_alias_defaults.go`

## Core invariants

### Provider behavior must stay provider-specific

Do not over-normalize auth flows. Copilot, Codex, and GitLab have materially different login, persistence, refresh, and execution rules.

### Auth store is an active repository

Parity-relevant behavior from `filestore.go`:
- `List(...)` scans auth JSON files and reconstructs auth records
- `Save(...)` may use provider-specific storage behavior, not only raw JSON serialization
- `Attributes["path"]` is populated after save
- `next_refresh_after` should be derived from expiry using an early-refresh buffer
- file load may enrich and rewrite metadata when required
- Windows auth IDs should be normalized to lowercase to avoid case-based duplicates

### Routing must be auth-instance aware

Do not treat availability as provider-wide only. Model availability, retry timing, suspension, and quota state can differ per auth/account.

### Centralize refresh policy lookup

Per-provider refresh lead times should be shared by runtime refresh and any background refresh logic.

## Remaining high-priority parity work

### 1. GitHub Copilot

Must preserve:
- persisted credential is the **GitHub OAuth token**, not the Copilot API token
- runtime exchanges GitHub token for short-lived Copilot API token from `https://api.github.com/copilot_internal/v2/token`
- per-auth in-memory API token cache with expiry buffer
- device-code login flow
- model normalization and model listing from trusted Copilot endpoints
- dynamic endpoint choice between `/chat/completions` and `/responses`
- Copilot-specific headers such as `User-Agent`, `Editor-Version`, `Editor-Plugin-Version`, `Openai-Intent`, and `Copilot-Integration-Id`
- alias handling for dotted Claude IDs is compatibility-critical

### 2. Codex / OpenAI OAuth

Implemented in current Rust rewrite:
- OAuth auth-code + PKCE login
- device flow that produces the same Codex auth record shape
- persisted token set includes `id_token`, `access_token`, `refresh_token`, `account_id`, `email`, `expired`, `last_refresh`
- refresh-token rotation support
- `refresh_token_reused` classified as non-retryable
- JWT parsing for account metadata and file naming
- strict request normalization for `/v1/responses` and `/v1/responses/compact`
- runtime reload + provider/model rebuild after auth mutations
- auth-aware selection via `selected_auth_id`
- execution-session sticky selected-auth routing via `execution_session_id` for chat/responses request paths

Deferred Codex parity:
- websocket transport implementation (`codex_websocket` executor path)
- websocket-specific sticky session lifecycle parity beyond current HTTP-path session mapping
- explicit selected-auth callback metadata plumbing (current behavior persists sticky selected auth server-side)

Important Codex request cleanup (implemented):
- strip unsupported fields like `previous_response_id`, `prompt_cache_retention`, and `safety_identifier`
- ensure `instructions` exists
- normalize model names before upstream call

### 3. GitLab Duo

GitLab auth is not just GitLab OAuth. It combines:
- source GitLab auth (`OAuth` or `PAT`)
- Duo direct-access discovery and refresh
- runtime delegation to native GitLab endpoints **or** synthetic Claude/Codex-style auth depending on discovered provider

Must preserve:
- explicit OAuth vs PAT modes
- persisted metadata containing both source auth data and Duo gateway metadata
- OAuth refresh for source token when needed
- Duo direct-access metadata refresh for both OAuth and PAT-backed auth
- synthetic gateway delegation when Duo reports Anthropic/OpenAI-like backing

### 4. Management OAuth callback rendezvous

Current `Rusuh` uses in-memory session flow, but parity requires the callback-file handoff pattern.

Must preserve:
- provider/state session registration
- provider canonicalization in one place
- strict state validation
- callback writing `.oauth-<provider>-<state>.oauth` into auth dir
- waiting login flow polling that file and finishing token exchange/persistence

State validation should be at least:
- non-empty
- max length 128
- no `/` or `\\`
- no `..`
- only alphanumeric plus `-`, `_`, `.`

### 5. Shared auth/execution plumbing still missing

Need parity for:
- pinned auth ID request metadata
- selected-auth callback plumbing
- execution session ID support
- sticky auth/session routing across multi-turn responses
- auth-aware websocket availability decisions per auth record/model
- built-in alias packs and dynamic alias generation
- registry-first `auto` model resolution from live availability

## Zed Cloud Provider (Implemented)

**Track A (opencode-zed-auth parity) - Complete foundation:**
- Auth parsing: `src/auth/zed.rs` - parses user_id + credential_json from auth files
- HTTP client: `src/providers/zed_client.rs` - endpoint helpers, stale token detection
- Request translation: `src/providers/zed_request.rs` - OpenAI format → Zed format with model normalization
- Response translation: `src/providers/zed_response.rs` - validates and extracts content from responses
- Streaming: `src/providers/zed_stream.rs` - JSON-lines → SSE conversion
- Provider runtime: `src/providers/zed_provider.rs` - token/model caching with 60s refresh buffer
- Registration: `src/providers/zed_registration.rs` - scans auth store, creates provider instances
- Credential import: `src/proxy/zed_import.rs` - POST /v0/management/zed/import endpoint
- Management endpoints: quota/billing check placeholders (single-account pattern like Kiro)

**Track B (zed2api parity) - Foundation in place:**
- Anthropic Messages API: `src/providers/zed_anthropic.rs` - converts between Anthropic and OpenAI formats, preserves thinking blocks
- Native-app login: `src/auth/zed_login.rs` - RSA keypair generation, login URL building
- Login endpoints: POST /v0/management/zed/login/initiate, GET /v0/management/zed/login/status (placeholders)

**Still needed for full parity:**
- Actual HTTP request execution (token refresh, completions, models fetch)
- Streaming request execution with JSON-lines parsing
- Native-app login callback server and credential decryption
- Integration with model registry and routing
- Actual quota/billing API calls

**Architecture notes:**
- Follows existing Kiro pattern: per-instance token caching, auth-file-backed providers
- Single-account quota checking (not aggregate multi-account)
- Credential import prevents overwrite, auto-adds .json extension
- All foundation modules have comprehensive test coverage

## Short implementation map

Recommended modules:
- `src/auth/github_copilot.rs`
- `src/auth/codex.rs`
- `src/auth/gitlab.rs`
- `src/auth/session.rs`
- `src/auth/callback.rs`
- `src/auth/refresh_policy.rs`
- `src/providers/github_copilot.rs`
- `src/providers/codex.rs`
- `src/providers/gitlab.rs`
- `src/providers/codex_websocket.rs`
- `src/proxy/execution_session.rs`

## Tricky parts checklist

- Copilot endpoint selection depends on request shape and model
- Copilot model listing should only trust intended upstream hosts
- Codex request cleanup is mandatory
- GitLab may choose native endpoints or synthetic delegation at runtime
- callback filenames must remain provider/state scoped and traversal-safe
- Windows path normalization matters for auth identity deduplication
- `auto` model resolution should use live registry availability, not static defaults

## Codex test coverage (implemented)

- `tests/codex_auth.rs`
- `tests/codex_device.rs`
- `tests/codex_jwt.rs`
- `tests/codex_provider.rs`
- `tests/codex_routing.rs`
- `tests/oauth.rs` (Codex callback-file and alias flow)
- `tests/http.rs` (Codex routing and execution-session sticky behavior)
- `tests/auth_files.rs` and `tests/dashboard_api.rs` (runtime visibility after auth mutation)

## Bottom line

The important architecture from `CLIProxyAPIPlus` is that provider auth, token refresh, model discovery, and request execution are tightly coupled. Preserve that coupling in `Rusuh` instead of flattening everything into overly generic layers.
