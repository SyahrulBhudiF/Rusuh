import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { managementRequest } from './management-api'
import { useManagementAuth } from './management-auth'
import { queryKeys } from './query'

export type OAuthProvider =
  | 'antigravity'
  | 'kiro-google'
  | 'kiro-github'
  | 'codex'
  | 'github-copilot'

export type StartOAuthResponse = {
  status: string
  url?: string
  state: string
  provider: string
  device_code?: string
  user_code?: string
  verification_uri?: string
  expires_in?: number
  interval?: number
}

export type OAuthStatusResponse = {
  status: 'wait' | 'ok' | 'error'
  provider?: string
  error?: string
}

export type SubmitOAuthCallbackResponse = {
  status: string
  error?: string
}

export function useStartOAuthMutation() {
  const { secret } = useManagementAuth()

  return useMutation({
    mutationFn: ({ provider, label }: { provider: OAuthProvider; label?: string }) => {
      const params = new URLSearchParams({ provider })
      if (label?.trim()) {
        params.set('label', label.trim())
      }

      return managementRequest<StartOAuthResponse>(
        `/v0/management/oauth/start?${params.toString()}`,
        secret,
      )
    },
  })
}

export function useSubmitOAuthCallbackMutation() {
  const { secret } = useManagementAuth()

  return useMutation({
    mutationFn: ({ provider, redirectUrl }: { provider: OAuthProvider; redirectUrl: string }) =>
      managementRequest<SubmitOAuthCallbackResponse>('/v0/management/oauth-callback', secret, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          provider,
          redirect_url: redirectUrl.trim(),
        }),
      }),
  })
}

export function useOAuthStatusQuery(state: string | null, enabled = true) {
  const queryClient = useQueryClient()
  const { secret } = useManagementAuth()
  const query = useQuery<OAuthStatusResponse>({
    queryKey: ['management', 'oauth-status', state],
    queryFn: () =>
      managementRequest<OAuthStatusResponse>(
        `/v0/management/oauth/status?state=${encodeURIComponent(state ?? '')}`,
        secret,
      ),
    enabled: enabled && Boolean(state),
    refetchInterval: (query) => {
      const status = query.state.data?.status
      return status === 'wait' || status === undefined ? 1500 : false
    },
  })

  if (query.data?.status === 'ok') {
    void queryClient.invalidateQueries({ queryKey: queryKeys.accounts })
    void queryClient.invalidateQueries({ queryKey: queryKeys.overview })
  }

  return query
}
