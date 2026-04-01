export type AddAccountOauthProvider = 'kiro' | 'antigravity' | 'codex'

export type TrackedOauthStates = Partial<Record<AddAccountOauthProvider, string>>

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
