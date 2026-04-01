import { Link } from '@tanstack/react-router'
import { useEffect, useRef, useState } from 'react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
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

import { useUploadAuthFileMutation } from '../../lib/management-auth-files'
import {
  useImportKiroMutation,
  useImportKiroSocialMutation,
  useStartKiroBuilderIdMutation,
} from '../../lib/management-kiro'
import {
  useOAuthStatusQuery,
  useStartOAuthMutation,
  useSubmitOAuthCallbackMutation,
} from '../../lib/management-oauth'
import {
  useStartZedLoginMutation,
  useZedLoginStatusQuery,
} from '../../lib/management-zed'
import { buildOAuthTerminalFeedback } from '../../lib/oauth-feedback'
import { toastError, toastInfo, toastSuccess } from '../../lib/toast'
import {
  formatOauthExpiryHint,
  resolveTrackedOauthSession,
  type AddAccountOauthProvider,
  type TrackedOauthStates,
} from './add-account-page.oauth'
import { PageShell } from '../page-shell'

const MAX_LABEL_LENGTH = 200
const MAX_UPLOAD_NAME_LENGTH = 200
const MAX_UPLOAD_BODY_LENGTH = 20000
const MAX_UPLOAD_FILE_SIZE = 1024 * 1024

function useProviderOauthStatus(state: string | undefined) {
  return useOAuthStatusQuery(state ?? null, Boolean(state))
}

