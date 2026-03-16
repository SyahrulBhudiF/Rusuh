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
  -H "Authorization: Bearer <your-api-key>" \
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

api-keys:          # incoming request auth (auto-generated if empty)
  - "your-secret-key"

debug: false
request-retry: 3   # retry on transient failures

routing:
  strategy: "round-robin"  # or "fill-first"

# Management API — required for /v0/management/* routes
# Leave secret-key empty to disable management API entirely (404)
remote-management:
  allow-remote: false      # only localhost can access management routes
  secret-key: ""           # admin password for management API

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

## Dashboard UI

Rusuh now includes a Bun-managed React dashboard served by Axum in production from `frontend/dist`. Read endpoints live under `/dashboard/*`.

### Development workflow

Run backend and frontend in separate terminals:

```bash
# Terminal 1 — Rust backend
cargo run

# Terminal 2 — frontend dev server
cd frontend
bun install
bun run dev
bun run lint        # oxlint
bun run format      # oxfmt
bun run check       # typecheck + oxlint + oxfmt check
```

Vite proxies these backend paths to the Rust server so frontend development stays same-origin and CORS-free:

- `/dashboard`
- `/v0`
- `/v1`
- `/v1beta`
- `/api`
- `/health`

Dashboard pages currently cover:

- Overview
- Accounts
- API Keys
- Config

Management mutations in the dashboard require `remote-management.secret-key`. The UI asks for this secret and keeps it in tab session storage.

### Using the dashboard

In development, open:
- `http://localhost:5173` — Vite frontend
- `http://localhost:8317` — Rust backend API

When the dashboard loads:
1. Enter `remote-management.secret-key` in the auth prompt.
2. Use the dashboard tabs: Overview, Accounts, API Keys, Config.
3. Read data comes from `/dashboard/*`.
4. Mutations still go through `/v0/management/*`.

In production, after building the frontend and starting Rusuh, open:
- `http://localhost:8317`

Axum serves the built SPA and mounts API routes before the frontend fallback.


Frontend toolchain now uses Oxc-native tooling:
- `oxlint` for linting
- `oxfmt` for formatting via `frontend/.oxfmt.json`
- no ESLint / Prettier runtime workflow


### Production build workflow
Build frontend first, then backend:
```bash
cd frontend
bun install
bun run build
cd ..
cargo build --release
./target/release/rusuh
```

Open:
- `http://localhost:8317`
At runtime, Axum serves the built frontend from `frontend/dist`, with API routes mounted before the SPA fallback.


## Authentication

### API Key Auth

Rusuh requires API keys to access proxy endpoints. Two behaviors:

- **Keys in config** — uses them, logs count at startup
- **No keys configured** — auto-generates a `rsk-<uuid>` key, prints it to console (session-only, not persisted)

```
  ╔══════════════════════════════════════════════════════════════╗
  ║  Auto-generated API key (not persisted):                    ║
  ║  rsk-550e8400-e29b-41d4-a716-446655440000                   ║
  ║                                                             ║
  ║  Add to config.yaml under `api-keys:` to persist.           ║
  ╚══════════════════════════════════════════════════════════════╝
```

Pass the key via header:

```bash
curl http://localhost:8317/v1/chat/completions \
  -H "Authorization: Bearer rsk-550e8400-..." \
  -H "Content-Type: application/json" \
  -d '{"model": "gemini-2.5-flash", "messages": [{"role": "user", "content": "Hi"}]}'
```

Or via `x-api-key` header:

```bash
curl -H "x-api-key: rsk-550e8400-..." http://localhost:8317/v1/models
```

### Management API Key CRUD

Generate and manage API keys at runtime via the management API. Requires `remote-management.secret-key` in config.

```bash
# Generate a new key for a user
curl -X PATCH http://localhost:8317/v0/management/api-keys \
  -H "Authorization: Bearer <your-management-secret>" \
  -H "Content-Type: application/json" \
  -d '{"generate": true}'
# → {"generated":["rsk-..."],"api-keys":["rsk-auto-...","rsk-..."]}

# Generate multiple keys at once
curl -X PATCH http://localhost:8317/v0/management/api-keys \
  -H "Authorization: Bearer <your-management-secret>" \
  -H "Content-Type: application/json" \
  -d '{"generate": true, "count": 5}'

# List all keys
curl http://localhost:8317/v0/management/api-keys \
  -H "Authorization: Bearer <your-management-secret>"

# Add a custom key
curl -X PATCH http://localhost:8317/v0/management/api-keys \
  -H "Authorization: Bearer <your-management-secret>" \
  -H "Content-Type: application/json" \
  -d '{"value": "my-custom-key"}'

# Replace all keys
curl -X PUT http://localhost:8317/v0/management/api-keys \
  -H "Authorization: Bearer <your-management-secret>" \
  -H "Content-Type: application/json" \
  -d '["key1", "key2", "key3"]'

# Delete a key
curl -X DELETE "http://localhost:8317/v0/management/api-keys?value=rsk-..." \
  -H "Authorization: Bearer <your-management-secret>"
```

