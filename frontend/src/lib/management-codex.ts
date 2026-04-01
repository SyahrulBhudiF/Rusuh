import { useMutation } from '@tanstack/react-query'

import { managementRequest } from './management-api'
import { useManagementAuth } from './management-auth'

type CodexQuotaWindow = {
  used_percent?: number
  limit_window_seconds?: number
  reset_after_seconds?: number
  reset_at?: number
}

type CodexQuotaBucket = {
  allowed?: boolean
  limit_reached?: boolean
  primary_window?: CodexQuotaWindow | null
  secondary_window?: CodexQuotaWindow | null
}

type CodexCredits = {
  has_credits?: boolean
  unlimited?: boolean
  balance?: number | null
  approx_local_messages?: number | null
  approx_cloud_messages?: number | null
}

type CodexSpendControl = {
  reached?: boolean
}

export type CodexQuotaResponse = {
  account: string
  status: 'available' | 'exhausted' | 'error'
  detail?: string
  retry_after_seconds?: number
  upstream_status: number
  plan_type?: string
  account_id?: string
  email?: string
  rate_limit?: CodexQuotaBucket
  code_review_rate_limit?: CodexQuotaBucket
  additional_rate_limits?: unknown
  credits?: CodexCredits
  spend_control?: CodexSpendControl
  raw_response?: unknown
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