export function AddAccountPage() {
  const uploadAuthFile = useUploadAuthFileMutation()
  const startOAuth = useStartOAuthMutation()
  const submitOAuthCallback = useSubmitOAuthCallbackMutation()
  const startKiroBuilderId = useStartKiroBuilderIdMutation()
  const startZedLogin = useStartZedLoginMutation()
  const importKiro = useImportKiroMutation()
  const importKiroSocial = useImportKiroSocialMutation()

  const [oauthStates, setOauthStates] = useState<TrackedOauthStates>({})
  const [provider, setProvider] = useState<AddAccountOauthProvider>('kiro')

  const [antigravityLabel, setAntigravityLabel] = useState('')
  const [antigravityAuthUrl, setAntigravityAuthUrl] = useState('')
  const [antigravityCallbackUrl, setAntigravityCallbackUrl] = useState('')

  const [codexLabel, setCodexLabel] = useState('')
  const [codexAuthUrl, setCodexAuthUrl] = useState('')
  const [codexCallbackUrl, setCodexCallbackUrl] = useState('')

  const [zedLabel, setZedLabel] = useState('')
  const [zedLoginUrl, setZedLoginUrl] = useState('')
  const [zedPort, setZedPort] = useState<number | null>(null)
  const [zedSessionId, setZedSessionId] = useState<string | null>(null)

  const [copilotLabel, setCopilotLabel] = useState('')
  const [copilotUserCode, setCopilotUserCode] = useState('')
  const [copilotVerificationUri, setCopilotVerificationUri] = useState('')
  const [copilotExpiresIn, setCopilotExpiresIn] = useState<number | undefined>()

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
  const [showAdvanced, setShowAdvanced] = useState(false)

  const kiroOauthState = oauthStates.kiro
  const antigravityOauthState = oauthStates.antigravity
  const codexOauthState = oauthStates.codex
  const copilotOauthState = oauthStates['github-copilot']

  const kiroOauthStatus = useProviderOauthStatus(kiroOauthState)
  const antigravityOauthStatus = useProviderOauthStatus(antigravityOauthState)
  const codexOauthStatus = useProviderOauthStatus(codexOauthState)
  const zedLoginStatus = useZedLoginStatusQuery(zedSessionId, Boolean(zedSessionId))
  const copilotOauthStatus = useProviderOauthStatus(copilotOauthState)

  const oauthStatusByProvider = {
    kiro: kiroOauthStatus,
    antigravity: antigravityOauthStatus,
    codex: codexOauthStatus,
    'github-copilot': copilotOauthStatus,
  } as const

  const activeOauthState = provider === 'zed' ? zedSessionId : oauthStates[provider]
  const activeOauthStatus = provider === 'zed' ? null : oauthStatusByProvider[provider]
  const activeOauthStatusData = activeOauthStatus?.data
  const activeOauthStatusSummary =
    provider === 'zed'
      ? (zedLoginStatus.data?.status ?? (zedLoginStatus.isFetching ? 'waiting' : 'idle'))
      : activeOauthState
        ? (activeOauthStatusData?.status ?? (activeOauthStatus?.isFetching ? 'wait' : 'idle'))
        : 'idle'
  const copilotExpiryHint = formatOauthExpiryHint(copilotExpiresIn)
  const lastNotifiedOauthStates = useRef<Partial<Record<AddAccountOauthProvider, string>>>({})
  const lastNotifiedZedSessionId = useRef<string | null>(null)

  useEffect(() => {
    const providerStatuses = {
      kiro: {
        status: kiroOauthStatus.data?.status,
        error: kiroOauthStatus.data?.error,
      },
      antigravity: {
        status: antigravityOauthStatus.data?.status,
        error: antigravityOauthStatus.data?.error,
      },
      codex: {
        status: codexOauthStatus.data?.status,
        error: codexOauthStatus.data?.error,
      },
      'github-copilot': {
        status: copilotOauthStatus.data?.status,
        error: copilotOauthStatus.data?.error,
      },
    } as const

    const trackedProviders: Array<Exclude<AddAccountOauthProvider, 'zed'>> = [
      'kiro',
      'antigravity',
      'codex',
      'github-copilot',
    ]

    for (const trackedProvider of trackedProviders) {
      const trackedState = oauthStates[trackedProvider]
      const trackedStatus = providerStatuses[trackedProvider].status
      const trackedError = providerStatuses[trackedProvider].error

      if (!trackedState || !trackedStatus || trackedStatus === 'wait') {
        continue
      }

      if (lastNotifiedOauthStates.current[trackedProvider] === trackedState) {
        continue
      }

      const feedback = buildOAuthTerminalFeedback(trackedStatus, trackedError)
      if (!feedback) {
        continue
      }

      if (feedback.type === 'success') {
        toastSuccess(feedback.title, feedback.detail)
      } else {
        toastError(feedback.title, feedback.detail)
      }

      lastNotifiedOauthStates.current[trackedProvider] = trackedState
    }
  }, [
    oauthStates,
    kiroOauthStatus.data?.status,
    kiroOauthStatus.data?.error,
    antigravityOauthStatus.data?.status,
    antigravityOauthStatus.data?.error,
    codexOauthStatus.data?.status,
    codexOauthStatus.data?.error,
    copilotOauthStatus.data?.status,
    copilotOauthStatus.data?.error,
  ])

  useEffect(() => {
    if (!zedSessionId || !zedLoginStatus.data?.status || zedLoginStatus.data.status === 'waiting') {
      return
    }

    if (lastNotifiedZedSessionId.current === zedSessionId) {
      return
    }

    toastSuccess('Zed login complete', 'Account connected. Open Accounts to review it.')
    lastNotifiedZedSessionId.current = zedSessionId
  }, [zedSessionId, zedLoginStatus.data?.status])

  function submitKiroStructuredImport() {
    importKiro.mutate(
      {
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
      },
      {
        onSuccess: () => {
          toastSuccess('Kiro account added', 'Open Accounts to review it.')
        },
        onError: (error) => {
          toastError('Could not add the Kiro account', error.message)
        },
      },
    )
  }

  function submitKiroJsonImport() {
    const parsed = JSON.parse(kiroImportJson) as Record<string, unknown>
    importKiro.mutate(
      {
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
      },
      {
        onSuccess: () => {
          toastSuccess('Kiro account added', 'Open Accounts to review it.')
        },
        onError: (error) => {
          toastError('Could not add the Kiro account', error.message)
        },
      },
    )
  }

  return (
    <PageShell
      eyebrow='Add Account'
      title='Add a provider account'
      description='Choose a provider to connect. Review and manage accounts on the Accounts page.'
      actions={
        <Button asChild variant='outline' className='rounded-xl'>
          <Link to='/accounts'>Back to Accounts</Link>
        </Button>
      }
    >
      <div className='space-y-8'>
        <section className='dashboard-enter-delay-1 motion-panel border-border bg-muted/30 rounded-3xl border p-5'>
          <p className='text-sm font-medium'>Start here</p>
          <ol className='text-muted-foreground mt-3 grid gap-2 text-sm md:grid-cols-3'>
            <li>1. Choose a provider</li>
            <li>2. Start sign-in or import tokens</li>
            <li>3. Open Accounts to verify the result</li>
          </ol>
        </section>

        <section className='space-y-4'>
          <Tabs
            value={provider}
            onValueChange={(value) => setProvider(value as AddAccountOauthProvider)}
          >
            <TabsList className='motion-panel grid w-full max-w-2xl grid-cols-5 rounded-2xl p-1'>
              <TabsTrigger value='kiro' className='rounded-xl'>
                Kiro
              </TabsTrigger>
              <TabsTrigger value='antigravity' className='rounded-xl'>
                Antigravity
              </TabsTrigger>
              <TabsTrigger value='codex' className='rounded-xl'>
                Codex
              </TabsTrigger>
              <TabsTrigger value='zed' className='rounded-xl'>
                Zed
              </TabsTrigger>
              <TabsTrigger value='github-copilot' className='rounded-xl'>
                Copilot
              </TabsTrigger>
            </TabsList>

            <TabsContent value='kiro' className='space-y-6 pt-2'>
              <section className='space-y-3'>
                <div className='flex items-center gap-2'>
                  <h3 className='text-lg font-semibold'>Kiro</h3>
                  <Badge variant='outline' className='rounded-full px-2.5 py-1 text-xs'>
                    fastest path
                  </Badge>
                </div>
                <p className='text-muted-foreground max-w-2xl text-sm'>
                  Use Builder ID first. Use imports only if you already have tokens.
                </p>
              </section>

              <section className='space-y-3'>
                <div className='motion-panel flex flex-col gap-3 rounded-2xl border p-4 sm:flex-row sm:items-center sm:justify-between'>
                  <div>
                    <p className='font-medium'>Builder ID</p>
                    <p className='text-muted-foreground text-sm'>
                      Opens a sign-in flow in a new tab.
                    </p>
                  </div>
                  <Button
                    type='button'
                    onClick={() =>
                      startKiroBuilderId.mutate(
                        { label: kiroLabel.trim() || undefined },
                        {
                          onSuccess: (data) => {
                            setOauthStates((prev) => ({ ...prev, kiro: data.session_id }))
                            toastSuccess(
                              'Kiro sign-in started',
                              'Finish the flow, then open Accounts.',
                            )
                            window.open(data.auth_url, '_blank', 'noopener,noreferrer')
                          },
                          onError: (error) => {
                            toastError('Could not start Kiro sign-in', error.message)
                          },
                        },
                      )
                    }
                    disabled={startKiroBuilderId.isPending}
                    className='rounded-xl px-4'
                  >
                    {startKiroBuilderId.isPending ? 'Launching…' : 'Start Builder ID'}
                  </Button>
                </div>

                <div className='motion-panel border-border bg-background/60 text-muted-foreground rounded-2xl border p-4 text-sm leading-6'>
                  {activeOauthState ? (
                    <>
                      <p>
                        Session ID <span className='text-foreground break-all'>{activeOauthState}</span>
                      </p>
                      <p>
                        Status <span className='text-foreground'>{activeOauthStatusSummary}</span>
                      </p>
                    </>
                  ) : (
                    <p>No sign-in session started yet.</p>
                  )}
                  {activeOauthStatusData?.error ? (
                    <p className='text-destructive mt-2'>{activeOauthStatusData.error}</p>
                  ) : null}
                </div>
              </section>

              <section className='space-y-4'>
                <div className='flex flex-wrap items-center justify-between gap-3'>
                  <div>
                    <h4 className='font-medium'>Import tokens</h4>
                    <p className='text-muted-foreground text-sm'>
                      Paste token fields or full JSON.
                    </p>
                  </div>
                  <Select
                    value={kiroImportMode}
                    onValueChange={(value) => setKiroImportMode(value as 'structured' | 'json')}
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
                    placeholder='{"access_token":"...","refresh_token":"..."}'
                    className='min-h-40 rounded-2xl px-4 py-3'
                  />
                )}

                <div className='flex justify-end'>
                  <Button
                    type='button'
                    onClick={() => {
                      try {
                        if (kiroImportMode === 'json') {
                          submitKiroJsonImport()
                          return
                        }
                        submitKiroStructuredImport()
                      } catch (error) {
                        toastError(
                          'Could not read the JSON',
                          error instanceof Error ? error.message : 'Check the JSON and try again.',
                        )
                      }
                    }}
                    disabled={importKiro.isPending}
                    className='h-11 rounded-xl px-4'
                  >
                    {importKiro.isPending ? 'Importing…' : 'Import Kiro auth'}
                  </Button>
                </div>
              </section>

              <section className='space-y-3'>
                <div>
                  <h4 className='font-medium'>Import social token</h4>
                  <p className='text-muted-foreground text-sm'>
                    Use this only if you already have a refresh token.
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
                      importKiroSocial.mutate(
                        {
                          refresh_token: kiroSocialRefreshToken.trim(),
                          label: kiroLabel.trim() || undefined,
                        },
                        {
                          onSuccess: () => {
                            toastSuccess('Kiro account added', 'Open Accounts to review it.')
                          },
                          onError: (error) => {
                            toastError('Could not add the Kiro account', error.message)
                          },
                        },
                      )
                    }
                    disabled={importKiroSocial.isPending}
                    className='h-11 rounded-xl px-4'
                  >
                    {importKiroSocial.isPending ? 'Importing…' : 'Import social token'}
                  </Button>
                </div>
              </section>
            </TabsContent>

            <TabsContent value='antigravity' className='space-y-6 pt-2'>
              <section className='space-y-3'>
                <h3 className='text-lg font-semibold'>Antigravity</h3>
                <p className='text-muted-foreground max-w-2xl text-sm'>
                  Start sign-in here, then review the account on the Accounts page.
                </p>
              </section>

              <section className='max-w-xl space-y-4'>
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
                            setOauthStates((prev) => ({ ...prev, [data.provider as AddAccountOauthProvider]: data.state }))
                            setAntigravityAuthUrl(data.url ?? '')
                            toastSuccess(
                              'Antigravity OAuth link ready',
                              'Open the link, login, then paste localhost callback URL below.',
                            )
                          },
                          onError: (error) => {
                            toastError('Could not start Antigravity sign-in', error.message)
                          },
                        },
                      )
                    }
                    disabled={startOAuth.isPending}
                    className='h-11 rounded-xl px-4'
                  >
                    {startOAuth.isPending ? 'Generating link…' : 'Start OAuth'}
                  </Button>
                </div>

                {antigravityAuthUrl ? (
                  <div className='border-border bg-background/60 space-y-3 rounded-2xl border p-4'>
                    <p className='text-muted-foreground text-sm'>Open this login link manually:</p>
                    <Input value={antigravityAuthUrl} readOnly className='h-11 rounded-2xl' />
                    <div className='flex justify-end'>
                      <Button
                        asChild
                        type='button'
                        variant='outline'
                        className='h-11 rounded-xl px-4'
                      >
                        <a href={antigravityAuthUrl} target='_blank' rel='noopener noreferrer'>
                          Open login link
                        </a>
                      </Button>
                    </div>
                  </div>
                ) : null}

                <div className='border-border bg-background/60 space-y-3 rounded-2xl border p-4'>
                  <p className='text-muted-foreground text-sm'>
                    Paste localhost callback URL after login:
                  </p>
                  <Textarea
                    value={antigravityCallbackUrl}
                    onChange={(event) => setAntigravityCallbackUrl(event.target.value)}
                    placeholder='http://localhost:3456/antigravity/callback?code=...&state=...'
                    className='min-h-24 rounded-2xl px-4 py-3'
                  />
                  <div className='flex justify-end'>
                    <Button
                      type='button'
                      onClick={() => {
                        const trackedSession = resolveTrackedOauthSession(
                          'antigravity',
                          antigravityCallbackUrl,
                          oauthStates,
                        )

                        submitOAuthCallback.mutate(
                          {
                            provider: 'antigravity',
                            redirectUrl: antigravityCallbackUrl,
                          },
                          {
                            onSuccess: () => {
                              if (trackedSession) {
                                setOauthStates((prev) => ({
                                  ...prev,
                                  [trackedSession.provider]: trackedSession.state,
                                }))
                              }
                              toastSuccess('Callback submitted', 'Polling OAuth status...')
                            },
                            onError: (error) => {
                              toastError('Could not submit callback URL', error.message)
                            },
                          },
                        )
                      }}
                      disabled={
                        submitOAuthCallback.isPending || antigravityCallbackUrl.trim().length === 0
                      }
                      className='h-11 rounded-xl px-4'
                    >
                      {submitOAuthCallback.isPending ? 'Submitting…' : 'Submit callback URL'}
                    </Button>
                  </div>
                </div>
              </section>
            </TabsContent>

            <TabsContent value='codex' className='space-y-6 pt-2'>
              <section className='space-y-3'>
                <h3 className='text-lg font-semibold'>Codex</h3>
                <p className='text-muted-foreground max-w-2xl text-sm'>
                  Start sign-in here, then review the account on the Accounts page.
                </p>
              </section>

              <section className='max-w-xl space-y-4'>
                <Input
                  type='text'
                  value={codexLabel}
                  onChange={(event) => setCodexLabel(event.target.value)}
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
                          provider: 'codex',
                          label: codexLabel.trim() || undefined,
                        },
                        {
                          onSuccess: (data) => {
                            setOauthStates((prev) => ({ ...prev, [data.provider as AddAccountOauthProvider]: data.state }))
                            setCodexAuthUrl(data.url ?? '')
                            toastSuccess(
                              'Codex OAuth link ready',
                              'Open the link, login, then paste localhost callback URL below.',
                            )
                          },
                          onError: (error) => {
                            toastError('Could not start Codex sign-in', error.message)
                          },
                        },
                      )
                    }
                    disabled={startOAuth.isPending}
                    className='h-11 rounded-xl px-4'
                  >
                    {startOAuth.isPending ? 'Generating link…' : 'Start OAuth'}
                  </Button>
                </div>

                {codexAuthUrl ? (
                  <div className='border-border bg-background/60 space-y-3 rounded-2xl border p-4'>
                    <p className='text-muted-foreground text-sm'>Open this login link manually:</p>
                    <Input value={codexAuthUrl} readOnly className='h-11 rounded-2xl' />
                    <div className='flex justify-end'>
                      <Button
                        asChild
                        type='button'
                        variant='outline'
                        className='h-11 rounded-xl px-4'
                      >
                        <a href={codexAuthUrl} target='_blank' rel='noopener noreferrer'>
                          Open login link
                        </a>
                      </Button>
                    </div>
                  </div>
                ) : null}

                <div className='border-border bg-background/60 space-y-3 rounded-2xl border p-4'>
                  <p className='text-muted-foreground text-sm'>
                    Paste localhost callback URL after login:
                  </p>
                  <Textarea
                    value={codexCallbackUrl}
                    onChange={(event) => setCodexCallbackUrl(event.target.value)}
                    placeholder='http://localhost:3456/codex/callback?code=...&state=...'
                    className='min-h-24 rounded-2xl px-4 py-3'
                  />
                  <div className='flex justify-end'>
                    <Button
                      type='button'
                      onClick={() => {
                        const trackedSession = resolveTrackedOauthSession(
                          'codex',
                          codexCallbackUrl,
                          oauthStates,
                        )

                        submitOAuthCallback.mutate(
                          {
                            provider: 'codex',
                            redirectUrl: codexCallbackUrl,
                          },
                          {
                            onSuccess: () => {
                              if (trackedSession) {
                                setOauthStates((prev) => ({
                                  ...prev,
                                  [trackedSession.provider]: trackedSession.state,
                                }))
                              }
                              toastSuccess('Callback submitted', 'Polling OAuth status...')
                            },
                            onError: (error) => {
                              toastError('Could not submit callback URL', error.message)
                            },
                          },
                        )
                      }}
                      disabled={
                        submitOAuthCallback.isPending || codexCallbackUrl.trim().length === 0
                      }
                      className='h-11 rounded-xl px-4'
                    >
                      {submitOAuthCallback.isPending ? 'Submitting…' : 'Submit callback URL'}
                    </Button>
                  </div>
                </div>
              </section>
            </TabsContent>

            <TabsContent value='zed' className='space-y-6 pt-2'>
              <section className='space-y-3'>
                <h3 className='text-lg font-semibold'>Zed</h3>
                <p className='text-muted-foreground max-w-2xl text-sm'>
                  Start the native-app sign-in flow here, then review the account on the Accounts page.
                </p>
              </section>

              <section className='max-w-xl space-y-4'>
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
                        { name: zedLabel.trim() || undefined },
                        {
                          onSuccess: (data) => {
                            setZedSessionId(data.session_id)
                            setZedLoginUrl(data.login_url)
                            setZedPort(data.port)
                            toastSuccess(
                              'Zed login started',
                              'Open the login link and finish the native-app flow.',
                            )
                            window.open(data.login_url, '_blank', 'noopener,noreferrer')
                          },
                          onError: (error) => {
                            toastError('Could not start Zed sign-in', error.message)
                          },
                        },
                      )
                    }
                    disabled={startZedLogin.isPending}
                    className='h-11 rounded-xl px-4'
                  >
                    {startZedLogin.isPending ? 'Launching…' : 'Start Zed login'}
                  </Button>
                </div>

                {zedLoginUrl ? (
                  <div className='border-border bg-background/60 space-y-3 rounded-2xl border p-4'>
                    <p className='text-muted-foreground text-sm'>Open this login link manually:</p>
                    <Input value={zedLoginUrl} readOnly className='h-11 rounded-2xl' />
                    {zedPort ? (
                      <p className='text-muted-foreground text-sm'>Native app callback port: {zedPort}</p>
                    ) : null}
                    <div className='flex justify-end'>
                      <Button
                        asChild
                        type='button'
                        variant='outline'
                        className='h-11 rounded-xl px-4'
                      >
                        <a href={zedLoginUrl} target='_blank' rel='noopener noreferrer'>
                          Open login link
                        </a>
                      </Button>
                    </div>
                  </div>
                ) : null}

                <div className='border-border bg-background/60 text-muted-foreground rounded-2xl border p-4 text-sm leading-6'>
                  {activeOauthState ? (
                    <>
                      <p>
                        Session ID <span className='text-foreground break-all'>{activeOauthState}</span>
                      </p>
                      <p>
                        Status <span className='text-foreground'>{activeOauthStatusSummary}</span>
                      </p>
                      {zedLoginStatus.data?.filename ? (
                        <p>
                          Auth file <span className='text-foreground break-all'>{zedLoginStatus.data.filename}</span>
                        </p>
                      ) : null}
                      {zedLoginStatus.data?.user_id ? (
                        <p>
                          User ID <span className='text-foreground break-all'>{zedLoginStatus.data.user_id}</span>
                        </p>
                      ) : null}
                    </>
                  ) : (
                    <p>No sign-in session started yet.</p>
                  )}
                  {zedLoginStatus.error ? (
                    <p className='text-destructive mt-2'>{zedLoginStatus.error.message}</p>
                  ) : null}
                </div>
              </section>
            </TabsContent>

            <TabsContent value='github-copilot' className='space-y-6 pt-2'>
              <section className='space-y-3'>
                <h3 className='text-lg font-semibold'>GitHub Copilot</h3>
                <p className='text-muted-foreground max-w-2xl text-sm'>
                  Start the device-code flow, then approve it on GitHub.
                </p>
              </section>

              <section className='max-w-xl space-y-4'>
                <Input
                  type='text'
                  value={copilotLabel}
                  onChange={(event) => setCopilotLabel(event.target.value)}
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
                          provider: 'github-copilot',
                          label: copilotLabel.trim() || undefined,
                        },
                        {
                          onSuccess: (data) => {
                            setOauthStates((prev) => ({
                              ...prev,
                              'github-copilot': data.state,
                            }))
                            setCopilotUserCode(data.user_code ?? '')
                            setCopilotVerificationUri(data.verification_uri ?? '')
                            setCopilotExpiresIn(data.expires_in)
                            toastSuccess(
                              'GitHub Copilot sign-in started',
                              'Open GitHub and enter the device code.',
                            )
                          },
                          onError: (error) => {
                            toastError('Could not start GitHub Copilot sign-in', error.message)
                          },
                        },
                      )
                    }
                    disabled={startOAuth.isPending}
                    className='h-11 rounded-xl px-4'
                  >
                    {startOAuth.isPending ? 'Starting…' : 'Start OAuth'}
                  </Button>
                </div>

                {copilotUserCode ? (
                  <div className='border-border bg-background/60 space-y-3 rounded-2xl border p-4'>
                    <p className='text-muted-foreground text-sm'>
                      Enter this code on GitHub:
                    </p>
                    <Input
                      value={copilotUserCode}
                      readOnly
                      className='h-11 rounded-2xl text-center font-mono text-lg font-semibold'
                    />
                    <Input
                      value={copilotVerificationUri}
                      readOnly
                      className='h-11 rounded-2xl'
                    />
                    {copilotExpiryHint ? (
                      <p className='text-muted-foreground text-sm'>{copilotExpiryHint}</p>
                    ) : null}
                    <div className='flex justify-end'>
                      <Button
                        asChild
                        type='button'
                        variant='outline'
                        className='h-11 rounded-xl px-4'
                      >
                        <a
                          href={copilotVerificationUri}
                          target='_blank'
                          rel='noopener noreferrer'
                        >
                          Open GitHub
                        </a>
                      </Button>
                    </div>
                  </div>
                ) : null}

                <div className='border-border bg-background/60 text-muted-foreground rounded-2xl border p-4 text-sm leading-6'>
                  {activeOauthState ? (
                    <>
                      <p>
                        Session ID <span className='text-foreground break-all'>{activeOauthState}</span>
                      </p>
                      <p>
                        Status <span className='text-foreground'>{activeOauthStatusSummary}</span>
                      </p>
                    </>
                  ) : (
                    <p>No sign-in session started yet.</p>
                  )}
                  {activeOauthStatusData?.error ? (
                    <p className='text-destructive mt-2'>{activeOauthStatusData.error}</p>
                  ) : null}
                </div>
              </section>
            </TabsContent>
          </Tabs>
        </section>

        <section className='space-y-3'>
          <Collapsible open={showAdvanced} onOpenChange={setShowAdvanced}>
            <div className='motion-panel flex items-center justify-between gap-3'>
              <div>
                <h3 className='text-base font-semibold'>Manual recovery</h3>
                <p className='text-muted-foreground text-sm'>
                  Use this only if sign-in or token import is not an option.
                </p>
              </div>
              <CollapsibleTrigger asChild>
                <Button type='button' variant='outline' className='rounded-xl'>
                  {showAdvanced ? 'Hide' : 'Show'}
                </Button>
              </CollapsibleTrigger>
            </div>

            <CollapsibleContent className='collapsible-smooth overflow-hidden pt-4'>
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
                      toastError('JSON file too large', 'Max 1 MB.')
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
                      if (body.length > MAX_UPLOAD_BODY_LENGTH) {
                        toastInfo(
                          'File truncated',
                          `Limited to ${MAX_UPLOAD_BODY_LENGTH.toLocaleString()} characters.`,
                        )
                      }

                      if (nextName.trim().length === 0 || nextBody.trim().length === 0) {
                        setUploadFileError('Selected JSON file is empty.')
                        toastError('Selected JSON file is empty')
                        return
                      }

                      uploadAuthFile.mutate(
                        { name: nextName.trim(), body: nextBody.trim() },
                        {
                          onSuccess: () => {
                            toastSuccess('Auth file uploaded', 'Open Accounts to review it.')
                          },
                          onError: (error) => {
                            toastError('Failed to upload auth file', error.message)
                          },
                        },
                      )
                    } catch {
                      setUploadFileError('Failed to read selected JSON file.')
                      toastError('Failed to read selected JSON file')
                    }
                  }}
                  className='h-11 rounded-2xl file:mr-4 file:border-0 file:bg-transparent file:text-sm file:font-medium'
                />
                {uploadFileError ? (
                  <p className='text-destructive text-sm'>{uploadFileError}</p>
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
                      uploadAuthFile.mutate(
                        { name: uploadName.trim(), body: uploadBody.trim() },
                        {
                          onSuccess: () => {
                            toastSuccess('Auth file uploaded', 'Open Accounts to review it.')
                          },
                          onError: (error) => {
                            toastError('Failed to upload auth file', error.message)
                          },
                        },
                      )
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
            </CollapsibleContent>
          </Collapsible>
        </section>
      </div>
    </PageShell>
  )
}