All changes are **hot-reloaded** — no restart needed.

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
  "expired": "2025-03-02T14:00:00Z",
  "timestamp": 1709388000000,
  "project_id": "useful-fuze-a1b2c",
  "status": "active",
  "disabled": false
}
```

You can also manually create/copy credential files into `~/.rusuh/` and restart — they'll be auto-discovered.

### KIRO Login (AWS CodeWhisperer)

KIRO provides AWS CodeWhisperer access via social OAuth (Google/GitHub) or AWS SSO.

**Social OAuth (Google/GitHub):**

```bash
# Login with Google
rusuh kiro-login --provider google

# Login with GitHub
rusuh kiro-login --provider github
```

This will:
1. Open your browser to KIRO OAuth consent screen
2. Start a local callback server on port 9876
3. Exchange the authorization code for AWS SSO tokens
4. Save credentials to `~/.rusuh/kiro-google-<email>.json` or `~/.rusuh/kiro-github-<username>.json`

**AWS SSO (Enterprise IDC):**

```bash
# Login with AWS Identity Center
rusuh kiro-login --provider sso --start-url https://your-org.awsapps.com/start
```

After login, **restart the server** to load the new credentials:

```bash
rusuh
```

**Multi-Account KIRO:**

You can login with multiple KIRO accounts (mix Google, GitHub, SSO):

```bash
rusuh kiro-login --provider google
rusuh kiro-login --provider github
rusuh kiro-login --provider sso --start-url https://company.awsapps.com/start

# Restart to load all accounts
rusuh
```

KIRO tokens auto-refresh before expiry (50-minute skew) and are persisted to disk.

### Other Providers (coming soon)
rusuh codex-login        # OpenAI Codex
rusuh claude-login       # Claude Code
rusuh qwen-login         # Qwen Code
rusuh iflow-login        # iFlow
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

### Management API

Requires `remote-management.secret-key` in config. Auth via `Authorization: Bearer <secret>` or `X-Management-Key` header.

#### Status & Config

```
GET  /v0/management/status
GET  /v0/management/config
```

#### API Key Management

```
GET    /v0/management/api-keys
PUT    /v0/management/api-keys
PATCH  /v0/management/api-keys
DELETE /v0/management/api-keys
```

#### Auth File Management

```
GET    /v0/management/auth-files
POST   /v0/management/auth-files
DELETE /v0/management/auth-files
GET    /v0/management/auth-files/download?name=...
PATCH  /v0/management/auth-files/status
PATCH  /v0/management/auth-files/fields
```

**List auth files:**

```bash
curl http://localhost:8317/v0/management/auth-files \
  -H "Authorization: Bearer <management-secret>"
# → {"auth-files":[{"id":"antigravity-user@gmail.com.json","type":"antigravity","status":"active","disabled":false,"updated_at":"2026-03-16T05:30:00Z","last_refreshed_at":"2026-03-16T05:30:00Z"}]}
```

**Upload auth file:**

```bash
curl -X POST "http://localhost:8317/v0/management/auth-files?name=antigravity-user@gmail.com.json" \
  -H "Authorization: Bearer <management-secret>" \
  -H "Content-Type: application/json" \
  --data-binary @antigravity-backup.json
# → {"status":"ok","name":"antigravity-user@gmail.com.json"}
```

**Download auth file:**

```bash
curl "http://localhost:8317/v0/management/auth-files/download?name=antigravity-user@gmail.com.json" \
  -H "Authorization: Bearer <management-secret>" \
  -o backup.json
```

**Disable/enable account:**

```bash
# Disable
curl -X PATCH http://localhost:8317/v0/management/auth-files/status \
  -H "Authorization: Bearer <management-secret>" \
  -H "Content-Type: application/json" \
  -d '{"name":"antigravity-user@gmail.com.json","disabled":true}'

# Enable
curl -X PATCH http://localhost:8317/v0/management/auth-files/status \
  -H "Authorization: Bearer <management-secret>" \
  -H "Content-Type: application/json" \
  -d '{"name":"antigravity-user@gmail.com.json","disabled":false}'
```

**Update editable auth-file fields:**

```bash
curl -X PATCH http://localhost:8317/v0/management/auth-files/fields \
  -H "Authorization: Bearer <management-secret>" \
  -H "Content-Type: application/json" \
  -d '{"name":"antigravity-user@gmail.com.json","prefix":"work","proxy_url":"socks5://127.0.0.1:1080","priority":1}'
```

**Delete auth file:**

```bash
curl -X DELETE "http://localhost:8317/v0/management/auth-files?name=antigravity-user@gmail.com.json" \
  -H "Authorization: Bearer <management-secret>"
