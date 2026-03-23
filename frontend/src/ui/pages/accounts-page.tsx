import { useMemo, useState } from 'react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Textarea } from '@/components/ui/textarea'

import {
  downloadAuthFile,
  type ManagementAuthFile,
  useDeleteAuthFileMutation,
  useManagementAuthFilesQuery,
  usePatchAuthFileFieldsMutation,
  useToggleAuthFileStatusMutation,
  useUploadAuthFileMutation,
} from '../../lib/management-auth-files'
import {
  useImportKiroMutation,
  useImportKiroSocialMutation,
  useStartKiroBuilderIdMutation,
  useCheckKiroQuotaMutation,
} from '../../lib/management-kiro'
import { useOAuthStatusQuery, useStartOAuthMutation } from '../../lib/management-oauth'
import {
  type ZedModelsResponse,
  type ZedQuotaResponse,
  useCheckZedQuotaMutation,
  useFetchZedModelsMutation,
  useStartZedLoginMutation,
  useZedLoginStatusQuery,
} from '../../lib/management-zed'
import { PageShell } from '../page-shell'
import { QueryState } from '../query-state'
import { statusTone } from '../status-tone'

const ALL_FILTER = 'all'
const STATUS_OPTIONS = ['active', 'refreshing', 'pending', 'error', 'disabled', 'unknown'] as const
const MAX_LABEL_LENGTH = 200
const MAX_UPLOAD_NAME_LENGTH = 200
const MAX_UPLOAD_BODY_LENGTH = 20000
const MAX_UPLOAD_FILE_SIZE = 1024 * 1024

type ProviderGroup = {
  key: string
  label: string
  items: ManagementAuthFile[]
}

function providerLabel(key: string) {
  if (key === 'kiro') return 'Kiro'
  if (key === 'antigravity') return 'Antigravity'
  if (key === 'zed') return 'Zed'
  return key
}

function accountSubtitle(item: ManagementAuthFile) {
  return item.email || item.project_id || item.provider || item.type || '—'
}

function renderKiroMetadata(item: ManagementAuthFile) {
  return (
    <div className='mt-3 flex flex-wrap gap-2 text-xs'>
      {item.auth_method ? (
        <Badge variant='outline' className='rounded-full px-2.5 py-1'>
          {item.auth_method}
        </Badge>
      ) : null}
      {item.region ? (
        <Badge variant='outline' className='rounded-full px-2.5 py-1'>
          {item.region}
        </Badge>
      ) : null}
      {item.email ? (
        <Badge variant='outline' className='rounded-full px-2.5 py-1'>
          {item.email}
        </Badge>
      ) : null}
      {item.start_url ? (
        <Badge variant='outline' className='max-w-full rounded-full px-2.5 py-1'>
          <span className='truncate'>{item.start_url}</span>
        </Badge>
      ) : null}
    </div>
  )
}

