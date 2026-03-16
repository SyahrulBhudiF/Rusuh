import { useMemo, useState } from 'react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'

import { useConfigQuery } from '../../lib/query'
import { PageShell } from '../page-shell'
import { QueryState } from '../query-state'
import { statusTone } from '../status-tone'

function BoolPill({ value }: { value: boolean }) {
  return (
    <span
      className={`rounded-full border px-2.5 py-1 text-xs ${statusTone(value ? 'active' : 'disabled')}`}
    >
      {value ? 'enabled' : 'disabled'}
    </span>
  )
}

function toYamlLines(value: unknown, indent = 0): string[] {
  const prefix = '  '.repeat(indent)

  if (Array.isArray(value)) {
    if (value.length === 0) {
      return [`${prefix}[]`]
    }

    return value.flatMap((item) => {
      if (item && typeof item === 'object') {
        const nested = toYamlLines(item, indent + 1)
        const [first, ...rest] = nested
        return [`${prefix}- ${first.trimStart()}`, ...rest]
      }

      return [`${prefix}- ${String(item)}`]
    })
  }

  if (value && typeof value === 'object') {
    const entries = Object.entries(value as Record<string, unknown>)
    if (entries.length === 0) {
      return [`${prefix}{}`]
    }

    return entries.flatMap(([key, nested]) => {
      if (nested && typeof nested === 'object') {
        return [`${prefix}${key}:`, ...toYamlLines(nested, indent + 1)]
      }

      return [`${prefix}${key}: ${String(nested)}`]
    })
  }

  return [`${prefix}${String(value)}`]
}

