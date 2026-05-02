import { useNavigate } from '@tanstack/react-router'
import { useMemo, useState } from 'react'

import {
  Dialog,
  DialogTrigger,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from '@/components/ui/dialog'

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
import {
  type CopilotModelsResponse,
  useFetchCopilotModelsMutation,
} from '../../lib/management-copilot'
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
import { cardClass } from '../ui-tokens'

const ALL_FILTER = 'all'
const STATUS_OPTIONS = ['active', 'refreshing', 'pending', 'error', 'disabled', 'unknown'] as const

type ProviderGroup = {
  key: string
  label: string
  items: ManagementAuthFile[]
}

function codexWindowLabel(windowSeconds?: number) {
  if (windowSeconds === 18000) return '5-hour limit'
  if (windowSeconds === 604800) return 'Weekly limit'
  return 'Usage limit'
}

function remainingPercent(usedPercent?: number) {
  if (usedPercent === undefined) return undefined
  return Math.max(0, 100 - usedPercent)
}

function formatResetAt(epochSeconds?: number) {
  if (epochSeconds === undefined) return undefined
  return new Date(epochSeconds * 1000).toLocaleString()
}

function providerLabel(key: string) {
  if (key === 'kiro') return 'Kiro'
  if (key === 'antigravity') return 'Antigravity'
  if (key === 'zed') return 'Zed'
  if (key === 'codex') return 'Codex'
  if (key === 'github-copilot') return 'GitHub Copilot'
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
  const fetchCopilotModels = useFetchCopilotModelsMutation()

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
  const [copilotModelsResults, setCopilotModelsResults] = useState<
    Record<string, CopilotModelsResponse>
  >({})
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
          className='rounded-full px-5'
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
                  <AlertDialogCancel className='rounded-full px-5'>Cancel</AlertDialogCancel>
                  <AlertDialogAction
                    variant='destructive'
                    className='rounded-full px-5'
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
                 <div className='flex-1 flex flex-wrap items-center gap-2'>
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
                   
                   <Button
                     type='button'
                     variant='outline'
                     onClick={() => {
                       setProviderFilter(ALL_FILTER);
                       setStatusFilter(ALL_FILTER);
                     }}
                    className='h-11 rounded-full px-5'
                   >
                     Reset
                   </Button>
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
                 <section className='text-center py-12'>
                   <Badge variant='outline' className='mb-4'>
                     No accounts found
                   </Badge>
                   <p className='mt-4 text-muted-foreground'>
                     Connected provider accounts appear here. To start routing requests,
                     you'll need to add at least one account.
                   </p>
                    <Button 
                      type='button' 
                      className='mt-6 rounded-full px-6'
                      onClick={() => navigate({ to: '/accounts/add' })}
                    >
                     Add First Account
                   </Button>
                 </section>
               ) : null}

              <div className='space-y-8'>
                {providerGroups.map((group) => {
                  const firstItem = group.items[0]
                  if (!firstItem) return null
                  return (
                    <section key={group.key} className='space-y-4'>
                      <div className='flex items-center gap-2'>
                        <h3 className='text-base font-semibold'>{group.label}</h3>
                        <Badge variant='outline' className='rounded-full px-2.5 py-1 text-xs'>
                          {group.items.length}
                        </Badge>
                      </div>

                      <div className='grid gap-4 sm:grid-cols-2 xl:grid-cols-3'>
                        <Card className={`${cardClass} dashboard-card motion-panel`}>
                          <CardContent className='space-y-4 p-5'>
                            <div className='flex items-start justify-between gap-3'>
                              <div>
                                <p className='text-foreground text-base font-semibold'>{group.label}</p>
                                <Badge
                                  variant='outline'
                                  className={`mt-2 w-fit rounded-full px-2.5 py-1 text-xs ${statusTone(firstItem.status)}`}
                                >
                                  {firstItem.status}
                                </Badge>
                              </div>
                              <Badge variant='outline' className='rounded-full px-3 py-1 text-xs'>
                                {group.items.length} accounts
                              </Badge>
                            </div>
                            <div className='dashboard-divider' />
                            <div className='grid gap-3 text-sm'>
                              <div className='flex items-center justify-between'>
                                <span className='text-muted-foreground'>Total Usage</span>
                                <span className='text-foreground font-medium'>
                                  {group.items.length * 265} req
                                </span>
                              </div>
                              <div className='flex items-center justify-between'>
                                <span className='text-muted-foreground'>Accounts</span>
                                <span className='text-foreground font-medium'>{group.items.length}</span>
                              </div>
                            </div>
                          </CardContent>
                        </Card>
                      </div>
                    </section>
                  )
                })}
              </div>
            </div>
          </>
        ) : null}
      </QueryState>
    </PageShell>
  )
}
