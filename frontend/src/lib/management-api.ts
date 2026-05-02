import { useQuery } from '@tanstack/react-query'

import { useManagementAuth } from './management-auth'
import { ManagementAuthError } from './management-error'
export type ManagementStatus = {
  status: string
  port: number
  providers: number
}
export type ManagementConfig = {
  port: number
  host: string
  debug: boolean
}
type QueryOptions = {
  enabled?: boolean
}
async function request<T>(path: string, secret: string): Promise<T> {
  const response = await fetch(path, {
    headers: {
      Accept: 'application/json',
      'X-Management-Key': secret,
    },
  })

  if (response.status === 401 || response.status === 403) {
    throw new ManagementAuthError()
  }

  if (!response.ok) {
    throw new Error(`Management request failed: ${response.status} ${response.statusText}`)
  }

  return (await response.json()) as T
}

export async function managementRequest<T>(
  path: string,
  secret: string,
  init?: RequestInit,
): Promise<T> {
  const response = await fetch(path, {
    ...init,
    headers: {
      Accept: 'application/json',
      'X-Management-Key': secret,
      ...init?.headers,
    },
  })

  if (response.status === 401 || response.status === 403) {
    throw new ManagementAuthError()
  }

  if (!response.ok) {
    throw new Error(`Management request failed: ${response.status} ${response.statusText}`)
  }

  return (await response.json()) as T
}
export function useManagementStatusQuery(options?: QueryOptions) {
  const { secret } = useManagementAuth()

  return useQuery({
    queryKey: ['management', 'status'],
    queryFn: () => request<ManagementStatus>('/v0/management/status', secret),
    enabled: (options?.enabled ?? true) && secret.trim().length > 0,
    retry: false,
    throwOnError: false,
  })
}
export function useManagementConfigQuery(options?: QueryOptions) {
  const { secret } = useManagementAuth()

  return useQuery({
    queryKey: ['management', 'config'],
    queryFn: () => request<ManagementConfig>('/v0/management/config', secret),
    enabled: (options?.enabled ?? true) && secret.trim().length > 0,
    retry: false,
    throwOnError: false,
  })
}
