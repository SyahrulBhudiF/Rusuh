import { useNavigate } from '@tanstack/react-router'
import { useMemo, useState } from 'react'

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
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

import {
  downloadAuthFile,
  type ManagementAuthFile,
  useDeleteAuthFileMutation,
  useManagementAuthFilesQuery,
  usePatchAuthFileFieldsMutation,
  useToggleAuthFileStatusMutation,
} from '../../lib/management-auth-files'
import { type CodexQuotaResponse, useCheckCodexQuotaMutation } from '../../lib/management-codex'
import { useCheckKiroQuotaMutation } from '../../lib/management-kiro'
import {
  type ZedModelsResponse,
  type ZedQuotaResponse,
  useCheckZedQuotaMutation,
  useFetchZedModelsMutation,
} from '../../lib/management-zed'
import { toastError, toastInfo, toastSuccess } from '../../lib/toast'
import { PageShell } from '../page-shell'
import { QueryState } from '../query-state'
import { statusTone } from '../status-tone'
import { cardClass, surfaceClass } from '../ui-tokens'

const ALL_FILTER = 'all'
const STATUS_OPTIONS = ['active', 'refreshing', 'pending', 'error', 'disabled', 'unknown'] as const

type ProviderGroup = {
  key: string
  label: string
  items: ManagementAuthFile[]
}

function providerLabel(key: string) {
  if (key === 'kiro') return 'Kiro'
  if (key === 'antigravity') return 'Antigravity'
  if (key === 'zed') return 'Zed'
  if (key === 'codex') return 'Codex'
  return key
}

function accountSubtitle(item: ManagementAuthFile) {
  return item.email || item.project_id || item.provider || item.type || '—'
}

function renderKiroMetadata(item: ManagementAuthFile) {
  return (
    <div className='mt-2 flex flex-wrap gap-2 text-xs'>
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
    </div>
  )
}

