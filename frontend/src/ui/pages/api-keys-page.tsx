import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'

import { managementRequest } from '../../lib/management-api'
import { useManagementAuth } from '../../lib/management-auth'
import { queryKeys, useApiKeysQuery } from '../../lib/query'
import { PageShell } from '../page-shell'
import { QueryState } from '../query-state'

type ApiKeysResponse = {
  'api-keys': string[]
  generated?: string[]
}

export function ApiKeysPage() {
  const queryClient = useQueryClient()
  const { secret } = useManagementAuth()
  const apiKeys = useApiKeysQuery()
  const [newKeyValue, setNewKeyValue] = useState('')
  const [replaceValue, setReplaceValue] = useState('')
  const [replaceIndex, setReplaceIndex] = useState<number | null>(null)

  const refresh = () => queryClient.invalidateQueries({ queryKey: queryKeys.apiKeys })

  const generateKey = useMutation({
    mutationFn: () =>
      managementRequest<ApiKeysResponse>('/v0/management/api-keys', secret, {
        method: 'PATCH',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({ generate: true }),
      }),
    onSuccess: () => {
      void refresh()
    },
  })

  const appendKey = useMutation({
    mutationFn: (value: string) =>
      managementRequest<ApiKeysResponse>('/v0/management/api-keys', secret, {
        method: 'PATCH',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({ value }),
      }),
    onSuccess: () => {
      setNewKeyValue('')
      void refresh()
    },
  })

  const replaceKey = useMutation({
    mutationFn: ({ index, value }: { index: number; value: string }) =>
      managementRequest<ApiKeysResponse>('/v0/management/api-keys', secret, {
        method: 'PATCH',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({ index, value }),
      }),
    onSuccess: () => {
      setReplaceIndex(null)
      setReplaceValue('')
      void refresh()
    },
  })

  const deleteKey = useMutation({
    mutationFn: (index: number) =>
      managementRequest<ApiKeysResponse>(`/v0/management/api-keys?index=${index}`, secret, {
        method: 'DELETE',
      }),
    onSuccess: () => {
      void refresh()
    },
  })

  const clearKeys = useMutation({
    mutationFn: () =>
      managementRequest<ApiKeysResponse>('/v0/management/api-keys?all=true', secret, {
        method: 'DELETE',
      }),
    onSuccess: () => {
      void refresh()
    },
  })

  const mutationError = [generateKey, appendKey, replaceKey, deleteKey, clearKeys].find(
    (mutation) => mutation.isError,
  )?.error as Error | undefined

  const hasKeys = apiKeys.data?.total ? apiKeys.data.total > 0 : false

  return (
    <PageShell
      eyebrow='API Keys'
      title='Management access keys'
      description='Read from `/dashboard`, mutate through existing management REST endpoints.'
      actions={
        <div className='flex flex-col gap-3 sm:flex-row sm:flex-wrap'>
          <Button
            type='button'
            onClick={() => generateKey.mutate()}
            disabled={generateKey.isPending}
            className='h-11 rounded-xl px-4'
          >
            {generateKey.isPending ? 'Generating…' : 'Generate key'}
          </Button>
          <Button
            type='button'
            variant='destructive'
            onClick={() => clearKeys.mutate()}
            disabled={clearKeys.isPending || !apiKeys.data || apiKeys.data.total === 0}
            className='h-11 rounded-xl px-4'
          >
            {clearKeys.isPending ? 'Clearing…' : 'Clear all'}
          </Button>
        </div>
      }
    >
      <QueryState
        isLoading={apiKeys.isLoading}
        isError={apiKeys.isError}
        error={apiKeys.error as Error | null}
      >
        {apiKeys.data ? (
          <div className='space-y-6'>
            <div className='grid gap-4 md:grid-cols-3'>
              <Card className='rounded-2xl border border-[var(--border)] bg-[var(--card)]'>
                <CardContent className='p-5'>
                  <p className='text-sm text-[var(--muted-foreground)]'>Key count</p>
                  <p className='mt-3 text-3xl font-semibold text-white'>{apiKeys.data.total}</p>
                </CardContent>
              </Card>
              <Card className='rounded-2xl border border-[var(--border)] bg-[var(--card)] md:col-span-2'>
                <CardContent className='p-5'>
                  <div className='flex flex-col gap-3 md:flex-row md:items-start md:justify-between'>
                    <div>
                      <p className='text-sm text-[var(--muted-foreground)]'>Source</p>
                      <p className='mt-3 text-white'>
                        {apiKeys.data.generated_only
                          ? 'Session-generated keys only'
                          : 'Config-backed or mixed key set'}
                      </p>
                    </div>
                    <Badge
                      variant='outline'
                      className='rounded-full px-3 py-1 text-xs text-[var(--muted-foreground)]'
                    >
                      {hasKeys ? 'Ready to rotate' : 'No keys yet'}
                    </Badge>
                  </div>
                  {mutationError ? (
                    <p className='mt-3 rounded-2xl border border-red-500/30 bg-red-500/10 px-4 py-3 text-sm text-red-100'>
                      {mutationError.message}
                    </p>
                  ) : null}
                </CardContent>
              </Card>
            </div>

            <Card className='rounded-3xl border border-[var(--border)] bg-[var(--card)]'>
              <CardContent className='p-6'>
                <h3 className='text-lg font-semibold'>Add key</h3>
                <div className='mt-4 flex flex-col gap-3 md:flex-row'>
                  <Input
                    type='text'
                    value={newKeyValue}
                    onChange={(event) => setNewKeyValue(event.target.value)}
                    className='h-11 flex-1 rounded-2xl border-[var(--border)] bg-black/20 px-4 text-white'
                    placeholder='rsk-custom-...'
                  />
                  <Button
                    type='button'
                    variant='outline'
                    onClick={() => appendKey.mutate(newKeyValue)}
                    disabled={appendKey.isPending || newKeyValue.trim().length === 0}
                    className='h-11 rounded-2xl px-4 text-white'
                  >
                    {appendKey.isPending ? 'Adding…' : 'Add key'}
                  </Button>
                </div>
              </CardContent>
            </Card>

            {!hasKeys ? (
              <article className='rounded-3xl border border-dashed border-[var(--border)] bg-black/10 p-6 text-sm text-[var(--muted-foreground)]'>
                <p className='text-white'>No management keys configured.</p>
                <p className='mt-2'>
                  Generate a key for this session or add one manually to unlock the dashboard.
                </p>
              </article>
            ) : null}

            <div className='space-y-3 md:hidden'>
              {apiKeys.data.items.map((item, index) => (
                <Card
                  key={`${item}-${index}`}
                  className='rounded-2xl border border-[var(--border)] bg-[var(--card)]'
                >
                  <CardContent className='p-4'>
                    <p className='text-xs tracking-[0.2em] text-[var(--muted-foreground)] uppercase'>
                      Key #{index + 1}
                    </p>
                    <code className='mt-3 block overflow-x-auto text-sm text-white'>{item}</code>
                    {replaceIndex === index ? (
                      <div className='mt-3 flex flex-col gap-3'>
                        <Input
                          type='text'
                          value={replaceValue}
                          onChange={(event) => setReplaceValue(event.target.value)}
                          className='h-11 rounded-2xl border-[var(--border)] bg-black/20 text-white'
                        />
                        <div className='flex flex-col gap-2 sm:flex-row'>
                          <Button
                            type='button'
                            onClick={() => replaceKey.mutate({ index, value: replaceValue })}
                            disabled={replaceKey.isPending || replaceValue.trim().length === 0}
                            className='h-11 rounded-2xl px-4'
                          >
                            Save
                          </Button>
                          <Button
                            type='button'
                            variant='outline'
                            onClick={() => {
                              setReplaceIndex(null)
                              setReplaceValue('')
                            }}
                            className='h-11 rounded-2xl px-4 text-white'
                          >
                            Cancel
                          </Button>
                        </div>
                      </div>
                    ) : (
                      <div className='mt-3 flex flex-col gap-2 sm:flex-row'>
                        <Button
                          type='button'
                          variant='outline'
                          onClick={() => {
                            setReplaceIndex(index)
                            setReplaceValue(item)
                          }}
                          className='h-11 rounded-xl px-3 text-white'
                        >
                          Replace
                        </Button>
                        <Button
                          type='button'
                          variant='destructive'
                          onClick={() => deleteKey.mutate(index)}
                          disabled={deleteKey.isPending}
                          className='h-11 rounded-xl px-3'
                        >
                          Delete
                        </Button>
                      </div>
                    )}
                  </CardContent>
                </Card>
              ))}
            </div>

            <div className='hidden overflow-hidden rounded-3xl border border-[var(--border)] bg-[var(--card)] md:block'>
              <div className='grid grid-cols-[0.5fr_1.5fr_1fr] gap-3 border-b border-[var(--border)] bg-white/5 px-4 py-3 text-xs tracking-[0.2em] text-[var(--muted-foreground)] uppercase'>
                <span>#</span>
                <span>Key</span>
                <span>Actions</span>
              </div>
              {apiKeys.data.items.map((item, index) => (
                <div
                  key={`${item}-${index}`}
                  className='grid grid-cols-[0.5fr_1.5fr_1fr] gap-3 border-t border-[var(--border)] px-4 py-4'
                >
                  <div className='text-sm text-[var(--muted-foreground)]'>{index + 1}</div>
                  <div>
                    <code className='block overflow-x-auto text-sm text-white'>{item}</code>
                    {replaceIndex === index ? (
                      <div className='mt-3 flex flex-col gap-3 md:flex-row'>
                        <Input
                          type='text'
                          value={replaceValue}
                          onChange={(event) => setReplaceValue(event.target.value)}
                          className='h-11 flex-1 rounded-2xl border-[var(--border)] bg-black/20 text-white'
                        />
                        <Button
                          type='button'
                          onClick={() => replaceKey.mutate({ index, value: replaceValue })}
                          disabled={replaceKey.isPending || replaceValue.trim().length === 0}
                          className='h-11 rounded-2xl px-4'
                        >
                          Save
                        </Button>
                        <Button
                          type='button'
                          variant='outline'
                          onClick={() => {
                            setReplaceIndex(null)
                            setReplaceValue('')
                          }}
                          className='h-11 rounded-2xl px-4 text-white'
                        >
                          Cancel
                        </Button>
                      </div>
                    ) : null}
                  </div>
                  <div className='flex flex-wrap gap-2'>
                    <Button
                      type='button'
                      variant='outline'
                      onClick={() => {
                        setReplaceIndex(index)
                        setReplaceValue(item)
                      }}
                      className='h-10 rounded-xl px-3 text-white'
                    >
                      Replace
                    </Button>
                    <Button
                      type='button'
                      variant='destructive'
                      onClick={() => deleteKey.mutate(index)}
                      disabled={deleteKey.isPending}
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
