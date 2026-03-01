# Rusuh

Rust rewrite of [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) — proxies Gemini CLI, Antigravity, Codex, Claude Code, Qwen Code, iFlow behind OpenAI/Claude/Gemini-compatible API endpoints.

Single binary, zero runtime dependencies, built with [Axum](https://github.com/tokio-rs/axum).

## Installation

**Requirements:** Rust 1.75+ (2021 edition)

```bash
git clone https://github.com/SyahrulBhudiF/Rusuh.git
cd Rusuh

# Install to ~/.cargo/bin (recommended — makes `rusuh` available globally)
cargo install --path .

# Or build without installing
cargo build --release
# Binary at ./target/release/rusuh
```

After `cargo install`, make sure `~/.cargo/bin` is in your PATH.

## Quick Start

```bash
# 1. Login with your Google account
rusuh antigravity-login

# 2. Start the proxy server
rusuh

# 3. Chat (from another terminal)
curl http://localhost:8317/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gemini-2.5-flash",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

> **Important:** You must restart `rusuh` after logging in. The server loads credentials at startup — if you login while the server is already running, it won't pick up the new credentials until restarted.

## Configuration

Rusuh works with **zero configuration** — just login and run. For customization, create a `config.yaml`:

```yaml
host: ""           # bind address ("" = all interfaces)
port: 8317         # listen port
auth-dir: "~/.rusuh"  # OAuth token storage

api-keys:          # incoming request auth (empty = disabled)
  - "your-secret-key"

debug: false
request-retry: 3   # retry on transient failures

routing:
  strategy: "round-robin"  # or "fill-first"

# proxy-url: "socks5://user:pass@127.0.0.1:1080"

# Provider API keys (optional)
# gemini-api-key:
#   - api-key: "AIzaSy..."
#
# codex-api-key:
#   - api-key: "sk-..."
#
# claude-api-key:
#   - api-key: "sk-ant-..."
#
# openai-compatibility:
#   - name: "openrouter"
#     base-url: "https://openrouter.ai/api/v1"
#     api-key-entries:
#       - api-key: "sk-or-v1-..."
#     models:
#       - name: "gpt-4"
#         alias: "or-gpt4"

# Model aliases — rewrite incoming model names
# oauth-model-alias:
#   default:
#     - name: "claude-sonnet-4-20250514"
#       alias: "sonnet"
```

See [`config.example.yaml`](config.example.yaml) for a full reference.

Run with a custom config:

```bash
rusuh --config my-config.yaml
```

## Authentication

### Antigravity Login (Google Cloud Code)

Antigravity is the primary provider — it uses Google OAuth to authenticate with the Cloud Code API.

```bash
rusuh antigravity-login
```

This will:
1. Open your browser to Google OAuth consent screen
2. Start a local callback server on port 51121
3. Exchange the authorization code for access + refresh tokens
4. Fetch your email and GCP project ID
5. Save credentials to `~/.rusuh/antigravity-<email>.json`

After login, **restart the server** to load the new credentials:

```bash
# Stop the running server (Ctrl+C), then:
rusuh
```

### Multi-Account Setup

You can login with **multiple Google accounts** — each gets its own credential file:

```bash
# Login with first account
rusuh antigravity-login
# → saved to ~/.rusuh/antigravity-user1@gmail.com.json

# Login with another account (different browser profile or incognito)
rusuh antigravity-login
# → saved to ~/.rusuh/antigravity-user2@gmail.com.json

# Restart to load both accounts
rusuh
```

At startup, Rusuh loads **all** credential files and creates a provider instance per account. Requests are distributed across accounts using the configured load balancing strategy (round-robin by default).

### Token Lifecycle

- **Access tokens** expire after ~1 hour
- **Refresh tokens** are long-lived and stored in the credential file
- Rusuh auto-refreshes access tokens before expiry (50-minute skew, matching Go CLIProxyAPI)
- Refreshed tokens are persisted back to disk automatically

### Credential File Format

Each file in `~/.rusuh/` is a JSON object:

```json
{
  "type": "antigravity",
  "email": "user@gmail.com",
  "access_token": "ya29.a0...",
  "refresh_token": "1//0e...",
  "expires_in": 3599,
  "project_id": "useful-fuze-a1b2c",
  "disabled": false
}
```

You can also manually create/copy credential files into `~/.rusuh/` and restart — they'll be auto-discovered.

### Other Providers (coming soon)

```bash
rusuh login              # Gemini / Google
rusuh codex-login        # OpenAI Codex
rusuh claude-login       # Claude Code
rusuh qwen-login         # Qwen Code
rusuh iflow-login        # iFlow
```

### Management API (planned)

In addition to CLI login, Rusuh will support managing auth files via HTTP:

```bash
# List all auth files
curl localhost:8317/v0/management/auth-files

# Upload a credential file
curl -X POST 'localhost:8317/v0/management/auth-files?name=antigravity-user@gmail.com.json' \
  -H 'Content-Type: application/json' \
  -d @~/.rusuh/antigravity-user@gmail.com.json

# Trigger OAuth login via HTTP (for web UI)
curl localhost:8317/v0/management/antigravity-auth-url
# → {"status":"ok","url":"https://accounts.google.com/...","state":"..."}

# Disable an account
curl -X PATCH localhost:8317/v0/management/auth-files/status \
  -H 'Content-Type: application/json' \
  -d '{"name":"antigravity-user@gmail.com.json","disabled":true}'

# Delete an account
curl -X DELETE 'localhost:8317/v0/management/auth-files?name=antigravity-user@gmail.com.json'
```

## API Endpoints

### OpenAI-compatible

```
GET  /v1/models
POST /v1/chat/completions
POST /v1/completions
POST /v1/responses
```

### Claude-compatible

```
POST /v1/messages
```

### Gemini-compatible

```
GET  /v1beta/models
POST /v1beta/models/{model}:generateContent
POST /v1beta/models/{model}:streamGenerateContent
```

### Amp provider routing

```
GET  /api/provider/{provider}/v1/models
POST /api/provider/{provider}/v1/chat/completions
POST /api/provider/{provider}/v1/messages
```

### Management (localhost only)

```
GET  /v0/management/status
GET  /v0/management/config
```

### Health

```
GET  /health
```

## Usage Examples

### Chat completion

```bash
curl http://localhost:8317/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gemini-2.5-flash",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

### Streaming

```bash
curl http://localhost:8317/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gemini-2.5-flash",
    "messages": [{"role": "user", "content": "Write a haiku"}],
    "stream": true
  }'
```

### List available models

```bash
curl http://localhost:8317/v1/models
```

### Route to specific provider

```bash
curl http://localhost:8317/api/provider/antigravity/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gemini-2.5-flash",
    "messages": [{"role": "user", "content": "Hi"}]
  }'
```

### Use with OpenAI SDK (Python)

```python
from openai import OpenAI

client = OpenAI(
    base_url="http://localhost:8317/v1",
    api_key="unused",  # no api-keys in config = auth disabled
)

response = client.chat.completions.create(
    model="gemini-2.5-flash",
    messages=[{"role": "user", "content": "Hello!"}],
)
print(response.choices[0].message.content)
```

### With API key auth

If you set `api-keys` in config, pass the key via header:

```bash
curl http://localhost:8317/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer your-secret-key" \
  -d '{"model": "gemini-2.5-flash", "messages": [{"role": "user", "content": "Hi"}]}'
```

Without `api-keys` configured, auth is disabled and no header is needed.

## Testing

```bash
# Run all tests (54 tests)
cargo test

# Run a specific test file
cargo test --test balancer
cargo test --test stream
cargo test --test http
cargo test --test model_registry
cargo test --test config
cargo test --test error

# Run a single test
cargo test round_robin_distributes_evenly

# Run tests in release mode
cargo test --release

# Lint
cargo clippy
```

### Test coverage

| File | Tests | What's covered |
|---|---|---|
| `tests/balancer.rs` | 8 | Round-robin distribution, fill-first, counters, subset cycling, edge cases |
| `tests/config.rs` | 8 | YAML parsing, defaults, listen address, model aliases, openai-compat |
| `tests/error.rs` | 6 | Transient/account error classification, HTTP status codes |
| `tests/http.rs` | 11 | Health, auth middleware (accept/reject/skip), models, chat completions, management |
| `tests/model_registry.rs` | 9 | Register/unregister, ref counting, multi-provider, quota, suspend/resume, reconciliation |
| `tests/stream.rs` | 12 | SSE buffering, partial chunks, Antigravity→OpenAI transform, tool calls, DONE sentinel |

## Project Structure

```
src/
├── main.rs              # Entry point, CLI dispatch, server bootstrap
├── lib.rs               # Library re-exports
├── error.rs             # AppError enum + IntoResponse
├── auth/
│   ├── antigravity.rs   # Antigravity OAuth constants
│   ├── antigravity_login.rs  # Full OAuth login flow
│   ├── cli.rs           # CLI subcommands (clap)
│   ├── manager.rs       # Account manager — loads credentials
│   └── store.rs         # File-based token storage
├── config/
│   └── mod.rs           # YAML config (serde)
├── middleware/
│   └── auth.rs          # API key auth middleware
├── models/
│   └── mod.rs           # OpenAI-compatible request/response types
├── providers/
│   ├── antigravity.rs   # Antigravity provider (request/response translation)
│   ├── model_info.rs    # ExtModelInfo type
│   ├── model_registry.rs # Global model registry with ref counting
│   ├── registry.rs      # Provider builder from config
│   └── static_models.rs # Static model catalogs
├── proxy/
│   ├── balancer.rs      # Round-robin / fill-first load balancer
│   ├── handlers.rs      # Route handlers (chat, models, etc.)
│   ├── management.rs    # Management API handlers
│   ├── mod.rs           # ProxyState
│   └── stream.rs        # SSE stream forwarding + buffering
└── router/
    └── mod.rs           # Axum route table
tests/
├── balancer.rs
├── config.rs
├── error.rs
├── http.rs
├── model_registry.rs
└── stream.rs
```

## How It Works

1. **Startup** — loads `config.yaml` (optional), reads OAuth tokens from `auth-dir`, builds providers
2. **Model registry** — each provider registers its models with ref counting
3. **Request routing** — incoming request → model alias resolution → registry lookup → provider selection via round-robin balancer
4. **Retry logic** — transient errors (5xx, timeout) retry same provider; account errors (401, 429) skip to next
5. **Streaming** — upstream SSE chunks are buffered across boundaries, transformed to OpenAI format, forwarded with proper headers

## Environment Variables

```bash
RUST_LOG=rusuh=debug     # Enable debug logging
RUST_LOG=rusuh=trace     # Trace-level logging
```