export function AccountsPage() {
  const navigate = useNavigate()
  const accounts = useManagementAuthFilesQuery()
  const toggleStatus = useToggleAuthFileStatusMutation()
  const patchFields = usePatchAuthFileFieldsMutation()
  const deleteAuthFile = useDeleteAuthFileMutation()
  const checkKiroQuota = useCheckKiroQuotaMutation()
  const checkCodexQuota = useCheckCodexQuotaMutation()
  const checkZedQuota = useCheckZedQuotaMutation()
  const fetchZedModels = useFetchZedModelsMutation()

  const [providerFilter, setProviderFilter] = useState(ALL_FILTER)
  const [statusFilter, setStatusFilter] = useState(ALL_FILTER)
  const [editName, setEditName] = useState<string | null>(null)
  const [editLabel, setEditLabel] = useState('')
  const [quotaResults, setQuotaResults] = useState<
    Record<string, { status: string; remaining?: number; detail?: string; message?: string }>
  >({})
  const [codexQuotaResults, setCodexQuotaResults] = useState<Record<string, CodexQuotaResponse>>({})
  const [zedQuotaResults, setZedQuotaResults] = useState<Record<string, ZedQuotaResponse>>({})
  const [zedModelsResults, setZedModelsResults] = useState<Record<string, ZedModelsResponse>>({})
  const [deleteTarget, setDeleteTarget] = useState<ManagementAuthFile | null>(null)

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

  return (
    <PageShell
      eyebrow='Accounts'
      title='Manage connected accounts'
      description='Review status, update labels, and handle provider account maintenance.'
      actions={
        <Button
          type='button'
          className='rounded-xl'
          onClick={() => navigate({ to: '/accounts/add' })}
        >
          Add Account
        </Button>
      }
    >
      <QueryState
        isLoading={accounts.isLoading}
        isError={accounts.isError}
        error={accounts.error as Error | null}
      >
        {accounts.data ? (
          <>
            <AlertDialog
              open={deleteTarget !== null}
              onOpenChange={(open) => {
                if (!open) {
                  setDeleteTarget(null)
                }
              }}
            >
              <AlertDialogContent className='rounded-3xl'>
                <AlertDialogHeader>
                  <AlertDialogTitle>Delete account?</AlertDialogTitle>
                  <AlertDialogDescription>
                    {deleteTarget
                      ? `This removes ${deleteTarget.id}. This action cannot be undone.`
                      : 'This action cannot be undone.'}
                  </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel className='rounded-xl'>Cancel</AlertDialogCancel>
                  <AlertDialogAction
                    variant='destructive'
                    className='rounded-xl'
                    onClick={() => {
                      if (!deleteTarget) return
                      const target = deleteTarget
                      deleteAuthFile.mutate(target.id, {
                        onSuccess: () => {
                          toastSuccess('Account deleted', target.id)
                          setDeleteTarget(null)
                        },
                        onError: (error) => {
                          toastError('Failed to delete account', error.message)
                          setDeleteTarget(null)
                        },
                      })
                    }}
                  >
                    Delete
                  </AlertDialogAction>
                </AlertDialogFooter>
              </AlertDialogContent>
            </AlertDialog>

            <div className='space-y-6'>
              <section className='flex flex-col gap-4 lg:flex-row lg:items-end lg:justify-between'>
                <div className='grid gap-4 md:grid-cols-2 xl:grid-cols-[220px_220px]'>
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
                </div>

                <div className='flex flex-wrap items-center gap-2'>
                  {providerGroups.map((group) => (
                    <Badge
                      key={group.key}
                      variant='outline'
                      className='rounded-full px-3 py-1 text-sm'
                    >
                      {group.label} · {group.items.length}
                    </Badge>
                  ))}
                  <Badge variant='outline' className='rounded-full px-3 py-1 text-xs'>
                    {hasItems ? `${items.length} visible` : 'No matches'}
                  </Badge>
                </div>
              </section>

              {!hasItems ? (
                <article
                  className={`${surfaceClass} bg-muted/40 text-muted-foreground border-dashed p-6 text-sm`}
                >
                  <p className='text-foreground'>No accounts to show.</p>
                  <p className='mt-2'>
                    Connected provider accounts appear here. Add one account to start routing
                    requests.
                  </p>
                  <Button
                    type='button'
                    className='mt-4 rounded-xl'
                    onClick={() => navigate({ to: '/accounts/add' })}
                  >
                    Add Account
                  </Button>
                </article>
              ) : null}

              <div className='space-y-8'>
                {providerGroups.map((group) => (
                  <section key={group.key} className='space-y-4'>
                    <div className='flex items-center gap-2'>
                      <h3 className='text-base font-semibold'>{group.label}</h3>
                      <Badge variant='outline' className='rounded-full px-2.5 py-1 text-xs'>
                        {group.items.length}
                      </Badge>
                    </div>

                    <div className='grid gap-3'>
                      {group.items.map((item) => (
                        <Card key={item.id} className={`${cardClass} dashboard-card motion-panel`}>
                          <CardContent className='space-y-4 p-4'>
                            <div className='flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between'>
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
                                <p className='text-muted-foreground mt-1 text-sm'>
                                  {accountSubtitle(item)}
                                </p>
                                <p className='text-muted-foreground mt-1 truncate text-xs'>
                                  {item.id}
                                </p>
                                {item.provider_key === 'kiro' ? renderKiroMetadata(item) : null}
                                {item.status_message ? (
                                  <p className='text-destructive mt-2 text-xs'>
                                    {item.status_message}
                                  </p>
                                ) : null}
                                {item.provider_key === 'kiro' && quotaResults[item.id] ? (
                                  <div className='mt-3 flex flex-wrap items-center gap-2 text-xs'>
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
                                    {quotaResults[item.id].remaining !== undefined ? (
                                      <span className='text-muted-foreground'>
                                        {quotaResults[item.id].remaining} remaining
                                      </span>
                                    ) : null}
                                    {quotaResults[item.id].detail ? (
                                      <span className='text-muted-foreground'>
                                        {quotaResults[item.id].detail}
                                      </span>
                                    ) : null}
                                    {quotaResults[item.id].message ? (
                                      <span className='text-muted-foreground'>
                                        {quotaResults[item.id].message}
                                      </span>
                                    ) : null}
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
                                      <p className='text-destructive mt-1 text-xs'>
                                        {zedQuotaResults[item.id].error}
                                      </p>
                                    ) : null}
                                  </div>
                                ) : null}
                                {item.provider_key === 'zed' && zedModelsResults[item.id] ? (
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
                                {item.provider_key === 'codex' && codexQuotaResults[item.id] ? (
                                  <div className='border-border bg-muted/50 mt-3 rounded-lg border p-3'>
                                    <p className='text-xs font-medium'>Quota Status:</p>
                                    <div className='mt-1 flex items-center gap-2'>
                                      <Badge
                                        variant={
                                          codexQuotaResults[item.id].status === 'available'
                                            ? 'default'
                                            : 'destructive'
                                        }
                                        className='rounded-full'
                                      >
                                        {codexQuotaResults[item.id].status}
                                      </Badge>
                                      {codexQuotaResults[item.id].retry_after_seconds !==
                                      undefined ? (
                                        <span className='text-muted-foreground text-xs'>
                                          Retry after{' '}
                                          {codexQuotaResults[item.id].retry_after_seconds}s
                                        </span>
                                      ) : null}
                                    </div>
                                    {codexQuotaResults[item.id].plan_type ? (
                                      <p className='text-muted-foreground mt-1 text-xs'>
                                        Plan: {codexQuotaResults[item.id].plan_type}
                                      </p>
                                    ) : null}
                                    {codexQuotaResults[item.id].detail ? (
                                      <p className='text-muted-foreground mt-1 text-xs'>
                                        {codexQuotaResults[item.id].detail}
                                      </p>
                                    ) : null}
                                  </div>
                                ) : null}
                              </div>

                              <div className='text-muted-foreground text-sm xl:text-right'>
                                <p>{new Date(item.updated_at).toLocaleString()}</p>
                                <p className='mt-1 text-xs'>
                                  Refreshed{' '}
                                  {item.last_refreshed_at
                                    ? new Date(item.last_refreshed_at).toLocaleString()
                                    : 'never'}
                                </p>
                              </div>
                            </div>

                            {editName === item.id ? (
                              <div className='space-y-3 rounded-2xl border p-4'>
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
                                      patchFields.mutate(
                                        {
                                          name: item.id,
                                          label: editLabel.trim(),
                                        },
                                        {
                                          onSuccess: () => {
                                            setEditName(null)
                                            setEditLabel('')
                                            toastSuccess('Account label updated')
                                          },
                                          onError: (error) => {
                                            toastError(
                                              'Failed to update account label',
                                              error.message,
                                            )
                                          },
                                        },
                                      )
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

                            <div className='flex flex-wrap gap-2'>
                              {item.provider_key === 'zed' ? (
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
                                      toastInfo('Models fetched', item.id)
                                    } catch (error) {
                                      toastError(
                                        'Failed to fetch models',
                                        error instanceof Error ? error.message : 'Unknown error.',
                                      )
                                    }
                                  }}
                                  disabled={fetchZedModels.isPending}
                                  className='h-10 rounded-xl px-3'
                                >
                                  {fetchZedModels.isPending ? 'Loading…' : 'Get models'}
                                </Button>
                              ) : null}
                              {item.provider_key === 'codex' ? (
                                <Button
                                  type='button'
                                  variant='outline'
                                  onClick={async () => {
                                    try {
                                      const result = await checkCodexQuota.mutateAsync({
                                        name: item.id,
                                      })
                                      setCodexQuotaResults((prev) => ({
                                        ...prev,
                                        [item.id]: result,
                                      }))
                                      toastInfo('Quota check complete', item.id)
                                    } catch (error) {
                                      toastError(
                                        'Quota check failed',
                                        error instanceof Error ? error.message : 'Unknown error.',
                                      )
                                    }
                                  }}
                                  disabled={checkCodexQuota.isPending}
                                  className='h-10 rounded-xl px-3'
                                >
                                  {checkCodexQuota.isPending ? 'Checking…' : 'Check quota'}
                                </Button>
                              ) : null}
                              {item.provider_key === 'kiro' ? (
                                <Button
                                  type='button'
                                  variant='outline'
                                  onClick={async () => {
                                    try {
                                      const result = await checkKiroQuota.mutateAsync({
                                        name: item.id,
                                      })
                                      setQuotaResults((prev) => ({
                                        ...prev,
                                        [item.id]: result,
                                      }))
                                      toastInfo(
                                        'Quota check complete',
                                        `${item.id}: ${result.status}`,
                                      )
                                    } catch (error) {
                                      toastError(
                                        'Quota check failed',
                                        error instanceof Error ? error.message : 'Unknown error.',
                                      )
                                    }
                                  }}
                                  disabled={checkKiroQuota.isPending}
                                  className='h-10 rounded-xl px-3'
                                >
                                  {checkKiroQuota.isPending ? 'Checking…' : 'Check quota'}
                                </Button>
                              ) : null}
                              {item.provider_key === 'zed' ? (
                                <Button
                                  type='button'
                                  variant='outline'
                                  onClick={async () => {
                                    try {
                                      const result = await checkZedQuota.mutateAsync({
                                        name: item.id,
                                      })
                                      setZedQuotaResults((prev) => ({
                                        ...prev,
                                        [item.id]: result,
                                      }))
                                      toastInfo('Quota check complete', item.id)
                                    } catch (error) {
                                      toastError(
                                        'Quota check failed',
                                        error instanceof Error ? error.message : 'Unknown error.',
                                      )
                                    }
                                  }}
                                  disabled={checkZedQuota.isPending}
                                  className='h-10 rounded-xl px-3'
                                >
                                  {checkZedQuota.isPending ? 'Checking…' : 'Check quota'}
                                </Button>
                              ) : null}

                              <Button
                                type='button'
                                variant='outline'
                                onClick={() => {
                                  setEditName(item.id)
                                  setEditLabel(item.label)
                                }}
                                className='h-10 rounded-xl px-3'
                              >
                                Edit label
                              </Button>

                              <Button
                                type='button'
                                variant='outline'
                                onClick={() =>
                                  toggleStatus.mutate(
                                    {
                                      name: item.id,
                                      disabled: item.status !== 'disabled',
                                    },
                                    {
                                      onSuccess: () => {
                                        toastSuccess(
                                          item.status === 'disabled'
                                            ? 'Account enabled'
                                            : 'Account disabled',
                                          item.id,
                                        )
                                      },
                                      onError: (error) => {
                                        toastError('Failed to update account status', error.message)
                                      },
                                    },
                                  )
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
                                  downloadAuthFile(item.id)
                                  toastInfo('Download started', item.id)
                                }}
                                className='h-10 rounded-xl px-3'
                              >
                                Download
                              </Button>

                              <Button
                                type='button'
                                variant='destructive'
                                onClick={() => setDeleteTarget(item)}
                                disabled={deleteAuthFile.isPending}
                                className='h-10 rounded-xl px-3'
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
            </div>
          </>
        ) : null}
      </QueryState>
    </PageShell>
  )
}