export function ConfigPage() {
  const config = useConfigQuery()
  const [format, setFormat] = useState<'structured' | 'json' | 'yaml'>('structured')

  const rawJson = useMemo(() => {
    if (!config.data) return ''
    return JSON.stringify(config.data, null, 2)
  }, [config.data])

  const rawYaml = useMemo(() => {
    if (!config.data) return ''
    return toYamlLines(config.data).join('\n')
  }, [config.data])

  const providerNames = config.data?.provider_names ?? []
  const hasProviders = providerNames.length > 0

  return (
    <PageShell
      eyebrow='Config'
      title='Runtime configuration'
      description='Read-only runtime snapshot: listener, management policy, routing, and provider key inventory.'
      actions={
        <div className='grid grid-cols-3 gap-2 rounded-2xl border border-[var(--border)] bg-black/20 p-1 sm:flex sm:w-fit'>
          {(['structured', 'json', 'yaml'] as const).map((value) => (
            <Button
              key={value}
              type='button'
              variant={format === value ? 'default' : 'ghost'}
              onClick={() => setFormat(value)}
              className='h-11 rounded-xl px-3 capitalize'
            >
              {value}
            </Button>
          ))}
        </div>
      }
    >
      <QueryState
        isLoading={config.isLoading}
        isError={config.isError}
        error={config.error as Error | null}
      >
        {config.data ? (
          format === 'structured' ? (
            <div className='space-y-6'>
              <div className='grid gap-4 md:grid-cols-2 xl:grid-cols-4'>
                <Card className='rounded-2xl border border-[var(--border)] bg-[var(--card)]'>
                  <CardContent className='p-5'>
                    <p className='text-sm text-[var(--muted-foreground)]'>Listen addr</p>
                    <p className='mt-3 text-lg font-semibold break-all text-white'>
                      {config.data.listen_addr}
                    </p>
                  </CardContent>
                </Card>
                <Card className='rounded-2xl border border-[var(--border)] bg-[var(--card)]'>
                  <CardContent className='p-5'>
                    <p className='text-sm text-[var(--muted-foreground)]'>Routing</p>
                    <p className='mt-3 text-lg font-semibold text-white'>
                      {config.data.routing_strategy}
                    </p>
                  </CardContent>
                </Card>
                <Card className='rounded-2xl border border-[var(--border)] bg-[var(--card)]'>
                  <CardContent className='p-5'>
                    <p className='text-sm text-[var(--muted-foreground)]'>Providers</p>
                    <p className='mt-3 text-lg font-semibold text-white'>
                      {config.data.provider_count}
                    </p>
                  </CardContent>
                </Card>
                <Card className='rounded-2xl border border-[var(--border)] bg-[var(--card)]'>
                  <CardContent className='p-5'>
                    <p className='text-sm text-[var(--muted-foreground)]'>API keys</p>
                    <p className='mt-3 text-lg font-semibold text-white'>
                      {config.data.api_key_count}
                    </p>
                  </CardContent>
                </Card>
              </div>

              <div className='grid gap-6 xl:grid-cols-[0.9fr_1.1fr]'>
                <article className='rounded-3xl border border-[var(--border)] bg-[var(--card)] p-6'>
                  <h3 className='text-lg font-semibold'>Core settings</h3>
                  <dl className='mt-4 space-y-3 text-sm'>
                    <div className='flex items-start justify-between gap-4'>
                      <dt className='text-[var(--muted-foreground)]'>Host</dt>
                      <dd className='text-right text-white'>
                        {config.data.host || '(all interfaces)'}
                      </dd>
                    </div>
                    <div className='flex items-start justify-between gap-4'>
                      <dt className='text-[var(--muted-foreground)]'>Port</dt>
                      <dd className='text-right text-white'>{config.data.port}</dd>
                    </div>
                    <div className='flex items-start justify-between gap-4'>
                      <dt className='text-[var(--muted-foreground)]'>Auth dir</dt>
                      <dd className='max-w-[18rem] text-right break-all text-white'>
                        {config.data.auth_dir || '(default)'}
                      </dd>
                    </div>
                    <div className='flex items-start justify-between gap-4'>
                      <dt className='text-[var(--muted-foreground)]'>Request retry</dt>
                      <dd className='text-right text-white'>{config.data.request_retry}</dd>
                    </div>
                    <div className='flex items-start justify-between gap-4'>
                      <dt className='text-[var(--muted-foreground)]'>Debug</dt>
                      <dd>
                        <BoolPill value={config.data.debug} />
                      </dd>
                    </div>
                    <div className='flex items-start justify-between gap-4'>
                      <dt className='text-[var(--muted-foreground)]'>Management API</dt>
                      <dd>
                        <BoolPill value={config.data.management.enabled} />
                      </dd>
                    </div>
                    <div className='flex items-start justify-between gap-4'>
                      <dt className='text-[var(--muted-foreground)]'>Remote management</dt>
                      <dd>
                        <BoolPill value={config.data.management.allow_remote} />
                      </dd>
                    </div>
                    <div className='flex items-start justify-between gap-4'>
                      <dt className='text-[var(--muted-foreground)]'>OAuth alias rules</dt>
                      <dd className='text-right text-white'>
                        {config.data.oauth_alias_count} across{' '}
                        {config.data.oauth_alias_channel_count} channel(s)
                      </dd>
                    </div>
                  </dl>
                </article>

                <article className='rounded-3xl border border-[var(--border)] bg-[var(--card)] p-6'>
                  <div className='flex flex-col gap-3 md:flex-row md:items-start md:justify-between'>
                    <div>
                      <h3 className='text-lg font-semibold'>Provider inventory</h3>
                      <p className='mt-1 text-sm text-[var(--muted-foreground)]'>
                        Snapshot of configured providers and API key inventories.
                      </p>
                    </div>
                    <Badge
                      variant='outline'
                      className='rounded-full px-3 py-1 text-xs text-[var(--muted-foreground)]'
                    >
                      {hasProviders ? `${providerNames.length} configured` : 'No providers'}
                    </Badge>
                  </div>
                  <div className='mt-4 space-y-4 text-sm'>
                    <div>
                      <p className='text-[var(--muted-foreground)]'>Registered providers</p>
                      {hasProviders ? (
                        <div className='mt-2 flex flex-wrap gap-2'>
                          {providerNames.map((name) => (
                            <Badge
                              key={name}
                              variant='outline'
                              className='rounded-full border-[var(--border)] bg-white/5 px-3 py-1 text-white'
                            >
                              {name}
                            </Badge>
                          ))}
                        </div>
                      ) : (
                        <p className='mt-2 text-white'>
                          None configured in the current runtime snapshot.
                        </p>
                      )}
                    </div>
                    <div className='grid gap-4 md:grid-cols-2'>
                      <section className='rounded-2xl border border-[var(--border)] bg-black/10 p-4'>
                        <p className='font-medium text-white'>Gemini API key entries</p>
                        <p className='mt-2 text-2xl font-semibold text-white'>
                          {config.data.gemini_api_keys.length}
                        </p>
                      </section>
                      <section className='rounded-2xl border border-[var(--border)] bg-black/10 p-4'>
                        <p className='font-medium text-white'>Codex API key entries</p>
                        <p className='mt-2 text-2xl font-semibold text-white'>
                          {config.data.codex_api_keys.length}
                        </p>
                      </section>
                      <section className='rounded-2xl border border-[var(--border)] bg-black/10 p-4'>
                        <p className='font-medium text-white'>Claude API key entries</p>
                        <p className='mt-2 text-2xl font-semibold text-white'>
                          {config.data.claude_api_keys.length}
                        </p>
                      </section>
                      <section className='rounded-2xl border border-[var(--border)] bg-black/10 p-4'>
                        <p className='font-medium text-white'>OpenAI-compatible providers</p>
                        <p className='mt-2 text-2xl font-semibold text-white'>
                          {config.data.openai_compat.length}
                        </p>
                      </section>
                    </div>
                  </div>
                </article>
              </div>
            </div>
          ) : (
            <Card className='rounded-3xl border border-[var(--border)] bg-[var(--card)]'>
              <CardContent className='p-6'>
                <p className='mb-4 text-sm text-[var(--muted-foreground)]'>
                  Raw {format.toUpperCase()} view of the current runtime snapshot.
                </p>
                <pre className='overflow-x-auto text-sm break-words whitespace-pre-wrap text-white'>
                  {format === 'json' ? rawJson : rawYaml}
                </pre>
              </CardContent>
            </Card>
          )
        ) : null}
      </QueryState>
    </PageShell>
  )
}
