//! API key authentication middleware.
//!
//! Validates `Authorization: Bearer <key>` or `x-api-key: <key>` headers
//! against the configured `api-keys` list. Skips auth for:
//! - `/health`
//! - `/v0/management/*` (localhost-only management API)
//! - When `api-keys` is empty (auth disabled)

pub mod auth;
