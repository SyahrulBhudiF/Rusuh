import { describe, expect, test } from 'bun:test'

import { buildOAuthTerminalFeedback } from './oauth-feedback'

describe('buildOAuthTerminalFeedback', () => {
  test('returns null for wait status', () => {
    expect(buildOAuthTerminalFeedback('wait')).toBeNull()
  })

  test('returns success feedback for ok status', () => {
    expect(buildOAuthTerminalFeedback('ok')).toEqual({
      type: 'success',
      title: 'OAuth login complete',
      detail: 'Account connected. Open Accounts to review it.',
    })
  })

  test('returns error feedback with backend message', () => {
    expect(buildOAuthTerminalFeedback('error', 'invalid_grant')).toEqual({
      type: 'error',
      title: 'OAuth login failed',
      detail: 'invalid_grant',
    })
  })

  test('returns default error feedback when message is empty', () => {
    expect(buildOAuthTerminalFeedback('error', '   ')).toEqual({
      type: 'error',
      title: 'OAuth login failed',
      detail: 'Unknown OAuth error.',
    })
  })
})
