import { useMutation } from '@tanstack/react-query'

import { managementRequest } from './management-api'
import { useManagementAuth } from './management-auth'

export type CopilotModelsResponse = {
  account: string
  provider_key: 'github-copilot'
  models: string[]
}

type FetchCopilotModelsInput = {
  name: string
}

export function fetchCopilotModels(secret: string, input: FetchCopilotModelsInput) {
  return managementRequest<CopilotModelsResponse>('/v0/management/github-copilot/models', secret, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({ name: input.name }),
  })
}

export function useFetchCopilotModelsMutation() {
  const { secret } = useManagementAuth()

  return useMutation({
    mutationFn: (input: FetchCopilotModelsInput) => fetchCopilotModels(secret, input),
  })
}
