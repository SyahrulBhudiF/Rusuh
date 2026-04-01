import { describe, expect, test } from 'bun:test'

import {
  formatOauthExpiryHint,
  parseOauthStateFromRedirectUrl,
  resolveTrackedOauthSession,
  type TrackedOauthStates,
} from './add-account-page.oauth'

describe('add-account-page OAuth session helpers', () => {
  test('keeps tracked OAuth states independent per provider', () => {
    const trackedStates: TrackedOauthStates = {
      antigravity: 'ag-state',
      codex: 'codex-state',
      zed: 'zed-session',
      'github-copilot': 'copilot-state',
    }

    expect(trackedStates.antigravity).toBe('ag-state')
    expect(trackedStates.codex).toBe('codex-state')
    expect(trackedStates.zed).toBe('zed-session')
    expect(trackedStates['github-copilot']).toBe('copilot-state')
    expect(trackedStates.kiro).toBeUndefined()
  })

  test('resolves the tracked session that matches callback state', () => {
    const trackedStates: TrackedOauthStates = {
      antigravity: 'ag-state',
      codex: 'codex-state',
    }

    expect(
      resolveTrackedOauthSession(
        'antigravity',
        'http://localhost:3456/codex/callback?code=abc&state=codex-state',
        trackedStates,
      ),
    ).toEqual({
      provider: 'codex',
      state: 'codex-state',
    })
  })

  test('falls back to the current provider state when callback state is missing', () => {
    const trackedStates: TrackedOauthStates = {
      antigravity: 'ag-state',
      codex: 'codex-state',
    }

    expect(
      resolveTrackedOauthSession(
        'antigravity',
        'http://localhost:3456/antigravity/callback?code=abc',
        trackedStates,
      ),
    ).toEqual({
      provider: 'antigravity',
      state: 'ag-state',
    })
  })

  test('parses state query param from callback URLs', () => {
    expect(
      parseOauthStateFromRedirectUrl(
        'http://localhost:3456/antigravity/callback?code=abc&state=ag-state',
      ),
    ).toBe('ag-state')
  })

  test('returns null when callback URL state cannot be parsed', () => {
    expect(parseOauthStateFromRedirectUrl('not a url')).toBeNull()
  })

  test('formats short device-code expiry in seconds', () => {
    expect(formatOauthExpiryHint(45)).toBe('Code expires in about 45 seconds.')
  })

  test('formats longer device-code expiry in minutes', () => {
    expect(formatOauthExpiryHint(180)).toBe('Code expires in about 3 minutes.')
  })

  test('returns null when device-code expiry is missing or invalid', () => {
    expect(formatOauthExpiryHint(undefined)).toBeNull()
    expect(formatOauthExpiryHint(0)).toBeNull()
    expect(formatOauthExpiryHint(Number.NaN)).toBeNull()
  })
})
