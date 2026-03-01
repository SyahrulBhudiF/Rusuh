# Antigravity Full Feature Parity — Implementation Plan

> **IMPORTANT**: Use plan-execute skill to implement this plan task-by-task.

**Goal:** Complete Antigravity provider to full CLIProxyAPI parity — runtime token refresh, management API auth CRUD, web-triggered OAuth, multi-account lifecycle, project_id auto-discovery.

**Architecture:** Extend existing `AccountManager` → `AuthRecord` with status fields. Add management API handlers for auth file CRUD + web OAuth triggers. Wire token refresh into provider request path. All scoped to Antigravity only — other providers remain stubs.

**Tech Stack:** Axum handlers, reqwest, serde_json, tokio, existing `FileTokenStore`/`AccountManager`

---

## Task 1: Extend `AuthRecord` with status fields

**File:** `src/auth/store.rs`

Add fields to `AuthRecord` matching Go `Auth` struct:
```rust
pub status: AuthStatus,          // Active, Disabled, Suspended
pub status_message: String,
pub last_refreshed_at: Option<DateTime<Utc>>,
pub next_retry_after: Option<DateTime<Utc>>,
```

Add enum:
```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum AuthStatus {
    #[default]
    Active,
    Disabled,
    Suspended,
}
```

Update `read_auth_file()` to populate `status` from `disabled` field.
Update `save()` to persist `disabled` from status.

**Verify:** `cargo test` passes. Existing auth files load correctly with new defaults.

---

## Task 2: Runtime token refresh in AntigravityProvider

**File:** `src/providers/antigravity.rs`

The Go executor calls `ensureAccessToken()` on every request — if token is <50min from expiry, it auto-refreshes using the refresh_token.

Currently our `AntigravityProvider::chat()` uses a stored `access_token` from the auth record but never refreshes it.

Steps:
1. Add `refresh_token: Option<String>` and `token_expiry: Option<DateTime<Utc>>` to `AntigravityProvider`
2. Add `ensure_access_token(&self) -> Result<String>` method:
   - If access_token exists AND token_expiry > now + 50min → return it
   - Otherwise call `refresh_access_token()` from `antigravity_login.rs`
   - Update internal state (need `RwLock` or similar)
   - Also update the auth file on disk via store
3. Call `ensure_access_token()` at the start of `chat()` instead of using the static token

**Key:** Match Go's `refreshSkew = 3000s` (50 minutes). Token lifetime is ~3600s (1 hour).

**Verify:** Provider compiles. Token refresh path is callable.

---

## Task 3: Project ID auto-discovery at load time

**File:** `src/auth/store.rs` and `src/auth/antigravity_login.rs`

Go's `readAuthFile()` checks if `project_id` is missing, and if so:
1. Extracts `access_token` from metadata
2. Calls `FetchAntigravityProjectID()` (loadCodeAssist)
3. Persists the project_id back to the JSON file

Add this to `FileTokenStore::read_auth_file()`:
1. If `provider == "antigravity"` and `project_id` is empty and `access_token` exists:
   - Spawn a background task to fetch project_id
   - Don't block loading — just update the file later
2. Alternative: do it during `AccountManager::reload()` after all files are loaded

**Verify:** Load an antigravity auth file without project_id → it gets auto-filled.

---

## Task 4: Management API — List auth files

**File:** `src/proxy/management.rs`

Add handler: `GET /v0/management/auth-files`

Response matches Go:
```json
{
  "files": [
    {
      "id": "antigravity-user@gmail.com.json",
      "name": "antigravity-user@gmail.com.json",
      "type": "antigravity",
      "provider": "antigravity",
      "label": "user@gmail.com",
      "email": "user@gmail.com",
      "status": "active",
      "disabled": false,
      "source": "file",
      "size": 1234,
      "modtime": "2025-01-01T00:00:00Z",
      "created_at": "2025-01-01T00:00:00Z",
      "updated_at": "2025-01-01T00:00:00Z"
    }
  ]
}
```

Wire into `ProxyState` which already has `account_mgr: Arc<AccountManager>`.

**Verify:** `curl localhost:8317/v0/management/auth-files` returns loaded auth files.

---

## Task 5: Management API — Upload/Delete auth files

**File:** `src/proxy/management.rs`

