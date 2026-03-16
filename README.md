# Rusuh

Rusuh is a Rust proxy server that exposes multiple coding/LLM backends behind OpenAI-, Claude-, and Gemini-compatible APIs.

Today the main implemented provider is:
- Antigravity

Also included:
- management API
- built-in dashboard UI
- Docker setup
- multi-account auth file handling

---

## What you get

- Rust + Axum 0.8 backend
- React dashboard served by the backend in production
- OpenAI-compatible chat endpoint
- Claude-compatible messages endpoint
- Gemini-compatible endpoint shape
- runtime API key management
- auth file upload / download / enable / disable
- Docker + docker compose

---

## Quick start

### 1. Clone and install

```bash
git clone https://github.com/SyahrulBhudiF/Rusuh.git
cd Rusuh
cargo build
```

### 2. Login

```bash
rusuh antigravity-login
```

This opens browser auth and stores credentials in:

```bash
~/.rusuh
```

### 3. Run server

```bash
cargo run
```

Default address:

```text
http://localhost:8317
```

### 4. Try request

```bash
curl http://localhost:8317/v1/chat/completions \
  -H "Authorization: Bearer <your-api-key>" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gemini-2.5-flash",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

> If no `api-keys` are configured, Rusuh auto-generates one at startup and prints it to terminal.

---

## Dashboard

Rusuh includes a dashboard UI.

### Development

Run backend and frontend separately:

```bash
# terminal 1
cargo run

# terminal 2
cd frontend
bun install
bun run dev
```

Open:
- frontend dev: `http://localhost:5173`
- backend: `http://localhost:8317`

### Production

Build frontend first:

```bash
cd frontend
bun install
bun run build
cd ..
cargo run
```

Open:
- `http://localhost:8317`

### Dashboard route split

Read-only dashboard data:
- `/dashboard/*`

Management mutations:
- `/v0/management/*`

Notes:
- `/dashboard/*` is for dashboard reads
- `/v0/management/*` requires `remote-management.secret-key`
- dashboard UI asks for that secret and stores it in tab session

---

## Docker

### Quick start

```bash
cp .env.example .env
cp config.example.yaml config.yaml
# edit config.yaml

docker compose up --build
```

Open:
- `http://localhost:8317`

### Compose env vars

Supported in `.env`:

```env
RUSUH_IMAGE=rusuh:local
RUSUH_PORT=8317
RUSUH_CONFIG_PATH=./config.yaml
RUSUH_AUTH_VOLUME=rusuh-auth
RUSUH_LOG=rusuh=info,tower_http=debug
```

See:
- [`.env.example`](.env.example)

### Healthcheck

Compose includes a healthcheck against:
- `GET /health`

Check it:

```bash
docker compose ps
```

### Persistent auth volume

Compose mounts auth storage to:

```text
/home/rusuh/.rusuh
```

That matches default config:

```yaml
auth-dir: "~/.rusuh"
```

If you change `auth-dir`, update the compose volume too.

---

## Configuration

Rusuh works with defaults, but a `config.yaml` is recommended.

Minimal example:

```yaml
host: ""
port: 8317

auth-dir: "~/.rusuh"

api-keys:
  - "your-api-key"

routing:
  strategy: "round-robin"

remote-management:
  allow-remote: false
  secret-key: "your-management-secret"
```

Full reference:
- [`config.example.yaml`](config.example.yaml)

Run with custom config:

```bash
cargo run -- --config config.yaml
```

---

## Authentication model

Rusuh has 2 auth layers.

### 1. Normal API auth
Used for:
- `/v1/*`
- `/v1beta/*`
- `/api/provider/*`

Send either:
- `Authorization: Bearer <api-key>`
- `x-api-key: <api-key>`

Example:

```bash
curl http://localhost:8317/v1/models \
  -H "Authorization: Bearer <your-api-key>"
```

### 2. Management auth
Used for:
- `/v0/management/*`

Send either:
- `Authorization: Bearer <management-secret>`
- `X-Management-Key: <management-secret>`

If `remote-management.secret-key` is empty:
- management API is disabled
- requests return 404

---

## Provider login

## Antigravity

Login:

```bash
rusuh antigravity-login
```

What happens:
1. browser opens OAuth flow
2. local callback receives code
3. tokens are saved under `~/.rusuh`
4. restart server to load new credentials

Restart after login:

```bash
cargo run
```

### Multi-account

You can login multiple accounts. Each auth file is loaded on startup.

### Token refresh

Rusuh auto-refreshes Antigravity tokens before expiry and saves refreshed data back to disk.