```

```
GET    /v0/management/status
GET    /v0/management/config
GET    /v0/management/api-keys
PUT    /v0/management/api-keys
PATCH  /v0/management/api-keys
DELETE /v0/management/api-keys
```

#### OAuth Management

```
GET  /v0/management/oauth/start?provider=antigravity|kiro-google|kiro-github
GET  /v0/management/oauth/status?state=...
```

**Start OAuth flow via API:**

```bash
# Start Antigravity OAuth
curl "http://localhost:8317/v0/management/oauth/start?provider=antigravity" \
  -H "Authorization: Bearer <management-secret>"
# → {"status":"ok","url":"https://accounts.google.com/o/oauth2/v2/auth?...","state":"uuid","provider":"antigravity"}

# Start KIRO Google OAuth
curl "http://localhost:8317/v0/management/oauth/start?provider=kiro-google" \
  -H "Authorization: Bearer <management-secret>"

# Start KIRO GitHub OAuth
curl "http://localhost:8317/v0/management/oauth/start?provider=kiro-github" \
  -H "Authorization: Bearer <management-secret>"
```

**Poll OAuth status:**

```bash
curl "http://localhost:8317/v0/management/oauth/status?state=<uuid>" \
  -H "Authorization: Bearer <management-secret>"
# → {"status":"wait","provider":"antigravity"}  # pending
# → {"status":"ok","provider":"antigravity"}    # complete
# → {"status":"error","provider":"antigravity","error":"..."}
```

Workflow:
1. Call `/v0/management/oauth/start?provider=...` to get auth URL
2. Open URL in browser or use the dashboard Accounts page
3. Poll `/v0/management/oauth/status?state=...` until status is "ok" or "error"
4. Server automatically saves credentials and reloads accounts

### Health

```
GET  /health
```

## Usage Examples

### Chat completion

```bash
curl http://localhost:8317/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <your-api-key>" \
  -d '{
    "model": "gemini-2.5-flash",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

### Streaming

```bash
curl http://localhost:8317/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <your-api-key>" \
  -d '{
    "model": "gemini-2.5-flash",
    "messages": [{"role": "user", "content": "Write a haiku"}],
    "stream": true
  }'
```

### List available models

```bash
curl -H "Authorization: Bearer <your-api-key>" http://localhost:8317/v1/models
```

### Route to specific provider

```bash
curl http://localhost:8317/api/provider/antigravity/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <your-api-key>" \
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
    api_key="rsk-your-key-here",
)

response = client.chat.completions.create(
    model="gemini-2.5-flash",
    messages=[{"role": "user", "content": "Hello!"}],
)
print(response.choices[0].message.content)
```

## Testing

```bash
# Run all tests (80 tests)
cargo test
cargo test --test balancer
cargo test --test stream
cargo test --test http
cargo test --test model_registry
cargo test --test config
cargo test --test error
cargo test --test antigravity
cargo test --test auth_status
# Run a single test
cargo test round_robin_distributes_evenly
cargo test --release
# Lint
cargo clippy
```

### Test coverage

| File | Tests | What's covered |
|---|---|---|
| `tests/antigravity.rs` | 13 | Token expiry parsing (RFC3339, timestamp+expires_in, unix), refresh skew, int64 helpers |
| `tests/auth_status.rs` | 13 | AuthStatus enum (display, parse, serde, usable), effective_status, record metadata helpers |
| `tests/balancer.rs` | 8 | Round-robin distribution, fill-first, counters, subset cycling, edge cases |
| `tests/config.rs` | 8 | YAML parsing, defaults, listen address, model aliases, openai-compat |
| `tests/error.rs` | 6 | Transient/account error classification, HTTP status codes |
| `tests/http.rs` | 11 | Health, auth middleware (accept/reject/skip), models, chat completions, management |
| `tests/model_registry.rs` | 9 | Register/unregister, ref counting, multi-provider, quota, suspend/resume, reconciliation |
| `tests/stream.rs` | 12 | SSE buffering, partial chunks, Antigravity→OpenAI transform, tool calls, DONE sentinel |

## How It Works

1. **Startup** — loads `config.yaml` (optional), reads OAuth tokens from `auth-dir`, auto-fetches missing `project_id`, auto-generates API key if none configured
2. **Model registry** — each provider registers its models with ref counting
3. **Token refresh** — antigravity tokens auto-refresh 50min before expiry (matching Go `refreshSkew = 3000s`), persisted to disk
4. **Request routing** — incoming request → API key auth → model alias resolution → registry lookup → provider selection via round-robin balancer
5. **Retry logic** — transient errors (5xx, timeout) retry same provider; account errors (401, 429) skip to next
6. **Streaming** — upstream SSE chunks are buffered across boundaries, transformed to OpenAI format, forwarded with proper headers
7. **Management** — admin generates/manages API keys via `/v0/management/api-keys` (gated by `secret-key`)

## Environment Variables

```bash
RUST_LOG=rusuh=debug     # Enable debug logging
RUST_LOG=rusuh=trace     # Trace-level logging
```