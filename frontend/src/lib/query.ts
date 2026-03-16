import { useQuery } from '@tanstack/react-query'

import { api } from './api'

export const queryKeys = {
  overview: ['dashboard', 'overview'] as const,
  accounts: ['dashboard', 'accounts'] as const,
  apiKeys: ['dashboard', 'api-keys'] as const,
  config: ['dashboard', 'config'] as const,
}

export function useOverviewQuery() {
  return useQuery({
    queryKey: queryKeys.overview,
    queryFn: api.overview,
  })
}

export function useAccountsQuery() {
  return useQuery({
    queryKey: queryKeys.accounts,
    queryFn: api.accounts,
  })
}

export function useApiKeysQuery() {
  return useQuery({
    queryKey: queryKeys.apiKeys,
    queryFn: api.apiKeys,
  })
}

export function useConfigQuery() {
  return useQuery({
    queryKey: queryKeys.config,
    queryFn: api.config,
  })
}