## Kiro

CLI login commands exist:

```bash
rusuh kiro-login --provider google
rusuh kiro-login --provider github
rusuh kiro-login --provider sso --start-url https://your-org.awsapps.com/start
```

Important:
- dashboard-side Kiro flow is currently being reworked toward device-code flow
- do not assume the old browser social flow is stable

---

## Main endpoints

### Public / client API

```text
GET  /health
GET  /v1/models
POST /v1/chat/completions
POST /v1/completions
POST /v1/responses
POST /v1/messages
GET  /v1beta/models
POST /v1beta/models/{model}:generateContent
POST /v1beta/models/{model}:streamGenerateContent
GET  /api/provider/{provider}/v1/models
POST /api/provider/{provider}/v1/chat/completions
POST /api/provider/{provider}/v1/messages
```

### Dashboard read API

```text
GET /dashboard/health
GET /dashboard/overview
GET /dashboard/accounts
GET /dashboard/api-keys
GET /dashboard/config
```

### Management API

```text
GET    /v0/management/status
GET    /v0/management/config
GET    /v0/management/api-keys
PUT    /v0/management/api-keys
PATCH  /v0/management/api-keys
DELETE /v0/management/api-keys
GET    /v0/management/auth-files
POST   /v0/management/auth-files
DELETE /v0/management/auth-files
GET    /v0/management/auth-files/download?name=...
PATCH  /v0/management/auth-files/status
PATCH  /v0/management/auth-files/fields
GET    /v0/management/oauth/start?provider=...
GET    /v0/management/oauth/status?state=...
```

---

## Useful examples

### Chat completion

```bash
curl http://localhost:8317/v1/chat/completions \
  -H "Authorization: Bearer <your-api-key>" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gemini-2.5-flash",
    "messages": [{"role": "user", "content": "Write a haiku"}]
  }'
```

### Streaming

```bash
curl http://localhost:8317/v1/chat/completions \
  -H "Authorization: Bearer <your-api-key>" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gemini-2.5-flash",
    "messages": [{"role": "user", "content": "Write a haiku"}],
    "stream": true
  }'
```

### Route to a specific provider

```bash
curl http://localhost:8317/api/provider/antigravity/v1/chat/completions \
  -H "Authorization: Bearer <your-api-key>" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gemini-2.5-flash",
    "messages": [{"role": "user", "content": "Hi"}]
  }'
```

### Use with OpenAI Python SDK

```python
from openai import OpenAI

client = OpenAI(
    base_url="http://localhost:8317/v1",
    api_key="rsk-your-key",
)

response = client.chat.completions.create(
    model="gemini-2.5-flash",
    messages=[{"role": "user", "content": "Hello!"}],
)

print(response.choices[0].message.content)
```

### List management auth files

```bash
curl http://localhost:8317/v0/management/auth-files \
  -H "Authorization: Bearer <management-secret>"
```

### Upload management auth file

```bash
curl -X POST "http://localhost:8317/v0/management/auth-files?name=antigravity-user@gmail.com.json" \
  -H "Authorization: Bearer <management-secret>" \
  -H "Content-Type: application/json" \
  --data-binary @antigravity-backup.json
```

### Disable account

```bash
curl -X PATCH http://localhost:8317/v0/management/auth-files/status \
  -H "Authorization: Bearer <management-secret>" \
  -H "Content-Type: application/json" \
  -d '{"name":"antigravity-user@gmail.com.json","disabled":true}'
```

---

## Frontend commands

```bash
cd frontend
bun install
bun run dev
bun run build
bun run typecheck
bun run lint
bun run format
bun run check
```

Frontend tooling:
- Vite
- React
- shadcn
- oxlint
- oxfmt
- zustand

---

## Backend commands

```bash
cargo build
cargo run
cargo run -- --config config.yaml
cargo clippy
cargo test
cargo test test_name
cargo test --test balancer
cargo fmt --check
```

---

## How Rusuh works

1. loads config
2. loads auth files from `auth-dir`
3. creates provider instances
4. registers models
5. accepts API requests
6. routes requests through provider + balancer
7. auto-refreshes tokens when needed
8. serves dashboard UI and management API

---

## Environment variables

```bash
RUST_LOG=rusuh=debug
RUST_LOG=rusuh=trace
```

---

## Project status

Current state:
- Antigravity path is the most complete
- dashboard management UI is available
- Docker setup is available
- some provider flows are still incomplete / evolving

If something feels off, check:
- `config.yaml`
- management secret
- auth files under `~/.rusuh`
- server logs
