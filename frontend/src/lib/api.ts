export type DashboardHealth = {
  status: string
  service: string
}

export type DashboardOverviewCard = {
  label: string
  value: string
  hint: string
}

export type DashboardAccountSummary = {
  provider: string
  total: number
  active: number
  refreshing: number
  pending: number
  error: number
  disabled: number
  unknown: number
}

export type DashboardOverview = {
  health: DashboardHealth
  cards: DashboardOverviewCard[]
  account_summaries: DashboardAccountSummary[]
  available_model_count: number
  provider_names: string[]
  routing_strategy: string
}

export type DashboardAuthRecord = {
  id: string
  provider: string
  label: string
  status: string
  disabled: boolean
  status_message: string | null
  last_refreshed_at: string | null
  updated_at: string
  email: string | null
  project_id: string | null
  path: string
}

export type DashboardAccountsPayload = {
  items: DashboardAuthRecord[]
  grouped_counts: DashboardAccountSummary[]
  total: number
}

export type DashboardApiKeysPayload = {
  items: string[]
  total: number
  generated_only: boolean
}

export type DashboardManagementConfig = {
  enabled: boolean
  allow_remote: boolean
}

export type DashboardProviderKeyEntry = {
  prefix: string | null
  base_url: string | null
  model_count: number
  excluded_model_count: number
  has_proxy_url: boolean
  header_count: number
  has_api_key: boolean
}

export type DashboardOpenAiCompatProvider = {
  name: string
  prefix: string | null
  base_url: string
  model_count: number
  api_key_entry_count: number
  header_count: number
}

export type DashboardConfigPayload = {
  host: string
  port: number
  listen_addr: string
  auth_dir: string
  debug: boolean
  request_retry: number
  routing_strategy: string
  api_key_count: number
  oauth_alias_channel_count: number
  oauth_alias_count: number
  provider_count: number
  provider_names: string[]
  management: DashboardManagementConfig
  gemini_api_keys: DashboardProviderKeyEntry[]
  codex_api_keys: DashboardProviderKeyEntry[]
  claude_api_keys: DashboardProviderKeyEntry[]
  openai_compat: DashboardOpenAiCompatProvider[]
}

async function dashboard<T>(path: string): Promise<T> {
  const response = await fetch(path, {
    headers: {
      Accept: 'application/json',
    },
  })

  if (!response.ok) {
    throw new Error(`Dashboard request failed: ${response.status} ${response.statusText}`)
  }

  return (await response.json()) as T
}

export const api = {
  health: () => dashboard<DashboardHealth>('/dashboard/health'),
  overview: () => dashboard<DashboardOverview>('/dashboard/overview'),
  accounts: () => dashboard<DashboardAccountsPayload>('/dashboard/accounts'),
  apiKeys: () => dashboard<DashboardApiKeysPayload>('/dashboard/api-keys'),
  config: () => dashboard<DashboardConfigPayload>('/dashboard/config'),
}
