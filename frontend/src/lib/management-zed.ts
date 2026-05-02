import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { managementRequest } from './management-api'
import { useManagementAuth } from './management-auth'
import { queryKeys } from './query'

export type StartZedLoginResponse = {
  login_url: string
  session_id: string
  port: number
  status: 'waiting'
}

export type ZedLoginStatusResponse = {
  status: 'waiting' | 'completed'
  session_id?: string
  filename?: string
  user_id?: string
}

export type ZedQuotaResponse = {
  account: string
  status: 'available' | 'error'
  plan?: string | null
  plan_v2?: string | null
  plan_v3?: string | null
  subscription_started_at?: string | null
  subscription_ended_at?: string | null
  model_requests_used?: number | null
  model_requests_limit?: number | string | null
  edit_predictions_used?: number | null
  edit_predictions_limit?: string | number | null
  is_account_too_young?: boolean | null
  has_overdue_invoices?: boolean | null
  is_usage_based_billing_enabled?: boolean | null
  feature_flags?: string[]
  error?: string | null
  upstream_status?: number
}

export type ZedModelsResponse = {
  account: string
  provider_key: 'zed'
  models: string[]
}

type StartZedLoginInput = {
  name?: string
}

type CheckZedQuotaInput = {
  name: string
}

type FetchZedModelsInput = {
  name: string
}

export function startZedLogin(secret: string, input: StartZedLoginInput = {}) {
  const name = input.name?.trim()

  return managementRequest<StartZedLoginResponse>('/v0/management/zed/login/initiate', secret, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify(name ? { name } : {}),
  })
}

export function fetchZedLoginStatus(secret: string, sessionId: string) {
  return managementRequest<ZedLoginStatusResponse>(
    `/v0/management/zed/login/status?session_id=${encodeURIComponent(sessionId)}`,
    secret,
  )
}

export function checkZedQuota(secret: string, input: CheckZedQuotaInput) {
  return managementRequest<ZedQuotaResponse>('/v0/management/zed/check-quota', secret, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({ name: input.name }),
  })
}

export function fetchZedModels(secret: string, input: FetchZedModelsInput) {
  return managementRequest<ZedModelsResponse>('/v0/management/zed/models', secret, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({ name: input.name }),
  })
}

export function useStartZedLoginMutation() {
  const { secret } = useManagementAuth()

  return useMutation({
    mutationFn: (input: StartZedLoginInput) => startZedLogin(secret, input),
  })
}

export function useZedLoginStatusQuery(sessionId: string | null, enabled = true) {
  const queryClient = useQueryClient()
  const { secret } = useManagementAuth()
  const query = useQuery<ZedLoginStatusResponse>({
    queryKey: ['management', 'zed-login-status', sessionId],
    queryFn: () => fetchZedLoginStatus(secret, sessionId ?? ''),
    enabled: enabled && Boolean(sessionId),
    refetchInterval: (query) => {
      const status = query.state.data?.status
      return status === 'waiting' || status === undefined ? 1500 : false
    },
  })

  if (query.data?.status === 'completed') {
    void queryClient.invalidateQueries({ queryKey: queryKeys.accounts })
    void queryClient.invalidateQueries({ queryKey: queryKeys.overview })
  }

  return query
}

export function useCheckZedQuotaMutation() {
  const { secret } = useManagementAuth()

  return useMutation({
    mutationFn: (input: CheckZedQuotaInput) => checkZedQuota(secret, input),
  })
}

export function useFetchZedModelsMutation() {
  const { secret } = useManagementAuth()

  return useMutation({
    mutationFn: (input: FetchZedModelsInput) => fetchZedModels(secret, input),
  })
}
