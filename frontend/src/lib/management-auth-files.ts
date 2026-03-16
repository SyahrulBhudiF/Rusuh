import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { managementRequest } from './management-api'
import { useManagementAuth } from './management-auth'
import { queryKeys } from './query'

type AuthFileStatusPayload = {
  status: string
  name: string
  disabled: boolean
}

type AuthFileFieldsPayload = {
  status: string
  name: string
}

type AuthFileDeletePayload = {
  status: string
  name: string
}

type AuthFileUploadPayload = {
  status: string
  name: string
}

type ManagementAuthFile = {
  id: string
  type: string
  provider_key: string
  label: string
  auth_method: string | null
  provider: string | null
  region: string | null
  start_url: string | null
  profile_arn: string | null
  email: string
  project_id: string
  status: string
  status_message: string | null
  disabled: boolean
  size: number
  updated_at: string
  last_refreshed_at: string | null
}

type ManagementAuthFilesPayload = {
  'auth-files': ManagementAuthFile[]
}

export function useManagementAuthFilesQuery() {
  const { secret } = useManagementAuth()

  return useQuery({
    queryKey: queryKeys.accounts,
    queryFn: () =>
      managementRequest<ManagementAuthFilesPayload>('/v0/management/auth-files', secret),
    enabled: secret.trim().length > 0,
  })
}

export type { ManagementAuthFile }
export function useToggleAuthFileStatusMutation() {
  const queryClient = useQueryClient()
  const { secret } = useManagementAuth()

  return useMutation({
    mutationFn: ({ name, disabled }: { name: string; disabled: boolean }) =>
      managementRequest<AuthFileStatusPayload>('/v0/management/auth-files/status', secret, {
        method: 'PATCH',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({ name, disabled }),
      }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.accounts })
      void queryClient.invalidateQueries({ queryKey: queryKeys.overview })
    },
  })
}

export function usePatchAuthFileFieldsMutation() {
  const queryClient = useQueryClient()
  const { secret } = useManagementAuth()

  return useMutation({
    mutationFn: ({ name, label }: { name: string; label: string }) =>
      managementRequest<AuthFileFieldsPayload>('/v0/management/auth-files/fields', secret, {
        method: 'PATCH',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({ name, label }),
      }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.accounts })
      void queryClient.invalidateQueries({ queryKey: queryKeys.overview })
    },
  })
}

export function useDeleteAuthFileMutation() {
  const queryClient = useQueryClient()
  const { secret } = useManagementAuth()

  return useMutation({
    mutationFn: (name: string) =>
      managementRequest<AuthFileDeletePayload>(
        `/v0/management/auth-files?name=${encodeURIComponent(name)}`,
        secret,
        {
          method: 'DELETE',
        },
      ),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.accounts })
      void queryClient.invalidateQueries({ queryKey: queryKeys.overview })
    },
  })
}

export function useUploadAuthFileMutation() {
  const queryClient = useQueryClient()
  const { secret } = useManagementAuth()

  return useMutation({
    mutationFn: ({ name, body }: { name: string; body: string }) =>
      managementRequest<AuthFileUploadPayload>(
        `/v0/management/auth-files?name=${encodeURIComponent(name)}`,
        secret,
        {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
          },
          body,
        },
      ),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.accounts })
      void queryClient.invalidateQueries({ queryKey: queryKeys.overview })
    },
  })
}

export function downloadAuthFile(name: string) {
  const url = `/v0/management/auth-files/download?name=${encodeURIComponent(name)}`
  window.open(url, '_blank', 'noopener,noreferrer')
}
