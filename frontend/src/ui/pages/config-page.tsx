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
      description='Read-only runtime config. Use this page to inspect the current setup, not to edit it.'
      actions={
        <div className='border-border bg-background/60 grid grid-cols-1 gap-2 rounded-2xl border p-1 sm:w-fit sm:grid-cols-3 lg:self-center'>
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
            <div className='space-y-5'>
              <div className='grid gap-3 sm:grid-cols-2 xl:grid-cols-4'>
                <div className='border-border rounded-2xl border p-4'>
                  <p className='text-muted-foreground text-sm'>Listen addr</p>
                  <p className='text-foreground mt-2 text-lg font-semibold break-all'>
                    {config.data.listen_addr}
                  </p>
                </div>
                <div className='border-border rounded-2xl border p-4'>
                  <p className='text-muted-foreground text-sm'>Routing</p>
                  <p className='text-foreground mt-2 text-lg font-semibold'>
                    {config.data.routing_strategy}
                  </p>
                </div>
                <div className='border-border rounded-2xl border p-4'>
                  <p className='text-muted-foreground text-sm'>Providers</p>
                  <p className='text-foreground mt-2 text-lg font-semibold'>
                    {config.data.provider_count}
                  </p>
                </div>
                <div className='border-border rounded-2xl border p-4'>
                  <p className='text-muted-foreground text-sm'>API keys</p>
                  <p className='text-foreground mt-2 text-lg font-semibold'>
                    {config.data.api_key_count}
                  </p>
                </div>
              </div>

              <div className='grid gap-5 lg:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]'>
                <section className='space-y-3'>
                  <div>
                    <h3 className='text-lg font-semibold'>Runtime settings</h3>
                    <p className='text-muted-foreground mt-1 text-sm'>
                      Runtime behavior and management flags.
                    </p>
                  </div>
                  <dl className='space-y-2'>
                    <div className='flex items-start justify-between gap-4 py-2 text-sm'>
                      <dt className='text-muted-foreground'>Host</dt>
                      <dd className='text-foreground text-right'>
                        {config.data.host || '(all interfaces)'}
                      </dd>
                    </div>
                    <div className='flex items-start justify-between gap-4 py-2 text-sm'>
                      <dt className='text-muted-foreground'>Port</dt>
                      <dd className='text-foreground text-right'>{config.data.port}</dd>
                    </div>
                    <div className='flex items-start justify-between gap-4 py-2 text-sm'>
                      <dt className='text-muted-foreground'>Auth dir</dt>
                      <dd className='text-foreground max-w-[18rem] text-right break-all'>
                        {config.data.auth_dir || '(default)'}
                      </dd>
                    </div>
                    <div className='flex items-start justify-between gap-4 py-2 text-sm'>
                      <dt className='text-muted-foreground'>Request retry</dt>
                      <dd className='text-foreground text-right'>{config.data.request_retry}</dd>
                    </div>
                    <div className='flex items-start justify-between gap-4 py-2 text-sm'>
                      <dt className='text-muted-foreground'>Debug</dt>
                      <dd>
                        <BoolPill value={config.data.debug} />
                      </dd>
                    </div>
                    <div className='flex items-start justify-between gap-4 py-2 text-sm'>
                      <dt className='text-muted-foreground'>Management API</dt>
                      <dd>
                        <BoolPill value={config.data.management.enabled} />
                      </dd>
                    </div>
                    <div className='flex items-start justify-between gap-4 py-2 text-sm'>
                      <dt className='text-muted-foreground'>Remote management</dt>
                      <dd>
                        <BoolPill value={config.data.management.allow_remote} />
                      </dd>
                    </div>
                    <div className='flex items-start justify-between gap-4 py-2 text-sm'>
                      <dt className='text-muted-foreground'>OAuth alias rules</dt>
                      <dd className='text-foreground text-right'>
                        {config.data.oauth_alias_count} across{' '}
                        {config.data.oauth_alias_channel_count} channel(s)
                      </dd>
                    </div>
                  </dl>
                </section>

                <section className='space-y-4'>
                  <div className='flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between'>
                    <div>
                      <h3 className='text-lg font-semibold'>Configured providers</h3>
                      <p className='text-muted-foreground mt-1 text-sm'>
                        Configured providers and key inventories.
                      </p>
                    </div>
                    <Badge variant='outline' className='w-fit rounded-full px-3 py-1 text-xs'>
                      {hasProviders ? `${providerNames.length} configured` : 'No providers'}
                    </Badge>
                  </div>

                  <div>
                    <p className='text-muted-foreground text-sm'>Registered providers</p>
                    {hasProviders ? (
                      <div className='mt-3 flex flex-wrap gap-2'>
                        {providerNames.map((name) => (
                          <Badge key={name} variant='outline' className='rounded-full px-3 py-1'>
                            {name}
                          </Badge>
                        ))}
                      </div>
                    ) : (
                      <p className='text-muted-foreground mt-2 text-sm'>
                        No providers configured yet. Add an account first, then return here to inspect the runtime config.
                      </p>
                    )}
                  </div>

                  <div className='grid gap-3 sm:grid-cols-2'>
                    <div className='border-border rounded-2xl border p-4'>
                      <p className='text-muted-foreground text-sm'>Gemini API key entries</p>
                      <p className='text-foreground mt-2 text-2xl font-semibold'>
                        {config.data.gemini_api_keys.length}
                      </p>
                    </div>
                    <div className='border-border rounded-2xl border p-4'>
                      <p className='text-muted-foreground text-sm'>Codex API key entries</p>
                      <p className='text-foreground mt-2 text-2xl font-semibold'>
                        {config.data.codex_api_keys.length}
                      </p>
                    </div>
                    <div className='border-border rounded-2xl border p-4'>
                      <p className='text-muted-foreground text-sm'>Claude API key entries</p>
                      <p className='text-foreground mt-2 text-2xl font-semibold'>
                        {config.data.claude_api_keys.length}
                      </p>
                    </div>
                    <div className='border-border rounded-2xl border p-4'>
                      <p className='text-muted-foreground text-sm'>OpenAI-compatible providers</p>
                      <p className='text-foreground mt-2 text-2xl font-semibold'>
                        {config.data.openai_compat.length}
                      </p>
                    </div>
                  </div>
                </section>
              </div>
            </div>
          ) : (
            <Card className='border-border bg-card rounded-3xl border'>
              <CardContent className='p-5 md:p-6'>
                <p className='text-muted-foreground mb-4 text-sm'>
                  Raw {format.toUpperCase()} view of the current runtime snapshot.
                </p>
                <pre className='text-foreground overflow-x-auto text-sm break-words whitespace-pre-wrap'>
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
