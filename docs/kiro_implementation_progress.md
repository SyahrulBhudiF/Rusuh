# KIRO Provider Implementation Progress

## Completed Tasks

### ✅ KIRO-006: AWS Event Stream Parser
**File:** `src/providers/kiro_stream.rs` (336 lines)

Implemented complete AWS Event Stream binary protocol parser:
- **Protocol parsing**: 12-byte prelude (total_length, headers_length, prelude_crc)
- **Header extraction**: Parses variable-length headers to extract `:event-type`
- **Payload handling**: Safely extracts JSON payloads between headers and message CRC
- **Safety checks**: Message size validation (10MB max), header bounds checking
- **Error handling**: Three error types (Fatal, Malformed, Io)
- **Zero-copy design**: Uses `Bytes` for efficient payload handling

**Test Coverage:** 13 tests (4 unit + 9 integration)
- Event type extraction
- Header value type skipping (9 types supported)
- JSON payload parsing
- Single/multiple message parsing
- Empty payload handling
- EOF detection
- Size validation (too large/small)
- Header bounds validation
- Usage/tool use event parsing

**Documentation:** `docs/kiro_event_stream.md` - Complete protocol reference

### ✅ KIRO-007: KiroProvider Struct (Partial)
**File:** `src/providers/kiro.rs` (370 lines)

Implemented core provider structure:
- **KiroProvider struct**: Account management, token state, HTTP client, retry config
- **TokenState**: Runtime token management with RwLock for concurrent refresh
- **RetryConfig**: Configurable retry parameters (max retries, delays, timeouts)
- **Provider trait**: Skeleton implementation with `list_models()` support
- **Token extraction**: Parse KiroTokenData from AuthRecord metadata
- **Endpoint selection**: Support for CodeWhisperer and AmazonQ endpoints
- **Retry helpers**: Error classification, status code checking, exponential backoff

**Status:** Compiles cleanly, but incomplete:
- ❌ Token refresh not implemented (returns error)
- ❌ Chat completion not implemented (returns error)
- ❌ Streaming not implemented (returns error)

## Next Steps

### KIRO-008: Add KIRO Model Definitions
**File:** `src/providers/static_models.rs`
**Estimated:** 60 lines

Add KIRO model definitions to static model list:
- Claude 3.5 Sonnet (CodeWhisperer & AmazonQ)
- Claude 3.5 Haiku (AmazonQ)
- GPT-4o (CodeWhisperer)

### KIRO-009: Implement Token Refresh Logic
**File:** `src/providers/kiro.rs`
**Estimated:** 180 lines

Complete the `ensure_access_token()` method:
- Implement refresh based on `auth_method` (builder-id, social, idc)
- Call appropriate refresh functions from `kiro_sso.rs` / `kiro_social.rs`
- Persist refreshed tokens to disk
- Update in-memory state

### KIRO-010: Implement Request Translator
**File:** `src/providers/kiro_translator.rs`
**Estimated:** 250 lines

OpenAI → KIRO (Claude format) request transformation:
- Convert OpenAI messages to Claude format
- Handle system messages
- Map tool definitions
- Set generation parameters (temperature, max_tokens, etc.)

### KIRO-011: Implement Response Translator
**File:** `src/providers/kiro_translator.rs`
**Estimated:** 300 lines

KIRO (Claude SSE) → OpenAI SSE response transformation:
- Parse AWS Event Stream messages
- Extract event types and payloads
- Convert to OpenAI SSE format
- Handle tool calls, usage events, stop reasons
- Filter out UI-specific events (followupPromptEvent)

### KIRO-012: Integrate into AccountManager
**File:** `src/auth/manager.rs`
**Estimated:** 120 lines

Auto-discover and load `kiro_*.json` files:
- Scan `auths/` directory for KIRO auth files
- Parse and validate KIRO token data
- Add to account registry

### KIRO-013: Add to Provider Registry
**File:** `src/providers/registry.rs`
**Estimated:** 100 lines

Build KiroProvider instances from auth records:
- Detect KIRO auth records
- Create KiroProvider instances
- Add to provider list

## Architecture Summary

```
┌─────────────────────────────────────────────────────────────┐
│ KiroProvider                                                │
├─────────────────────────────────────────────────────────────┤
│ - account_name: String                                      │
│ - token: RwLock<TokenState>                                 │
│ - client: reqwest::Client                                   │
│ - auth_file_path: PathBuf                                   │
│ - endpoint: String (CodeWhisperer/AmazonQ)                  │
│ - retry_config: RetryConfig                                 │
├─────────────────────────────────────────────────────────────┤
│ Methods:                                                    │
│ - ensure_access_token() → refresh with 50min skew          │
│ - is_retryable_error() → classify errors                   │
│ - is_retryable_status() → check HTTP status                │
│ - calculate_retry_delay() → exponential backoff + jitter   │
│ - list_models() → static model list                        │
│ - chat_completion() → not supported (streaming only)       │
│ - chat_completion_stream() → TODO                          │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ AWS Event Stream Parser (kiro_stream.rs)                   │
├─────────────────────────────────────────────────────────────┤
│ EventStreamParser<R: Read>                                  │
│ - read_message() → parse binary protocol                   │
│ - parse_all() → collect all messages                       │
├─────────────────────────────────────────────────────────────┤
│ EventStreamMessage                                          │
│ - event_type: String                                        │
│ - payload: Bytes (JSON)                                     │
└─────────────────────────────────────────────────────────────┘
```

## Dependencies

**Auth Layer (Already Implemented):**
- `src/auth/kiro.rs` - OAuth constants and types ✅
- `src/auth/kiro_sso.rs` - AWS SSO OIDC client ✅
- `src/auth/kiro_social.rs` - Social OAuth client ✅

**Provider Layer (In Progress):**
- `src/providers/kiro_stream.rs` - Event Stream parser ✅
- `src/providers/kiro.rs` - Provider struct ⚠️ (partial)
- `src/providers/kiro_translator.rs` - Request/response translation ❌

**Integration (Not Started):**
- `src/auth/store.rs` - Add Kiro variant to AuthRecord ❌
- `src/auth/manager.rs` - Auto-discover kiro_*.json ❌
- `src/providers/registry.rs` - Build KiroProvider instances ❌
- `src/providers/model_registry.rs` - Register KIRO models ❌
- `src/providers/static_models.rs` - Add KIRO model list ❌

## Testing Status

**Unit Tests:**
- ✅ AWS Event Stream parser (13 tests, all passing)
- ❌ KIRO provider (not yet implemented)
- ❌ Request/response translation (not yet implemented)

**Integration Tests:**
- ❌ Full OAuth flows (not yet implemented)
- ❌ Chat completion streaming (not yet implemented)
- ❌ Token refresh (not yet implemented)

## Known Issues

1. **Token refresh not implemented** - `ensure_access_token()` returns error
2. **Streaming not implemented** - `chat_completion_stream()` returns error
3. **Non-streaming not supported** - KIRO requires streaming mode
4. **No request/response translation** - Need translator module
5. **Not integrated** - Not yet wired into AccountManager/Registry

## Estimated Completion

**Phase 2 (Provider Implementation):** 60% complete
- ✅ Event Stream parser
- ✅ Provider struct skeleton
- ❌ Token refresh
- ❌ Request translator
- ❌ Response translator

**Remaining work:** ~730 lines across 3 tasks (KIRO-008, KIRO-009, KIRO-010, KIRO-011)
