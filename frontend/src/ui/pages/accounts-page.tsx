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
import { Textarea } from '@/components/ui/textarea'

import {
  downloadAuthFile,
  useDeleteAuthFileMutation,
  usePatchAuthFileFieldsMutation,
  useToggleAuthFileStatusMutation,
  useUploadAuthFileMutation,
} from '../../lib/management-auth-files'
import {
  type OAuthProvider,
  useOAuthStatusQuery,
  useStartOAuthMutation,
} from '../../lib/management-oauth'
import { useAccountsQuery } from '../../lib/query'
import { PageShell } from '../page-shell'
import { QueryState } from '../query-state'
import { statusTone } from '../status-tone'

const ALL_FILTER = 'all'
const STATUS_OPTIONS = ['active', 'refreshing', 'pending', 'error', 'disabled', 'unknown'] as const

const MAX_LABEL_LENGTH = 200
const MAX_UPLOAD_NAME_LENGTH = 200
const MAX_UPLOAD_BODY_LENGTH = 20000
const MAX_UPLOAD_FILE_SIZE = 1024 * 1024

export function AccountsPage() {
  const accounts = useAccountsQuery()
  const toggleStatus = useToggleAuthFileStatusMutation()
  const patchFields = usePatchAuthFileFieldsMutation()
  const deleteAuthFile = useDeleteAuthFileMutation()
  const uploadAuthFile = useUploadAuthFileMutation()
  const startOAuth = useStartOAuthMutation()

  const [providerFilter, setProviderFilter] = useState(ALL_FILTER)
  const [statusFilter, setStatusFilter] = useState(ALL_FILTER)
  const [editName, setEditName] = useState<string | null>(null)
  const [editLabel, setEditLabel] = useState('')
  const [uploadName, setUploadName] = useState('')
  const [uploadBody, setUploadBody] = useState('')
  const [oauthProvider, setOauthProvider] = useState<OAuthProvider>('antigravity')
  const [oauthLabel, setOauthLabel] = useState('')
  const [oauthState, setOauthState] = useState<string | null>(null)
  const [uploadFileError, setUploadFileError] = useState<string | null>(null)

  const oauthStatus = useOAuthStatusQuery(oauthState, Boolean(oauthState))
  const oauthStatusData = oauthStatus.data

  const oauthStatusSummary = oauthState
    ? (oauthStatusData?.status ?? (oauthStatus.isFetching ? 'wait' : 'idle'))
    : 'idle'

  const items = useMemo(() => {
    const source = accounts.data?.items ?? []

    return [...source]
      .filter((item) => providerFilter === ALL_FILTER || item.provider === providerFilter)
      .filter((item) => statusFilter === ALL_FILTER || item.status === statusFilter)
      .sort((a, b) => Date.parse(b.updated_at) - Date.parse(a.updated_at))
  }, [accounts.data?.items, providerFilter, statusFilter])

  const providers = useMemo(
    () => (accounts.data?.grouped_counts ?? []).map((item) => item.provider),
    [accounts.data?.grouped_counts],
  )

  const mutationError = [
    toggleStatus,
    patchFields,
    deleteAuthFile,
    uploadAuthFile,
    startOAuth,
  ].find((mutation) => mutation.isError)?.error as Error | undefined

  const hasItems = items.length > 0

  return (
    <PageShell
      eyebrow='Accounts'
      title='Provider-agnostic auth inventory'
      description='Read-only inventory plus core management actions from the existing management API.'
    >
      <QueryState
        isLoading={accounts.isLoading}
        isError={accounts.isError}
        error={accounts.error as Error | null}
      >
        {accounts.data ? (
          <div className='space-y-6'>
            <div className='flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between'>
              <div>
                <p className='text-muted-foreground text-sm'>
                  {accounts.data.total} record{accounts.data.total === 1 ? '' : 's'} in inventory
                </p>
                <div className='mt-3 flex flex-wrap gap-2'>
                  {accounts.data.grouped_counts.map((summary) => (
                    <Badge
                      key={summary.provider}
                      variant='outline'
                      className='rounded-full px-3 py-1 text-sm'
                    >
                      {summary.provider} · {summary.total}
                    </Badge>
                  ))}
                </div>
              </div>
              <Badge variant='outline' className='w-fit rounded-full px-3 py-1 text-xs'>
                {hasItems ? `${items.length} visible` : 'No matches'}
              </Badge>
            </div>

            <div className='grid gap-5 xl:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]'>
              <section className='space-y-4'>
                <div>
                  <h3 className='text-lg font-semibold'>Filters</h3>
                  <p className='text-muted-foreground mt-1 text-sm'>
                    Narrow the inventory, then act on a record.
                  </p>
                </div>
                <div className='grid gap-4 xl:grid-cols-[0.8fr_0.8fr_1.1fr]'>
                  <label className='space-y-2'>
                    <span className='text-muted-foreground text-sm'>Provider</span>
                    <Select value={providerFilter} onValueChange={setProviderFilter}>
                      <SelectTrigger className='border-border text-foreground bg-background/60 h-11 w-full rounded-2xl'>
                        <SelectValue placeholder='All providers' />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value={ALL_FILTER}>All providers</SelectItem>
                        {providers.map((provider) => (
                          <SelectItem key={provider} value={provider}>
                            {provider}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </label>

                  <label className='space-y-2'>
                    <span className='text-muted-foreground text-sm'>Status</span>
                    <Select value={statusFilter} onValueChange={setStatusFilter}>
                      <SelectTrigger className='border-border text-foreground bg-background/60 h-11 w-full rounded-2xl'>
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
                    Sorted by latest update. Inline edit writes the auth-file{' '}
                    <span className='text-foreground'>prefix</span> field.
                  </p>
                </div>
              </section>

              <section className='space-y-4'>
                <div>
                  <h3 className='text-lg font-semibold'>Start OAuth</h3>
                  <p className='text-muted-foreground mt-1 text-sm'>
                    Launch a new auth flow when needed.
                  </p>
                </div>
                <div className='grid gap-3 lg:grid-cols-[0.9fr_1.1fr] xl:grid-cols-[0.9fr_1.1fr_auto]'>
                  <Select
                    value={oauthProvider}
                    onValueChange={(value) => setOauthProvider(value as OAuthProvider)}
                  >
                    <SelectTrigger className='border-border text-foreground bg-background/60 h-11 rounded-2xl'>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value='antigravity'>antigravity</SelectItem>
                      <SelectItem value='kiro-google'>kiro-google (unavailable)</SelectItem>
                      <SelectItem value='kiro-github'>kiro-github (unavailable)</SelectItem>
                    </SelectContent>
                  </Select>
                  <Input
                    type='text'
                    value={oauthLabel}
                    onChange={(event) => setOauthLabel(event.target.value)}
                    className='border-border text-foreground bg-background/60 h-11 rounded-2xl'
                    placeholder='Optional label'
                    maxLength={MAX_LABEL_LENGTH}
                  />
                  <Button
                    type='button'
                    onClick={() => {
                      if (oauthProvider !== 'antigravity') {
                        return
                      }
                      const label = oauthLabel.trim()
                      startOAuth.mutate(
                        {
                          provider: oauthProvider,
                          label: label.length > 0 ? label : undefined,
                        },
                        {
                          onSuccess: (data) => {
                            setOauthState(data.state)
                            window.open(data.url, '_blank', 'noopener,noreferrer')
                          },
                        },
                      )
                    }}
                    disabled={startOAuth.isPending || oauthProvider !== 'antigravity'}
                    className='h-11 rounded-xl px-4 xl:self-end'
                  >
                    {oauthProvider === 'antigravity'
                      ? startOAuth.isPending
                        ? 'Starting…'
                        : 'Launch OAuth'
                      : 'Unavailable'}
                  </Button>
                </div>
                <div className='text-muted-foreground text-sm leading-6'>
                  {oauthState ? (
                    <>
                      <p>
                        Current state:{' '}
                        <span className='text-foreground break-all'>{oauthState}</span>
                      </p>
                      <p className='mt-2'>
                        Status: <span className='text-foreground'>{oauthStatusSummary}</span>
                      </p>
                      {oauthStatusData?.error ? (
                        <p className='border-destructive/30 bg-destructive/10 text-destructive mt-3 rounded-2xl border px-4 py-3 dark:text-destructive-foreground'>
                          {oauthStatusData.error}
                        </p>
                      ) : null}
                    </>
                  ) : (
                    <p>No OAuth flow started yet.</p>
                  )}
                </div>
                {oauthProvider !== 'antigravity' ? (
                  <p className='border-border bg-muted/50 text-muted-foreground rounded-2xl border px-4 py-3 text-sm'>
                    Kiro social login is not supported for third-party apps in CLIProxyAPIPlus web flow.
                    Use Builder ID / IDC outside this dashboard, or import an existing Kiro token file.
                  </p>
                ) : null}
              </section>
              {mutationError ? (
                <p className='border-destructive/30 bg-destructive/10 text-destructive rounded-2xl border px-4 py-3 text-sm xl:col-span-2 dark:text-destructive-foreground'>
                  {mutationError.message}
                </p>
              ) : null}
            </div>

            <section className='space-y-4'>
              <div>
                <h3 className='text-lg font-semibold'>Upload auth file</h3>
                <p className='text-muted-foreground mt-1 text-sm'>
                  Paste an auth record or load a local JSON file to add it to the inventory.
                </p>
              </div>
              <div className='grid gap-4'>
                <Input
                  type='file'
                  accept='.json,application/json'
                  onChange={async (event) => {
                    const file = event.target.files?.[0]
                    event.currentTarget.value = ''

                    if (!file) {
                      return
                    }

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
                  className='border-border text-foreground bg-background/60 h-11 rounded-2xl file:mr-4 file:border-0 file:bg-transparent file:text-sm file:font-medium'
                />
                {uploadFileError ? (
                  <p className='border-destructive/30 bg-destructive/10 text-destructive rounded-2xl border px-4 py-3 text-sm dark:text-destructive-foreground'>
                    {uploadFileError}
                  </p>
                ) : null}
                <Input
                  type='text'
                  value={uploadName}
                  onChange={(event) => setUploadName(event.target.value)}
                  className='border-border text-foreground bg-background/60 h-11 rounded-2xl'
                  placeholder='antigravity-user.json'
                  maxLength={MAX_UPLOAD_NAME_LENGTH}
                />
                <Textarea
                  value={uploadBody}
                  onChange={(event) => setUploadBody(event.target.value)}
                  className='border-border text-foreground bg-background/60 min-h-40 rounded-2xl px-4 py-3'
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

            {!hasItems ? (
              <article className='border-border bg-muted/40 text-muted-foreground rounded-3xl border border-dashed p-6 text-sm'>
                <p className='text-foreground'>No auth records match the current filters.</p>
                <p className='mt-2'>
                  Change the filters, start OAuth, or upload an auth file to add another account.
                </p>
              </article>
            ) : null}

            <div className='grid gap-3 xl:hidden'>
              {items.map((item) => (
                <Card key={item.id} className='border-border bg-card rounded-2xl border'>
                  <CardContent className='space-y-4 p-4'>
                    <div className='flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between'>
                      <div className='min-w-0'>
                        <p className='text-foreground font-medium'>{item.label || item.id}</p>
                        <p className='text-muted-foreground mt-1 truncate text-sm'>
                          {item.provider}
                        </p>
                        <p className='text-muted-foreground mt-2 truncate text-sm'>
                          {item.email ?? item.project_id ?? '—'}
                        </p>
                        <p className='text-muted-foreground mt-1 truncate text-xs'>{item.path}</p>
                      </div>
                      <div className='flex flex-wrap items-center gap-2'>
                        <Badge
                          variant='outline'
                          className={`rounded-full px-2.5 py-1 text-xs ${statusTone(item.status)}`}
                        >
                          {item.status}
                        </Badge>
                        <span className='text-muted-foreground text-xs'>
                          {new Date(item.updated_at).toLocaleString()}
                        </span>
                      </div>
                    </div>

                    {item.status_message ? (
                      <p className='text-destructive text-xs dark:text-destructive-foreground'>
                        {item.status_message}
                      </p>
                    ) : null}

                    {editName === item.id ? (
                      <div className='border-border bg-background/50 space-y-3 rounded-2xl border p-4'>
                        <Input
                          type='text'
                          value={editLabel}
                          onChange={(event) => setEditLabel(event.target.value)}
                          className='border-border text-foreground bg-background/60 h-11 rounded-xl'
                          placeholder='prefix value'
                        />
                        <div className='flex flex-col gap-2 sm:flex-row'>
                          <Button
                            type='button'
                            onClick={() =>
                              patchFields.mutate({ name: item.id, label: editLabel.trim() })
                            }
                            disabled={patchFields.isPending || editLabel.trim().length === 0}
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

                    <div className='grid gap-2 sm:grid-cols-2'>
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
                        Edit
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

            <div className='hidden space-y-2 xl:block'>
              {items.map((item) => (
                <div key={item.id} className='border-border rounded-2xl border p-4'>
                  <div className='grid gap-4 xl:grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)_auto]'>
                    <div className='min-w-0'>
                      <div className='flex flex-wrap items-center gap-2'>
                        <p className='text-foreground font-medium'>{item.label || item.id}</p>
                        <Badge
                          variant='outline'
                          className={`rounded-full px-2.5 py-1 text-xs ${statusTone(item.status)}`}
                        >
                          {item.status}
                        </Badge>
                      </div>
                      <p className='text-muted-foreground mt-1 truncate text-sm'>{item.provider}</p>
                      <p className='text-muted-foreground mt-2 truncate text-sm'>
                        {item.email ?? item.project_id ?? '—'}
                      </p>
                      <p className='text-muted-foreground mt-1 truncate text-xs'>{item.path}</p>
                      {item.status_message ? (
                        <p className='text-destructive mt-2 text-xs dark:text-destructive-foreground'>
                          {item.status_message}
                        </p>
                      ) : null}
                      {editName === item.id ? (
                        <div className='mt-4 flex gap-2'>
                          <Input
                            type='text'
                            value={editLabel}
                            onChange={(event) => setEditLabel(event.target.value)}
                            className='border-border text-foreground bg-background/60 h-10 rounded-xl'
                            placeholder='prefix value'
                          />
                          <Button
                            type='button'
                            onClick={() =>
                              patchFields.mutate({ name: item.id, label: editLabel.trim() })
                            }
                            disabled={patchFields.isPending || editLabel.trim().length === 0}
                            className='h-10 rounded-xl px-3'
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
                            className='h-10 rounded-xl px-3'
                          >
                            Cancel
                          </Button>
                        </div>
                      ) : null}
                    </div>

                    <div className='text-muted-foreground text-sm'>
                      <p>{new Date(item.updated_at).toLocaleString()}</p>
                      <p className='mt-1 text-xs'>
                        Refreshed:{' '}
                        {item.last_refreshed_at
                          ? new Date(item.last_refreshed_at).toLocaleString()
                          : 'never'}
                      </p>
                    </div>

                    <div className='grid gap-2 sm:grid-cols-2 xl:w-[280px]'>
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
                        className='h-10 rounded-xl px-3'
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
                        className='h-10 rounded-xl px-3'
                      >
                        Edit
                      </Button>
                      <Button
                        type='button'
                        variant='outline'
                        onClick={() => downloadAuthFile(item.id)}
                        className='h-10 rounded-xl px-3'
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
                        className='h-10 rounded-xl px-3'
                      >
                        Delete
                      </Button>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          </div>
        ) : null}
      </QueryState>
    </PageShell>
  )
}
