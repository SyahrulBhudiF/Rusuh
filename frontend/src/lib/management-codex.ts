import { useMutation } from '@tanstack/react-query'

import { managementRequest } from './management-api'
import { useManagementAuth } from './management-auth'

export type CodexQuotaResponse = {
  account: string
  status: 'available' | 'exhausted' | 'error'
  detail?: string
  retry_after_seconds?: number
  upstream_status: number
  plan_type?: string
}

type CheckCodexQuotaInput = {
  name: string
}

export function checkCodexQuota(secret: string, input: CheckCodexQuotaInput) {
  return managementRequest<CodexQuotaResponse>('/v0/management/codex/check-quota', secret, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({ name: input.name }),
  })
}

export function useCheckCodexQuotaMutation() {
  const { secret } = useManagementAuth()

  return useMutation({
    mutationFn: (input: CheckCodexQuotaInput) => checkCodexQuota(secret, input),
  })
}
