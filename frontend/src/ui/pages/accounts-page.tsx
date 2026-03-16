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

  const oauthStatus = useOAuthStatusQuery(oauthState, Boolean(oauthState))
  const oauthStatusData = oauthStatus.data

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
            <div className='grid gap-4 md:grid-cols-2 xl:grid-cols-4'>
              <Card className='rounded-2xl border border-[var(--border)] bg-[var(--card)]'>
                <CardContent className='p-5'>
                  <p className='text-sm text-[var(--muted-foreground)]'>Total records</p>
                  <p className='mt-3 text-3xl font-semibold text-white'>{accounts.data.total}</p>
                </CardContent>
              </Card>
              <Card className='rounded-2xl border border-[var(--border)] bg-[var(--card)] md:col-span-3'>
                <CardContent className='p-5'>
                  <div className='flex flex-col gap-3 md:flex-row md:items-start md:justify-between'>
                    <div>
                      <p className='text-sm text-[var(--muted-foreground)]'>Providers</p>
                      <div className='mt-3 flex flex-wrap gap-2'>
                        {accounts.data.grouped_counts.map((summary) => (
                          <Badge
                            key={summary.provider}
                            variant='outline'
                            className='rounded-full border-[var(--border)] bg-white/5 px-3 py-1 text-sm text-white'
                          >
                            {summary.provider} · {summary.total}
                          </Badge>
                        ))}
                      </div>
                    </div>
                    <Badge
                      variant='outline'
                      className='rounded-full px-3 py-1 text-xs text-[var(--muted-foreground)]'
                    >
                      {hasItems ? `${items.length} visible` : 'No matches'}
                    </Badge>
                  </div>
                </CardContent>
              </Card>
            </div>

            <Card className='rounded-3xl border border-[var(--border)] bg-[var(--card)]'>
              <CardContent className='p-6'>
                <div className='grid gap-6 xl:grid-cols-2'>
                  <div className='space-y-4'>
                    <h3 className='text-lg font-semibold'>Filters</h3>
                    <div className='grid gap-4 md:grid-cols-[0.8fr_0.8fr_1.4fr]'>
                      <label className='space-y-2'>
                        <span className='text-sm text-[var(--muted-foreground)]'>
                          Provider filter
                        </span>
                        <Select value={providerFilter} onValueChange={setProviderFilter}>
                          <SelectTrigger className='h-11 w-full rounded-2xl border-[var(--border)] bg-black/20 text-white'>
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
                        <span className='text-sm text-[var(--muted-foreground)]'>
                          Status filter
                        </span>
                        <Select value={statusFilter} onValueChange={setStatusFilter}>
                          <SelectTrigger className='h-11 w-full rounded-2xl border-[var(--border)] bg-black/20 text-white'>
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

                      <div className='rounded-2xl border border-[var(--border)] bg-black/10 p-4 text-sm text-[var(--muted-foreground)]'>
                        Sorted by <span className='text-white'>latest update</span>. Inline edit
                        currently writes the auth-file <span className='text-white'>prefix</span>{' '}
                        field because the backend has no mutable label yet.
                      </div>
                    </div>
                  </div>

                  <div className='space-y-4'>
                    <h3 className='text-lg font-semibold'>Start OAuth</h3>
                    <div className='grid gap-4 md:grid-cols-[0.9fr_1.1fr_auto]'>
                      <Select
                        value={oauthProvider}
                        onValueChange={(value) => setOauthProvider(value as OAuthProvider)}
                      >
                        <SelectTrigger className='h-11 rounded-2xl border-[var(--border)] bg-black/20 text-white'>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value='antigravity'>antigravity</SelectItem>
                          <SelectItem value='kiro-google'>kiro-google</SelectItem>
                          <SelectItem value='kiro-github'>kiro-github</SelectItem>
                        </SelectContent>
                      </Select>
                      <Input
                        type='text'
                        value={oauthLabel}
                        onChange={(event) => setOauthLabel(event.target.value)}
                        className='h-11 rounded-2xl border-[var(--border)] bg-black/20 text-white'
                        placeholder='Optional label'
                      />
                      <Button
                        type='button'
                        onClick={() => {
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
                        disabled={startOAuth.isPending}
                        className='h-11 rounded-xl px-4'
                      >
                        {startOAuth.isPending ? 'Starting…' : 'Launch OAuth'}
                      </Button>
                    </div>
                    <div className='rounded-2xl border border-[var(--border)] bg-black/10 p-4 text-sm text-[var(--muted-foreground)]'>
                      {oauthState ? (
                        <>
                          <p>
                            Current state:{' '}
                            <span className='break-all text-white'>{oauthState}</span>
                          </p>
                          <p className='mt-2'>
                            Status:{' '}
                            <span className='text-white'>
                              {oauthStatusData?.status ??
                                (oauthStatus.isFetching ? 'wait' : 'idle')}
                            </span>
                          </p>
                          {oauthStatusData?.error ? (
                            <p className='mt-3 rounded-2xl border border-red-500/30 bg-red-500/10 px-4 py-3 text-red-100'>
                              {oauthStatusData.error}
                            </p>
                          ) : null}
                        </>
                      ) : (
                        <p>
                          No OAuth flow started yet. Launch one to open the provider consent screen.
                        </p>
                      )}
                    </div>
                  </div>
                </div>
                {mutationError ? (
                  <p className='mt-4 rounded-2xl border border-red-500/30 bg-red-500/10 px-4 py-3 text-sm text-red-100'>
                    {mutationError.message}
                  </p>
                ) : null}
              </CardContent>
            </Card>

            <Card className='rounded-3xl border border-[var(--border)] bg-[var(--card)]'>
              <CardContent className='p-6'>
                <h3 className='text-lg font-semibold'>Upload auth file</h3>
                <div className='mt-4 grid gap-4'>
                  <Input
                    type='text'
                    value={uploadName}
                    onChange={(event) => setUploadName(event.target.value)}
                    className='h-11 rounded-2xl border-[var(--border)] bg-black/20 text-white'
                    placeholder='antigravity-user.json'
                  />
                  <Textarea
                    value={uploadBody}
                    onChange={(event) => setUploadBody(event.target.value)}
                    className='min-h-40 rounded-2xl border-[var(--border)] bg-black/20 px-4 py-3 text-white'
                    placeholder='{"type":"antigravity"}'
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
              </CardContent>
            </Card>

            {!hasItems ? (
              <article className='rounded-3xl border border-dashed border-[var(--border)] bg-black/10 p-6 text-sm text-[var(--muted-foreground)]'>
                <p className='text-white'>No auth records match the current filters.</p>
                <p className='mt-2'>
                  Change the filters, start OAuth, or upload an auth file to add another account.
                </p>
              </article>
            ) : null}

            <div className='space-y-3 md:hidden'>
              {items.map((item) => (
                <Card
                  key={item.id}
                  className='rounded-2xl border border-[var(--border)] bg-[var(--card)]'
                >
                  <CardContent className='p-4'>
                    <div className='min-w-0'>
                      <p className='font-medium text-white'>{item.label || item.id}</p>
                      <p className='truncate text-sm text-[var(--muted-foreground)]'>
                        {item.provider}
                      </p>
                      <p className='mt-2 truncate text-sm text-[var(--muted-foreground)]'>
                        {item.email ?? item.project_id ?? '—'}
                      </p>
                      <p className='mt-1 truncate text-xs text-[var(--muted-foreground)]'>
                        {item.path}
                      </p>
                    </div>
                    <div className='mt-3 flex flex-wrap items-center gap-2'>
                      <Badge
                        variant='outline'
                        className={`rounded-full px-2.5 py-1 text-xs ${statusTone(item.status)}`}
                      >
                        {item.status}
                      </Badge>
                      <span className='text-xs text-[var(--muted-foreground)]'>
                        {new Date(item.updated_at).toLocaleString()}
                      </span>
                    </div>
                    {item.status_message ? (
                      <p className='mt-2 text-xs text-red-200'>{item.status_message}</p>
                    ) : null}
                    {editName === item.id ? (
                      <div className='mt-3 flex flex-col gap-2'>
                        <Input
                          type='text'
                          value={editLabel}
                          onChange={(event) => setEditLabel(event.target.value)}
                          className='h-11 rounded-xl border-[var(--border)] bg-black/20 text-white'
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
                            className='h-11 rounded-xl px-3 text-white'
                          >
                            Cancel
                          </Button>
                        </div>
                      </div>
                    ) : null}
                    <div className='mt-3 flex flex-col gap-2 sm:flex-row sm:flex-wrap'>
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
                        className='h-11 rounded-xl px-3 text-white'
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
                        className='h-11 rounded-xl px-3 text-white'
                      >
                        Edit
                      </Button>
                      <Button
                        type='button'
                        variant='outline'
                        onClick={() => downloadAuthFile(item.id)}
                        className='h-11 rounded-xl px-3 text-white'
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

            <div className='hidden overflow-hidden rounded-3xl border border-[var(--border)] bg-[var(--card)] md:block'>
              <div className='grid grid-cols-[1fr_1fr_0.7fr_1fr_1.2fr] gap-3 border-b border-[var(--border)] bg-white/5 px-4 py-3 text-xs tracking-[0.2em] text-[var(--muted-foreground)] uppercase'>
                <span>Provider / label</span>
                <span>Identity</span>
                <span>Status</span>
                <span>Updated</span>
                <span>Actions</span>
              </div>

              {items.map((item) => (
                <div
                  key={item.id}
                  className='grid grid-cols-[1fr_1fr_0.7fr_1fr_1.2fr] gap-3 border-t border-[var(--border)] px-4 py-4 text-sm'
                >
                  <div className='min-w-0'>
                    <p className='font-medium text-white'>{item.label || item.id}</p>
                    <p className='truncate text-[var(--muted-foreground)]'>{item.provider}</p>
                    {editName === item.id ? (
                      <div className='mt-3 flex flex-col gap-2'>
                        <Input
                          type='text'
                          value={editLabel}
                          onChange={(event) => setEditLabel(event.target.value)}
                          className='h-10 rounded-xl border-[var(--border)] bg-black/20 text-white'
                          placeholder='prefix value'
                        />
                        <div className='flex gap-2'>
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
                            className='h-10 rounded-xl px-3 text-white'
                          >
                            Cancel
                          </Button>
                        </div>
                      </div>
                    ) : null}
                  </div>
                  <div className='min-w-0 text-[var(--muted-foreground)]'>
                    <p className='truncate'>{item.email ?? item.project_id ?? '—'}</p>
                    <p className='truncate text-xs'>{item.path}</p>
                  </div>
                  <div>
                    <Badge
                      variant='outline'
                      className={`rounded-full px-2.5 py-1 text-xs ${statusTone(item.status)}`}
                    >
                      {item.status}
                    </Badge>
                    {item.status_message ? (
                      <p className='mt-2 text-xs text-red-200'>{item.status_message}</p>
                    ) : null}
                  </div>
                  <div className='text-[var(--muted-foreground)]'>
                    <p>{new Date(item.updated_at).toLocaleString()}</p>
                    <p className='mt-1 text-xs'>
                      Refreshed:{' '}
                      {item.last_refreshed_at
                        ? new Date(item.last_refreshed_at).toLocaleString()
                        : 'never'}
                    </p>
                  </div>
                  <div className='flex flex-wrap gap-2'>
                    <Button
                      type='button'
                      variant='outline'
                      onClick={() =>
                        toggleStatus.mutate({ name: item.id, disabled: item.status !== 'disabled' })
                      }
                      disabled={toggleStatus.isPending}
                      className='h-10 rounded-xl px-3 text-white'
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
                      className='h-10 rounded-xl px-3 text-white'
                    >
                      Edit
                    </Button>
                    <Button
                      type='button'
                      variant='outline'
                      onClick={() => downloadAuthFile(item.id)}
                      className='h-10 rounded-xl px-3 text-white'
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
              ))}
            </div>
          </div>
        ) : null}
      </QueryState>
    </PageShell>
  )
}
