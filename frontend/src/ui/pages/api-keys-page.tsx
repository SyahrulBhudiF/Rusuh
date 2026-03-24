import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
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

const MAX_KEY_LENGTH = 400

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
      description='Create and rotate the keys that clients use to access the proxy API.'
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
            <div className='space-y-5'>
              <div className='flex flex-col gap-2'>
                <p className='text-muted-foreground text-sm'>
                  {apiKeys.data.total} key{apiKeys.data.total === 1 ? '' : 's'} ·{' '}
                  {apiKeys.data.generated_only
                    ? 'session-generated only'
                    : 'config-backed or mixed'}
                </p>
                {mutationError ? (
                  <p className='border-destructive/30 bg-destructive/10 text-destructive dark:text-destructive-foreground rounded-2xl border px-4 py-3 text-sm'>
                    {mutationError.message}
                  </p>
                ) : null}
              </div>

              <div className='grid gap-4 xl:grid-cols-[minmax(0,320px)_minmax(0,1fr)]'>
                <section className='space-y-3'>
                  <div>
                    <h3 className='text-lg font-semibold'>Add key</h3>
                    <p className='text-muted-foreground mt-1 text-sm'>
                      Create or paste a management key.
                    </p>
                  </div>
                  <div className='grid gap-3'>
                    <Input
                      type='text'
                      value={newKeyValue}
                      onChange={(event) => setNewKeyValue(event.target.value)}
                      className='border-border text-foreground bg-background/60 h-11 rounded-2xl px-4'
                      placeholder='rsk-custom-...'
                      maxLength={MAX_KEY_LENGTH}
                    />
                    <Button
                      type='button'
                      variant='outline'
                      onClick={() => appendKey.mutate(newKeyValue)}
                      disabled={appendKey.isPending || newKeyValue.trim().length === 0}
                      className='h-11 rounded-2xl px-4 xl:w-fit'
                    >
                      {appendKey.isPending ? 'Adding…' : 'Add key'}
                    </Button>
                  </div>
                </section>

                <section className='space-y-3'>
                  <div className='flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between'>
                    <div>
                      <h3 className='text-lg font-semibold'>Active API keys</h3>
                      <p className='text-muted-foreground mt-1 text-sm'>
                        Most teams start by generating one key, then copy it into their client.
                      </p>
                    </div>
                    <Badge variant='outline' className='w-fit rounded-full px-3 py-1 text-xs'>
                      {hasKeys ? 'Ready to rotate' : 'No keys yet'}
                    </Badge>
                  </div>
                  {!hasKeys ? (
                    <div className='bg-muted/40 text-muted-foreground rounded-2xl p-4 text-sm'>
                      No API keys yet. Generate one key to let clients connect to the proxy.
                    </div>
                  ) : null}

                  <div className='space-y-2'>
                    {apiKeys.data.items.map((item, index) => (
                      <div
                        key={`${item}-${index}`}
                        className='border-border rounded-2xl border p-4'
                      >
                        <div className='flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between'>
                          <div className='min-w-0'>
                            <p className='text-muted-foreground text-xs uppercase'>
                              Key #{index + 1}
                            </p>
                            <code className='text-foreground mt-2 block overflow-x-auto text-sm break-all'>
                              {item}
                            </code>
                          </div>
                          {replaceIndex === index ? null : (
                            <div className='grid gap-2 sm:grid-cols-2'>
                              <Button
                                type='button'
                                variant='outline'
                                onClick={() => {
                                  setReplaceIndex(index)
                                  setReplaceValue(item)
                                }}
                                className='h-10 rounded-xl px-3'
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
                          )}
                        </div>

                        {replaceIndex === index ? (
                          <div className='mt-4 grid gap-3 sm:grid-cols-[minmax(0,1fr)_auto_auto]'>
                            <Input
                              type='text'
                              value={replaceValue}
                              onChange={(event) => setReplaceValue(event.target.value)}
                              className='border-border text-foreground bg-background/60 h-11 rounded-2xl'
                              maxLength={MAX_KEY_LENGTH}
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
                              className='h-11 rounded-2xl px-4'
                            >
                              Cancel
                            </Button>
                          </div>
                        ) : null}
                      </div>
                    ))}
                  </div>
                </section>
              </div>
            </div>
          </div>
        ) : null}
      </QueryState>
    </PageShell>
  )
}