export function AccountsPage() {
  const accounts = useManagementAuthFilesQuery()
  const toggleStatus = useToggleAuthFileStatusMutation()
  const patchFields = usePatchAuthFileFieldsMutation()
  const deleteAuthFile = useDeleteAuthFileMutation()
  const uploadAuthFile = useUploadAuthFileMutation()
  const startOAuth = useStartOAuthMutation()
  const startKiroBuilderId = useStartKiroBuilderIdMutation()
  const startZedLogin = useStartZedLoginMutation()
  const importKiro = useImportKiroMutation()
  const importKiroSocial = useImportKiroSocialMutation()
  const checkKiroQuota = useCheckKiroQuotaMutation()
  const checkZedQuota = useCheckZedQuotaMutation()
  const fetchZedModels = useFetchZedModelsMutation()

  const [providerFilter, setProviderFilter] = useState(ALL_FILTER)
  const [statusFilter, setStatusFilter] = useState(ALL_FILTER)
  const [editName, setEditName] = useState<string | null>(null)
  const [editLabel, setEditLabel] = useState('')
  const [quotaResults, setQuotaResults] = useState<
    Record<string, { status: string; remaining?: number; detail?: string; message?: string }>
  >({})
  const [zedQuotaResults, setZedQuotaResults] = useState<Record<string, ZedQuotaResponse>>({})
  const [zedModelsResults, setZedModelsResults] = useState<Record<string, ZedModelsResponse>>({})

  const [antigravityLabel, setAntigravityLabel] = useState('')
  const [oauthState, setOauthState] = useState<string | null>(null)
  const [zedLabel, setZedLabel] = useState('')
  const [zedSessionId, setZedSessionId] = useState<string | null>(null)
  const [onboardingProvider, setOnboardingProvider] = useState<'kiro' | 'antigravity' | 'zed'>(
    'kiro',
  )

  const [kiroLabel, setKiroLabel] = useState('')
  const [kiroImportMode, setKiroImportMode] = useState<'structured' | 'json'>('structured')
  const [kiroImportJson, setKiroImportJson] = useState('')
  const [kiroAccessToken, setKiroAccessToken] = useState('')
  const [kiroRefreshToken, setKiroRefreshToken] = useState('')
  const [kiroExpiresAt, setKiroExpiresAt] = useState('')
  const [kiroClientId, setKiroClientId] = useState('')
  const [kiroClientSecret, setKiroClientSecret] = useState('')
  const [kiroProfileArn, setKiroProfileArn] = useState('')
  const [kiroProvider, setKiroProvider] = useState('AWS')
  const [kiroRegion, setKiroRegion] = useState('us-east-1')
  const [kiroStartUrl, setKiroStartUrl] = useState('https://view.awsapps.com/start')
  const [kiroEmail, setKiroEmail] = useState('')
  const [kiroSocialRefreshToken, setKiroSocialRefreshToken] = useState('')

  const [uploadName, setUploadName] = useState('')
  const [uploadBody, setUploadBody] = useState('')
  const [uploadFileError, setUploadFileError] = useState<string | null>(null)

  const oauthStatus = useOAuthStatusQuery(oauthState, Boolean(oauthState))
  const oauthStatusData = oauthStatus.data
  const oauthStatusSummary = oauthState
    ? (oauthStatusData?.status ?? (oauthStatus.isFetching ? 'wait' : 'idle'))
    : 'idle'
  const zedLoginStatus = useZedLoginStatusQuery(zedSessionId, Boolean(zedSessionId))
  const zedLoginStatusData = zedLoginStatus.data
  const zedLoginStatusSummary = zedSessionId
    ? (zedLoginStatusData?.status ?? (zedLoginStatus.isFetching ? 'waiting' : 'idle'))
    : 'idle'

  const mutationError = [
    toggleStatus,
    patchFields,
    deleteAuthFile,
    uploadAuthFile,
    startOAuth,
    startKiroBuilderId,
    startZedLogin,
    importKiro,
    importKiroSocial,
    checkZedQuota,
    fetchZedModels,
  ].find((mutation) => mutation.isError)?.error as Error | undefined

  const items = useMemo(() => {
    const source = accounts.data?.['auth-files'] ?? []

    return [...source]
      .filter((item) => providerFilter === ALL_FILTER || item.provider_key === providerFilter)
      .filter((item) => statusFilter === ALL_FILTER || item.status === statusFilter)
      .sort((a, b) => Date.parse(b.updated_at) - Date.parse(a.updated_at))
  }, [accounts.data, providerFilter, statusFilter])

  const providerGroups = useMemo<ProviderGroup[]>(() => {
    const map = new Map<string, ManagementAuthFile[]>()

    for (const item of items) {
      const existing = map.get(item.provider_key) ?? []
      existing.push(item)
      map.set(item.provider_key, existing)
    }

    return [...map.entries()].map(([key, groupedItems]) => ({
      key,
      label: providerLabel(key),
      items: groupedItems,
    }))
  }, [items])

  const providerOptions = useMemo(() => {
    const source = accounts.data?.['auth-files'] ?? []
    return [...new Set(source.map((item) => item.provider_key))]
  }, [accounts.data])

  const hasItems = items.length > 0

  function submitKiroStructuredImport() {
    importKiro.mutate({
      access_token: kiroAccessToken.trim(),
      refresh_token: kiroRefreshToken.trim(),
      expires_at: kiroExpiresAt.trim(),
      client_id: kiroClientId.trim(),
      client_secret: kiroClientSecret.trim(),
      profile_arn: kiroProfileArn.trim(),
      auth_method: 'import',
      provider: kiroProvider.trim() || 'AWS',
      region: kiroRegion.trim() || 'us-east-1',
      start_url: kiroStartUrl.trim(),
      email: kiroEmail.trim(),
      label: kiroLabel.trim(),
    })
  }

  function submitKiroJsonImport() {
    const parsed = JSON.parse(kiroImportJson) as Record<string, unknown>
    importKiro.mutate({
      access_token: String(parsed.access_token ?? ''),
      refresh_token: String(parsed.refresh_token ?? ''),
      expires_at: String(parsed.expires_at ?? ''),
      client_id: String(parsed.client_id ?? ''),
      client_secret: String(parsed.client_secret ?? ''),
      profile_arn: String(parsed.profile_arn ?? ''),
      auth_method: typeof parsed.auth_method === 'string' ? parsed.auth_method : 'import',
      provider: typeof parsed.provider === 'string' ? parsed.provider : 'AWS',
      region: typeof parsed.region === 'string' ? parsed.region : 'us-east-1',
      start_url: typeof parsed.start_url === 'string' ? parsed.start_url : '',
      email: typeof parsed.email === 'string' ? parsed.email : '',
      label: kiroLabel.trim(),
    })
  }

  return (
    <PageShell
      eyebrow='Accounts'
      title='Provider onboarding and account management'
      description='Separate add-account flows from existing account management, with Kiro grouped explicitly by provider.'
    >
      <QueryState
        isLoading={accounts.isLoading}
        isError={accounts.isError}
        error={accounts.error as Error | null}
      >
        {accounts.data ? (
          <div className='space-y-6'>
            <section className='space-y-4'>
              <div>
                <h3 className='text-lg font-semibold'>Login / Add account</h3>
                <p className='text-muted-foreground mt-1 text-sm'>
                  Start provider-specific onboarding here. Generic auth-file upload stays as an
                  advanced fallback below.
                </p>
              </div>

              <Tabs
                value={onboardingProvider}
                onValueChange={(value) =>
                  setOnboardingProvider(value as 'kiro' | 'antigravity' | 'zed')
                }
              >
                <TabsList className='grid w-full grid-cols-3 rounded-3xl p-2'>
                  <TabsTrigger value='kiro' className='rounded-2xl'>
                    Kiro
                  </TabsTrigger>
                  <TabsTrigger value='antigravity' className='rounded-2xl'>
                    Antigravity
                  </TabsTrigger>
                  <TabsTrigger value='zed' className='rounded-2xl'>
                    Zed
                  </TabsTrigger>
                </TabsList>

                <TabsContent value='kiro'>
                  <Card className='border-border rounded-3xl border'>
                    <CardContent className='space-y-4 p-5'>
                      <div>
                        <div className='flex items-center gap-2'>
                          <h4 className='text-base font-semibold'>Kiro</h4>
                          <Badge variant='outline' className='rounded-full px-2.5 py-1 text-xs'>
                            provider-specific
                          </Badge>
                        </div>
                        <p className='text-muted-foreground mt-1 text-sm'>
                          Choose the Kiro account path you want to add. Builder ID is live, import
                          flows are ready, and Identity Center stays deferred for this pass.
                        </p>
                      </div>

                      <div className='space-y-3 rounded-2xl border p-4'>
                        <div className='flex items-center justify-between gap-3'>
                          <div>
                            <p className='font-medium'>Builder ID</p>
                            <p className='text-muted-foreground text-sm'>
                              Launch the Kiro Builder ID web flow.
                            </p>
                          </div>
                          <Button
                            type='button'
                            onClick={() =>
                              startKiroBuilderId.mutate(
                                { label: kiroLabel.trim() || undefined },
                                {
                                  onSuccess: (data) => {
                                    setOauthState(data.session_id)
                                    window.open(data.auth_url, '_blank', 'noopener,noreferrer')
                                  },
                                },
                              )
                            }
                            disabled={startKiroBuilderId.isPending}
                            className='h-10 rounded-xl px-4'
                          >
                            {startKiroBuilderId.isPending ? 'Launching…' : 'Launch'}
                          </Button>
                        </div>
                      </div>

                      <div className='space-y-3 rounded-2xl border p-4'>
                        <div className='flex items-center justify-between gap-3'>
                          <div>
                            <p className='font-medium'>Identity Center</p>
                            <p className='text-muted-foreground text-sm'>
                              Deferred for now. Backend flow not wired yet.
                            </p>
                          </div>
                          <Badge variant='outline' className='rounded-full px-2.5 py-1 text-xs'>
                            coming soon
                          </Badge>
                        </div>
                      </div>

                      <div className='space-y-4 rounded-2xl border p-4'>
                        <div className='flex items-center justify-between gap-3'>
                          <div>
                            <p className='font-medium'>Import Kiro auth</p>
                            <p className='text-muted-foreground text-sm'>
                              Paste canonical Kiro fields or a full JSON token document.
                            </p>
                          </div>
                          <Select
                            value={kiroImportMode}
                            onValueChange={(value) =>
                              setKiroImportMode(value as 'structured' | 'json')
                            }
                          >
                            <SelectTrigger className='h-10 w-[180px] rounded-xl'>
                              <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                              <SelectItem value='structured'>Structured</SelectItem>
                              <SelectItem value='json'>Paste JSON</SelectItem>
                            </SelectContent>
                          </Select>
                        </div>

                        <Input
                          type='text'
                          value={kiroLabel}
                          onChange={(event) => setKiroLabel(event.target.value)}
                          placeholder='Optional label'
                          maxLength={MAX_LABEL_LENGTH}
                          className='h-11 rounded-2xl'
                        />
                        {kiroImportMode === 'structured' ? (
                          <div className='grid gap-3 lg:grid-cols-2'>
                            <Input
                              value={kiroAccessToken}
                              onChange={(event) => setKiroAccessToken(event.target.value)}
                              placeholder='access_token'
                              className='h-11 rounded-2xl'
                            />
                            <Input
                              value={kiroRefreshToken}
                              onChange={(event) => setKiroRefreshToken(event.target.value)}
                              placeholder='refresh_token'
                              className='h-11 rounded-2xl'
                            />
                            <Input
                              value={kiroExpiresAt}
                              onChange={(event) => setKiroExpiresAt(event.target.value)}
                              placeholder='expires_at (RFC3339)'
                              className='h-11 rounded-2xl'
                            />
                            <Input
                              value={kiroClientId}
                              onChange={(event) => setKiroClientId(event.target.value)}
                              placeholder='client_id'
                              className='h-11 rounded-2xl'
                            />
                            <Input
                              value={kiroClientSecret}
                              onChange={(event) => setKiroClientSecret(event.target.value)}
                              placeholder='client_secret'
                              className='h-11 rounded-2xl'
                            />
                            <Input
                              value={kiroProfileArn}
                              onChange={(event) => setKiroProfileArn(event.target.value)}
                              placeholder='profile_arn (optional)'
                              className='h-11 rounded-2xl'
                            />
                            <Input
                              value={kiroProvider}
                              onChange={(event) => setKiroProvider(event.target.value)}
                              placeholder='provider'
                              className='h-11 rounded-2xl'
                            />
                            <Input
                              value={kiroRegion}
                              onChange={(event) => setKiroRegion(event.target.value)}
                              placeholder='region'
                              className='h-11 rounded-2xl'
                            />
                            <Input
                              value={kiroStartUrl}
                              onChange={(event) => setKiroStartUrl(event.target.value)}
                              placeholder='start_url'
                              className='h-11 rounded-2xl lg:col-span-2'
                            />
                            <Input
                              value={kiroEmail}
                              onChange={(event) => setKiroEmail(event.target.value)}
                              placeholder='email (optional)'
                              className='h-11 rounded-2xl lg:col-span-2'
                            />
                          </div>
                        ) : (
                          <Textarea
                            value={kiroImportJson}
                            onChange={(event) => setKiroImportJson(event.target.value)}
                            placeholder='{"access_token":"...","refresh_token":"...","expires_at":"2026-04-01T00:00:00Z","client_id":"...","client_secret":"..."}'
                            className='min-h-40 rounded-2xl px-4 py-3'
                          />
                        )}

                        <div className='flex justify-end'>
                          <Button
                            type='button'
                            onClick={() => {
                              if (kiroImportMode === 'json') {
                                submitKiroJsonImport()
                                return
                              }
                              submitKiroStructuredImport()
                            }}
                            disabled={importKiro.isPending}
                            className='h-11 rounded-xl px-4'
                          >
                            {importKiro.isPending ? 'Importing…' : 'Import Kiro auth'}
                          </Button>
                        </div>
                      </div>

                      <div className='space-y-4 rounded-2xl border p-4'>
                        <div>
                          <p className='font-medium'>Import social refresh token</p>
                          <p className='text-muted-foreground text-sm'>
                            Legacy Kiro social compatibility path. Paste a refresh token starting
                            with `aorAAAAAG...`.
                          </p>
                        </div>

                        <Textarea
                          value={kiroSocialRefreshToken}
                          onChange={(event) => setKiroSocialRefreshToken(event.target.value)}
                          placeholder='aorAAAAAG...'
                          className='min-h-24 rounded-2xl px-4 py-3'
                        />

                        <div className='flex justify-end'>
                          <Button
                            type='button'
                            onClick={() =>
                              importKiroSocial.mutate({
                                refresh_token: kiroSocialRefreshToken.trim(),
                                label: kiroLabel.trim() || undefined,
                              })
                            }
                            disabled={importKiroSocial.isPending}
                            className='h-11 rounded-xl px-4'
                          >
                            {importKiroSocial.isPending ? 'Importing…' : 'Import social token'}
                          </Button>
                        </div>
                      </div>
                      <div className='text-muted-foreground text-sm leading-6'>
                        {oauthState ? (
                          <>
                            <p>
                              Current session:{' '}
                              <span className='text-foreground break-all'>{oauthState}</span>
                            </p>
                            <p className='mt-1'>
                              Status: <span className='text-foreground'>{oauthStatusSummary}</span>
                            </p>
                            {oauthStatusData?.error ? (
                              <p className='border-destructive/30 bg-destructive/10 text-destructive dark:text-destructive-foreground mt-3 rounded-2xl border px-4 py-3'>
                                {oauthStatusData.error}
                              </p>
                            ) : null}
                          </>
                        ) : (
                          <p>No Kiro Builder ID session started yet.</p>
                        )}
                      </div>
                    </CardContent>
                  </Card>
                </TabsContent>

                <TabsContent value='antigravity'>
                  <Card className='border-border rounded-3xl border'>
                    <CardContent className='space-y-4 p-5'>
                      <div>
                        <div className='flex items-center gap-2'>
                          <h4 className='text-base font-semibold'>Antigravity</h4>
                          <Badge variant='outline' className='rounded-full px-2.5 py-1 text-xs'>
                            OAuth
                          </Badge>
                        </div>
                        <p className='text-muted-foreground mt-1 text-sm'>
                          Use the existing generic OAuth launcher for Antigravity.
                        </p>
                      </div>

                      <Input
                        type='text'
                        value={antigravityLabel}
                        onChange={(event) => setAntigravityLabel(event.target.value)}
                        className='h-11 rounded-2xl'
                        placeholder='Optional label'
                        maxLength={MAX_LABEL_LENGTH}
                      />

                      <div className='flex justify-end'>
                        <Button
                          type='button'
                          onClick={() =>
                            startOAuth.mutate(
                              {
                                provider: 'antigravity',
                                label: antigravityLabel.trim() || undefined,
                              },
                              {
                                onSuccess: (data) => {
                                  setOauthState(data.state)
                                  window.open(data.url, '_blank', 'noopener,noreferrer')
                                },
                              },
                            )
                          }
                          disabled={startOAuth.isPending}
                          className='h-11 rounded-xl px-4'
                        >
                          {startOAuth.isPending ? 'Launching…' : 'Launch OAuth'}
                        </Button>
                      </div>
                    </CardContent>
                  </Card>
                </TabsContent>

                <TabsContent value='zed'>
                  <Card className='border-border rounded-3xl border'>
                    <CardContent className='space-y-4 p-5'>
                      <div>
                        <div className='flex items-center gap-2'>
                          <h4 className='text-base font-semibold'>Zed</h4>
                          <Badge variant='outline' className='rounded-full px-2.5 py-1 text-xs'>
                            native login
                          </Badge>
                        </div>
                        <p className='text-muted-foreground mt-1 text-sm'>
                          Launch the Zed native-app sign-in flow, then wait for the localhost
                          callback to finish.
                        </p>
                      </div>

                      <Input
                        type='text'
                        value={zedLabel}
                        onChange={(event) => setZedLabel(event.target.value)}
                        className='h-11 rounded-2xl'
                        placeholder='Optional label'
                        maxLength={MAX_LABEL_LENGTH}
                      />

                      <div className='flex justify-end'>
                        <Button
                          type='button'
                          onClick={() =>
                            startZedLogin.mutate(
                              {
                                name: zedLabel.trim() || undefined,
                              },
                              {
                                onSuccess: (data) => {
                                  setZedSessionId(data.session_id)
                                  window.open(data.login_url, '_blank', 'noopener,noreferrer')
                                },
                              },
                            )
                          }
                          disabled={startZedLogin.isPending}
                          className='h-11 rounded-xl px-4'
                        >
                          {startZedLogin.isPending ? 'Launching…' : 'Launch Zed login'}
                        </Button>
                      </div>

                      <div className='text-muted-foreground text-sm leading-6'>
                        {zedSessionId ? (
                          <>
                            <p>
                              Current session:{' '}
                              <span className='text-foreground break-all'>{zedSessionId}</span>
                            </p>
                            <p className='mt-1'>
                              Status:{' '}
                              <span className='text-foreground'>{zedLoginStatusSummary}</span>
                            </p>
                            {zedLoginStatusData?.filename ? (
                              <p className='mt-1'>
                                Saved file:{' '}
                                <span className='text-foreground break-all'>
                                  {zedLoginStatusData.filename}
                                </span>
                              </p>
                            ) : null}
                            {zedLoginStatusData?.user_id ? (
                              <p className='mt-1'>
                                User ID:{' '}
                                <span className='text-foreground break-all'>
                                  {zedLoginStatusData.user_id}
                                </span>
                              </p>
                            ) : null}
                          </>
                        ) : (
                          <p>No Zed login session started yet.</p>
                        )}
                      </div>
                    </CardContent>
                  </Card>
                </TabsContent>
              </Tabs>

              {mutationError ? (
                <p className='border-destructive/30 bg-destructive/10 text-destructive dark:text-destructive-foreground rounded-2xl border px-4 py-3 text-sm'>
                  {mutationError.message}
                </p>
              ) : null}
            </section>

            <section className='space-y-4'>
              <div className='flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between'>
                <div>
                  <h3 className='text-lg font-semibold'>Manage existing accounts</h3>
                  <p className='text-muted-foreground mt-1 text-sm'>
                    Provider-grouped inventory with Kiro-specific metadata and generic actions.
                  </p>
                  <div className='mt-3 flex flex-wrap gap-2'>
                    {providerGroups.map((group) => (
                      <Badge
                        key={group.key}
                        variant='outline'
                        className='rounded-full px-3 py-1 text-sm'
                      >
                        {group.label} · {group.items.length}
                      </Badge>
                    ))}
                  </div>
                </div>
                <Badge variant='outline' className='w-fit rounded-full px-3 py-1 text-xs'>
                  {hasItems ? `${items.length} visible` : 'No matches'}
                </Badge>
              </div>

              <div className='grid gap-4 xl:grid-cols-[0.8fr_0.8fr_1.1fr]'>
                <label className='space-y-2'>
                  <span className='text-muted-foreground text-sm'>Provider</span>
                  <Select value={providerFilter} onValueChange={setProviderFilter}>
                    <SelectTrigger className='h-11 rounded-2xl'>
                      <SelectValue placeholder='All providers' />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value={ALL_FILTER}>All providers</SelectItem>
                      {providerOptions.map((provider) => (
                        <SelectItem key={provider} value={provider}>
                          {providerLabel(provider)}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </label>

                <label className='space-y-2'>
                  <span className='text-muted-foreground text-sm'>Status</span>
                  <Select value={statusFilter} onValueChange={setStatusFilter}>
                    <SelectTrigger className='h-11 rounded-2xl'>
                      <SelectValue placeholder='All statuses' />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value={ALL_FILTER}>All statuses</SelectItem>
                      {STATUS_OPTIONS.map((status) => (
                        <SelectItem key={status} value={status}>
                          {status}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </label>

                <p className='text-muted-foreground self-end text-sm leading-6'>
                  Sorted by latest update. Inline edit updates the persisted account label.
                </p>
              </div>

              {!hasItems ? (
                <article className='border-border bg-muted/40 text-muted-foreground rounded-3xl border border-dashed p-6 text-sm'>
                  <p className='text-foreground'>No auth records match the current filters.</p>
                  <p className='mt-2'>Change the filters or add another account above.</p>
                </article>
              ) : null}

              <div className='space-y-6'>
                {providerGroups.map((group) => (
                  <section key={group.key} className='space-y-3'>
                    <div className='flex items-center gap-2'>
                      <h4 className='text-base font-semibold'>{group.label}</h4>
                      <Badge variant='outline' className='rounded-full px-2.5 py-1 text-xs'>
                        {group.items.length}
                      </Badge>
                    </div>

                    <div className='grid gap-3'>
                      {group.items.map((item) => (
                        <Card key={item.id} className='border-border rounded-2xl border'>
                          <CardContent className='space-y-4 p-4'>
                            <div className='flex flex-col gap-3 xl:flex-row xl:items-start xl:justify-between'>
                              <div className='min-w-0'>
                                <div className='flex flex-wrap items-center gap-2'>
                                  <p className='text-foreground font-medium'>
                                    {item.label || item.id}
                                  </p>
                                  <Badge
                                    variant='outline'
                                    className={`rounded-full px-2.5 py-1 text-xs ${statusTone(item.status)}`}
                                  >
                                    {item.status}
                                  </Badge>
                                </div>
                                <p className='text-muted-foreground mt-1 truncate text-sm'>
                                  {accountSubtitle(item)}
                                </p>
                                <p className='text-muted-foreground mt-1 truncate text-xs'>
                                  {item.id}
                                </p>
                                {item.provider_key === 'kiro' ? renderKiroMetadata(item) : null}
                                {item.status_message ? (
                                  <p className='text-destructive dark:text-destructive-foreground mt-2 text-xs'>
                                    {item.status_message}
                                  </p>
                                ) : null}
                                {item.provider_key === 'kiro' && quotaResults[item.id] ? (
                                  <div className='border-border bg-muted/50 mt-3 rounded-lg border p-3'>
                                    <p className='text-xs font-medium'>Quota Status:</p>
                                    <div className='mt-1 flex items-center gap-2'>
                                      <Badge
                                        variant={
                                          quotaResults[item.id].status === 'available'
                                            ? 'default'
                                            : quotaResults[item.id].status === 'exhausted'
                                              ? 'destructive'
                                              : 'outline'
                                        }
                                        className='rounded-full'
                                      >
                                        {quotaResults[item.id].status}
                                      </Badge>
                                      {quotaResults[item.id].remaining !== undefined && (
                                        <span className='text-muted-foreground text-xs'>
                                          {quotaResults[item.id].remaining} remaining
                                        </span>
                                      )}
                                    </div>
                                    {quotaResults[item.id].detail && (
                                      <p className='text-muted-foreground mt-1 text-xs'>
                                        {quotaResults[item.id].detail}
                                      </p>
                                    )}
                                    {quotaResults[item.id].message && (
                                      <p className='text-muted-foreground mt-1 text-xs'>
                                        {quotaResults[item.id].message}
                                      </p>
                                    )}
                                  </div>
                                ) : null}
                                {item.provider_key === 'zed' && zedQuotaResults[item.id] ? (
                                  <div className='border-border bg-muted/50 mt-3 rounded-lg border p-3'>
                                    <p className='text-xs font-medium'>Quota Status:</p>
                                    <div className='mt-1 flex items-center gap-2'>
                                      <Badge
                                        variant={
                                          zedQuotaResults[item.id].status === 'available'
                                            ? 'default'
                                            : 'destructive'
                                        }
                                        className='rounded-full'
                                      >
                                        {zedQuotaResults[item.id].status}
                                      </Badge>
                                      {zedQuotaResults[item.id].model_requests_used !== undefined &&
                                      zedQuotaResults[item.id].model_requests_limit !==
                                        undefined ? (
                                        <span className='text-muted-foreground text-xs'>
                                          {zedQuotaResults[item.id].model_requests_used}/
                                          {String(zedQuotaResults[item.id].model_requests_limit)}{' '}
                                          used
                                        </span>
                                      ) : null}
                                    </div>
                                    {zedQuotaResults[item.id].plan ? (
                                      <p className='text-muted-foreground mt-1 text-xs'>
                                        Plan: {zedQuotaResults[item.id].plan}
                                      </p>
                                    ) : null}
                                    {zedQuotaResults[item.id].error ? (
                                      <p className='text-muted-foreground mt-1 text-xs'>
                                        {zedQuotaResults[item.id].error}
                                      </p>
                                    ) : null}
                                  </div>
                                ) : null}
                                {zedModelsResults[item.id] ? (
                                  <div className='border-border bg-muted/50 mt-3 rounded-lg border p-3'>
                                    <p className='text-xs font-medium'>Available Models:</p>
                                    {zedModelsResults[item.id].models.length > 0 ? (
                                      <div className='mt-2 flex flex-wrap gap-2'>
                                        {zedModelsResults[item.id].models.map((model) => (
                                          <Badge
                                            key={`${item.id}-${model}`}
                                            variant='outline'
                                            className='rounded-full px-2.5 py-1 text-xs'
                                          >
                                            {model}
                                          </Badge>
                                        ))}
                                      </div>
                                    ) : (
                                      <p className='text-muted-foreground mt-1 text-xs'>
                                        No models reported for this account.
                                      </p>
                                    )}
                                  </div>
                                ) : null}
                              </div>

                              <div className='text-muted-foreground text-sm xl:text-right'>
                                <p>{new Date(item.updated_at).toLocaleString()}</p>
                                <p className='mt-1 text-xs'>
                                  Refreshed:{' '}
                                  {item.last_refreshed_at
                                    ? new Date(item.last_refreshed_at).toLocaleString()
                                    : 'never'}
                                </p>
                              </div>
                            </div>

                            {editName === item.id ? (
                              <div className='border-border bg-background/50 space-y-3 rounded-2xl border p-4'>
                                <Input
                                  type='text'
                                  value={editLabel}
                                  onChange={(event) => setEditLabel(event.target.value)}
                                  className='h-11 rounded-xl'
                                  placeholder='Account label'
                                />
                                <div className='flex flex-col gap-2 sm:flex-row'>
                                  <Button
                                    type='button'
                                    onClick={() =>
                                      patchFields.mutate({ name: item.id, label: editLabel.trim() })
                                    }
                                    disabled={
                                      patchFields.isPending || editLabel.trim().length === 0
                                    }
                                    className='h-11 rounded-xl px-3'
                                  >
                                    Save
                                  </Button>
                                  <Button
                                    type='button'
                                    variant='outline'
                                    onClick={() => {
                                      setEditName(null)
                                      setEditLabel('')
                                    }}
                                    className='h-11 rounded-xl px-3'
                                  >
                                    Cancel
                                  </Button>
                                </div>
                              </div>
                            ) : null}

                            <div className='grid gap-2 sm:grid-cols-2 xl:grid-cols-4'>
                              {item.provider_key === 'zed' && (
                                <Button
                                  type='button'
                                  variant='outline'
                                  onClick={async () => {
                                    try {
                                      const result = await fetchZedModels.mutateAsync({
                                        name: item.id,
                                      })
                                      setZedModelsResults((prev) => ({
                                        ...prev,
                                        [item.id]: result,
                                      }))
                                    } catch (error) {
                                      console.error('Zed models fetch failed:', error)
                                    }
                                  }}
                                  disabled={fetchZedModels.isPending}
                                  className='h-11 rounded-xl px-3'
                                >
                                  {fetchZedModels.isPending ? 'Loading...' : 'Get Models'}
                                </Button>
                              )}
                              {item.provider_key === 'kiro' && (
                                <Button
                                  type='button'
                                  variant='outline'
                                  onClick={async () => {
                                    try {
                                      const result = await checkKiroQuota.mutateAsync({
                                        name: item.id,
                                      })
                                      setQuotaResults((prev) => ({ ...prev, [item.id]: result }))
                                    } catch (error) {
                                      console.error('Quota check failed:', error)
                                    }
                                  }}
                                  disabled={checkKiroQuota.isPending}
                                  className='h-11 rounded-xl px-3'
                                >
                                  {checkKiroQuota.isPending ? 'Checking...' : 'Check Quota'}
                                </Button>
                              )}
                              {item.provider_key === 'zed' && (
                                <Button
                                  type='button'
                                  variant='outline'
                                  onClick={async () => {
                                    try {
                                      const result = await checkZedQuota.mutateAsync({
                                        name: item.id,
                                      })
                                      setZedQuotaResults((prev) => ({ ...prev, [item.id]: result }))
                                    } catch (error) {
                                      console.error('Zed quota check failed:', error)
                                    }
                                  }}
                                  disabled={checkZedQuota.isPending}
                                  className='h-11 rounded-xl px-3'
                                >
                                  {checkZedQuota.isPending ? 'Checking...' : 'Check Quota'}
                                </Button>
                              )}
                              <Button
                                type='button'
                                variant='outline'
                                onClick={() =>
                                  toggleStatus.mutate({
                                    name: item.id,
                                    disabled: item.status !== 'disabled',
                                  })
                                }
                                disabled={toggleStatus.isPending}
                                className='h-11 rounded-xl px-3'
                              >
                                {item.status === 'disabled' ? 'Enable' : 'Disable'}
                              </Button>
                              <Button
                                type='button'
                                variant='outline'
                                onClick={() => {
                                  setEditName(item.id)
                                  setEditLabel(item.label)
                                }}
                                className='h-11 rounded-xl px-3'
                              >
                                Edit label
                              </Button>
                              <Button
                                type='button'
                                variant='outline'
                                onClick={() => downloadAuthFile(item.id)}
                                className='h-11 rounded-xl px-3'
                              >
                                Download
                              </Button>
                              <Button
                                type='button'
                                variant='destructive'
                                onClick={() => {
                                  if (window.confirm(`Delete ${item.id}?`)) {
                                    deleteAuthFile.mutate(item.id)
                                  }
                                }}
                                disabled={deleteAuthFile.isPending}
                                className='h-11 rounded-xl px-3'
                              >
                                Delete
                              </Button>
                            </div>
                          </CardContent>
                        </Card>
                      ))}
                    </div>
                  </section>
                ))}
              </div>
            </section>

            <section className='space-y-4'>
              <div>
                <h3 className='text-lg font-semibold'>Advanced fallback: upload raw auth file</h3>
                <p className='text-muted-foreground mt-1 text-sm'>
                  Keep for non-Kiro/manual recovery flows. Prefer provider-specific onboarding
                  above.
                </p>
              </div>

              <div className='grid gap-4'>
                <Input
                  type='file'
                  accept='.json,application/json'
                  onChange={async (event) => {
                    const file = event.target.files?.[0]
                    event.currentTarget.value = ''

                    if (!file) return
                    if (file.size > MAX_UPLOAD_FILE_SIZE) {
                      setUploadFileError('JSON file too large. Max 1 MB.')
                      return
                    }

                    setUploadFileError(null)

                    try {
                      const body = await file.text()
                      const nextName = file.name.slice(0, MAX_UPLOAD_NAME_LENGTH)
                      const nextBody = body.slice(0, MAX_UPLOAD_BODY_LENGTH)
                      setUploadName(nextName)
                      setUploadBody(nextBody)
                      setUploadFileError(
                        body.length > MAX_UPLOAD_BODY_LENGTH
                          ? `File truncated to ${MAX_UPLOAD_BODY_LENGTH.toLocaleString()} characters.`
                          : null,
                      )

                      if (nextName.trim().length === 0 || nextBody.trim().length === 0) {
                        setUploadFileError('Selected JSON file is empty.')
                        return
                      }

                      uploadAuthFile.mutate({ name: nextName.trim(), body: nextBody.trim() })
                    } catch {
                      setUploadFileError('Failed to read selected JSON file.')
                    }
                  }}
                  className='h-11 rounded-2xl file:mr-4 file:border-0 file:bg-transparent file:text-sm file:font-medium'
                />
                {uploadFileError ? (
                  <p className='border-destructive/30 bg-destructive/10 text-destructive dark:text-destructive-foreground rounded-2xl border px-4 py-3 text-sm'>
                    {uploadFileError}
                  </p>
                ) : null}
                <Input
                  type='text'
                  value={uploadName}
                  onChange={(event) => setUploadName(event.target.value)}
                  className='h-11 rounded-2xl'
                  placeholder='auth-file.json'
                  maxLength={MAX_UPLOAD_NAME_LENGTH}
                />
                <Textarea
                  value={uploadBody}
                  onChange={(event) => setUploadBody(event.target.value)}
                  className='min-h-40 rounded-2xl px-4 py-3'
                  placeholder='{"type":"antigravity"}'
                  maxLength={MAX_UPLOAD_BODY_LENGTH}
                />
                <div>
                  <Button
                    type='button'
                    onClick={() =>
                      uploadAuthFile.mutate({ name: uploadName.trim(), body: uploadBody.trim() })
                    }
                    disabled={
                      uploadAuthFile.isPending ||
                      uploadName.trim().length === 0 ||
                      uploadBody.trim().length === 0
                    }
                    className='h-11 rounded-xl px-4'
                  >
                    {uploadAuthFile.isPending ? 'Uploading…' : 'Upload auth file'}
                  </Button>
                </div>
              </div>
            </section>
          </div>
        ) : null}
      </QueryState>
    </PageShell>
  )
}
