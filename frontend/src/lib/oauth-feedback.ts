export type OAuthTerminalStatus = 'wait' | 'ok' | 'error'

type OAuthFeedback = {
  type: 'success' | 'error'
  title: string
  detail: string
}

export function buildOAuthTerminalFeedback(
  status: OAuthTerminalStatus,
  error?: string,
): OAuthFeedback | null {
  if (status === 'wait') {
    return null
  }

  if (status === 'ok') {
    return {
      type: 'success',
      title: 'OAuth login complete',
      detail: 'Account connected. Open Accounts to review it.',
    }
  }

  return {
    type: 'error',
    title: 'OAuth login failed',
    detail: error?.trim() || 'Unknown OAuth error.',
  }
}