Add handlers:
- `POST /v0/management/auth-files?name=antigravity-foo.json` — raw JSON body saved to auth-dir
- `DELETE /v0/management/auth-files?name=antigravity-foo.json` — delete file + reload
- `DELETE /v0/management/auth-files?all=true` — delete all auth files

After upload/delete, call `account_mgr.reload()` to refresh in-memory state.

**Verify:** Upload a JSON file → appears in `GET /auth-files`. Delete → gone.

---

## Task 6: Management API — Patch auth file status & fields

**File:** `src/proxy/management.rs`

Add handlers:
- `PATCH /v0/management/auth-files/status` — body `{"name":"x.json","disabled":true}` → toggle status
- `PATCH /v0/management/auth-files/fields` — body `{"name":"x.json","priority":5}` → update metadata

After patching, update file on disk + call `account_mgr.reload()`.

**Verify:** Disable an auth file → it no longer appears in provider list. Re-enable → it's back.

---

## Task 7: Management API — Download auth file

**File:** `src/proxy/management.rs`

Add handler:
- `GET /v0/management/auth-files/download?name=antigravity-foo.json` — return raw JSON with `Content-Disposition: attachment`

**Verify:** Download returns valid JSON matching the file on disk.

---

## Task 8: Management API — Web OAuth trigger for Antigravity

**File:** `src/proxy/management.rs`

Add handler:
- `GET /v0/management/antigravity-auth-url` — starts Antigravity OAuth flow via HTTP (not CLI):
  1. Generate state + build auth URL
  2. Start callback server (or reuse management server's callback route)
  3. Return `{"status":"ok","url":"https://accounts.google.com/...","state":"..."}`
  4. Background task waits for callback, exchanges code, saves token, reloads accounts

Add callback route:
- `GET /antigravity/callback?code=...&state=...` — receives OAuth callback, writes to file, completes flow

This enables web UI and external tooling to trigger login without CLI.

**Verify:** `curl localhost:8317/v0/management/antigravity-auth-url` returns auth URL. Opening URL + completing OAuth saves credentials.

---

## Task 9: Management API — Auth status endpoint

**File:** `src/proxy/management.rs`

Add handler:
- `GET /v0/management/auth-status?state=...` — poll OAuth session status (pending/complete/error)

This is needed for web UI to know when login is done.

Add in-memory OAuth session tracker:
```rust
struct OAuthSession {
    provider: String,
    status: OAuthStatus,  // Pending, Complete, Error
    error: Option<String>,
}
```

**Verify:** After triggering auth URL, polling status returns "pending" → "complete" after callback.

---

## Task 10: Management API — Get auth file models

**File:** `src/proxy/management.rs`

Add handler:
- `GET /v0/management/auth-files/models?name=antigravity-user@gmail.com.json` — returns models supported by this specific auth file

Queries `ModelRegistry` for models registered by the provider client matching this auth file.

**Verify:** Returns model list for an antigravity auth file.

---

## Task 11: Update router to register all management routes

**File:** `src/router/mod.rs`, `src/proxy/management.rs`

Register all new management routes under `/v0/management/`:
- auth-files (GET, POST, DELETE)
- auth-files/status (PATCH)
- auth-files/fields (PATCH)
- auth-files/download (GET)
- auth-files/models (GET)
- antigravity-auth-url (GET)
- auth-status (GET)

Add OAuth callback route at top level:
- `/antigravity/callback` (GET)

**Verify:** All routes respond. `cargo test` passes including http integration tests.

---

## Task 12: Update README

**File:** `README.md`

Add:
- Multi-account setup section (login multiple times)
- Management API auth endpoints documentation
- Web OAuth trigger flow documentation
- Token refresh explanation

**Verify:** README renders correctly.

---

## Summary: File Changes

| File | Change |
|------|--------|
| `src/auth/store.rs` | Add `AuthStatus`, status fields to `AuthRecord` |
| `src/providers/antigravity.rs` | Runtime token refresh via `ensure_access_token()` |
| `src/auth/antigravity_login.rs` | Export helpers for management API use |
| `src/auth/manager.rs` | Add `get_by_id()`, `update()`, project_id backfill |
| `src/proxy/management.rs` | Auth file CRUD + web OAuth + status tracking |
| `src/router/mod.rs` | Register new management routes |
| `README.md` | Documentation updates |
