import { useMutation, useQueryClient } from '@tanstack/react-query'

import { managementRequest } from './management-api'
import { useManagementAuth } from './management-auth'
import { queryKeys } from './query'

type StartKiroBuilderIdPayload = {
  session_id: string
  auth_url: string
  expires_at: string
  auth_method: 'builder-id'
  provider_key: 'kiro'
}

type ImportKiroPayload = {
  status: string
  name: string
  provider_key: 'kiro'
  label: string
  auth_method: string
  provider: string
}

type ImportKiroInput = {
  access_token: string
  refresh_token: string
  expires_at: string
  client_id: string
  client_secret: string
  profile_arn?: string
  auth_method?: string
  provider?: string
  region?: string
  start_url?: string
  email?: string
  label?: string
}

type ImportKiroSocialInput = {
  refresh_token: string
  label?: string
}

export function useStartKiroBuilderIdMutation() {
  const { secret } = useManagementAuth()

  return useMutation({
    mutationFn: ({ label }: { label?: string }) =>
      managementRequest<StartKiroBuilderIdPayload>('/v0/management/kiro/builder-id/start', secret, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({ label }),
      }),
  })
}

export function useImportKiroMutation() {
  const queryClient = useQueryClient()
  const { secret } = useManagementAuth()

  return useMutation({
    mutationFn: (input: ImportKiroInput) =>
      managementRequest<ImportKiroPayload>('/v0/management/kiro/import', secret, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify(input),
      }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.accounts })
      void queryClient.invalidateQueries({ queryKey: queryKeys.overview })
    },
  })
}

export function useImportKiroSocialMutation() {
  const queryClient = useQueryClient()
  const { secret } = useManagementAuth()

  return useMutation({
    mutationFn: (input: ImportKiroSocialInput) =>
      managementRequest<ImportKiroPayload>('/v0/management/kiro/social/import', secret, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify(input),
      }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.accounts })
      void queryClient.invalidateQueries({ queryKey: queryKeys.overview })
    },
  })
}

export function useCheckKiroQuotaMutation() {
  const { secret } = useManagementAuth()

  return useMutation({
    mutationFn: ({ name }: { name: string }) =>
      managementRequest<{
        status: 'unknown' | 'available' | 'exhausted'
        remaining?: number
        detail?: string
        message?: string
      }>('/v0/management/kiro/check-quota', secret, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({ name }),
      }),
  })
}
