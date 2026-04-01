export type AddAccountOauthProvider =
  | 'kiro'
  | 'antigravity'
  | 'codex'
  | 'github-copilot'

export type TrackedOauthStates = Partial<Record<AddAccountOauthProvider, string>>

export function formatOauthExpiryHint(expiresInSeconds: number | undefined): string | null {
  if (!Number.isFinite(expiresInSeconds) || expiresInSeconds === undefined || expiresInSeconds <= 0) {
    return null
  }

  if (expiresInSeconds < 60) {
    return `Code expires in about ${Math.round(expiresInSeconds)} seconds.`
  }

  const minutes = Math.round(expiresInSeconds / 60)
  return `Code expires in about ${minutes} minute${minutes === 1 ? '' : 's'}.`
}

export function parseOauthStateFromRedirectUrl(redirectUrl: string): string | null {
  try {
    const url = new URL(redirectUrl)
    return url.searchParams.get('state')
  } catch {
    return null
  }
}

export function resolveTrackedOauthSession(
  provider: AddAccountOauthProvider,
  redirectUrl: string,
  trackedStates: TrackedOauthStates,
): { provider: AddAccountOauthProvider; state: string } | null {
  const callbackState = parseOauthStateFromRedirectUrl(redirectUrl)

  if (callbackState) {
    const matchingEntry = (Object.entries(trackedStates) as Array<[AddAccountOauthProvider, string]>)
      .find(([, trackedState]) => trackedState === callbackState)

    if (matchingEntry) {
      const [matchedProvider, state] = matchingEntry
      return { provider: matchedProvider, state }
    }
  }

  const state = trackedStates[provider]
  if (!state) {
    return null
  }

  return { provider, state }
}
