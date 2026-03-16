import { useQueryClient } from '@tanstack/react-query'
import { useMemo } from 'react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'

import { queryKeys, useOverviewQuery } from '../../lib/query'
import { PageShell } from '../page-shell'
import { QueryState } from '../query-state'
import { statusTone } from '../status-tone'

export function OverviewPage() {
  const queryClient = useQueryClient()
  const overview = useOverviewQuery()
  const accountSummaries = overview.data?.account_summaries
  const providerNames = overview.data?.provider_names ?? []

  const summaryRows = useMemo(
    () =>
      (accountSummaries ?? []).map((summary) => ({
        ...summary,
        chips: [
          ['active', summary.active],
          ['refreshing', summary.refreshing],
          ['pending', summary.pending],
          ['error', summary.error],
          ['disabled', summary.disabled],
          ['unknown', summary.unknown],
        ].filter(([, count]) => Number(count) > 0),
      })),
    [accountSummaries],
  )

  const hasProviders = providerNames.length > 0

  return (
    <PageShell
      eyebrow='Overview'
      title='Proxy runtime overview'
      description='Live health, account inventory, routing posture, and model surface from the Rust backend.'
      actions={
        <Button
          type='button'
          variant='outline'
          onClick={() => {
            void queryClient.invalidateQueries({ queryKey: queryKeys.overview })
          }}
          className='h-11 rounded-xl px-4 text-white'
        >
          Refresh
        </Button>
      }
    >
      <QueryState
        isLoading={overview.isLoading}
        isError={overview.isError}
        error={overview.error as Error | null}
      >
        {overview.data ? (
          <>
            <div className='grid gap-4 md:grid-cols-2 xl:grid-cols-4'>
              {overview.data.cards.map((card) => (
                <Card
                  key={card.label}
                  className='dashboard-card rounded-2xl border border-[var(--border)] bg-[var(--card)] shadow-[0_8px_30px_rgba(0,0,0,0.25)]'
                >
                  <CardContent className='p-5'>
                    <p className='text-sm text-[var(--muted-foreground)]'>{card.label}</p>
                    <p className='mt-3 text-3xl font-semibold text-white'>{card.value}</p>
                    <p className='mt-2 text-sm text-[var(--muted-foreground)]'>{card.hint}</p>
                  </CardContent>
                </Card>
              ))}
            </div>

            <div className='mt-6 grid gap-6 xl:grid-cols-[1.4fr_0.8fr]'>
              <article className='rounded-3xl border border-[var(--border)] bg-[var(--card)] p-6'>
                <div className='flex flex-col gap-3 md:flex-row md:items-start md:justify-between'>
                  <div>
                    <h3 className='text-lg font-semibold'>Accounts overview</h3>
                    <p className='text-sm text-[var(--muted-foreground)]'>
                      Grouped auth records by provider and lifecycle state.
                    </p>
                  </div>
                  <Badge
                    variant='outline'
                    className='dashboard-status rounded-full px-3 py-1 text-xs text-[var(--muted-foreground)]'
                  >
                    {accountSummaries?.length ?? 0} provider(s)
                  </Badge>
                </div>

                {summaryRows.length > 0 ? (
                  <div className='mt-5 overflow-hidden rounded-2xl border border-[var(--border)]'>
                    <div className='grid grid-cols-[1.2fr_0.8fr_1.2fr] gap-3 border-b border-[var(--border)] bg-white/5 px-4 py-3 text-xs tracking-[0.2em] text-[var(--muted-foreground)] uppercase'>
                      <span>Provider</span>
                      <span>Total</span>
                      <span>Status mix</span>
                    </div>
                    {summaryRows.map((summary) => (
                      <div
                        key={summary.provider}
                        className='grid grid-cols-[1.2fr_0.8fr_1.2fr] gap-3 px-4 py-4 text-sm'
                      >
                        <span className='text-white'>{summary.provider}</span>
                        <span className='text-[var(--muted-foreground)]'>{summary.total}</span>
                        <div className='flex flex-wrap gap-2'>
                          {summary.chips.length > 0 ? (
                            summary.chips.map(([label, count]) => (
                              <Badge
                                key={String(label)}
                                variant='outline'
                                className={`dashboard-status rounded-full px-2.5 py-1 text-xs ${statusTone(String(label))}`}
                              >
                                {label}: {count}
                              </Badge>
                            ))
                          ) : (
                            <span className='text-[var(--muted-foreground)]'>No status data</span>
                          )}
                        </div>
                      </div>
                    ))}
                  </div>
                ) : (
                  <div className='mt-5 rounded-2xl border border-dashed border-[var(--border)] bg-black/10 p-5 text-sm text-[var(--muted-foreground)]'>
                    No account summaries available yet.
                  </div>
                )}
              </article>

              <article className='rounded-3xl border border-[var(--border)] bg-[var(--card)] p-6'>
                <div className='flex items-start justify-between gap-4'>
                  <div>
                    <h3 className='text-lg font-semibold'>Runtime facts</h3>
                    <p className='mt-1 text-sm text-[var(--muted-foreground)]'>
                      Current service health, routing posture, and available model surface.
                    </p>
                  </div>
                  <Badge
                    variant='outline'
                    className='dashboard-status rounded-full px-3 py-1 text-xs text-[var(--muted-foreground)]'
                  >
                    {hasProviders ? 'Providers online' : 'No providers'}
                  </Badge>
                </div>
                <ul className='mt-4 space-y-3 text-sm text-[var(--muted-foreground)]'>
                  <li className='flex items-start justify-between gap-4'>
                    <span>Health</span>
                    <span className='text-white'>{overview.data.health.status}</span>
                  </li>
                  <li className='flex items-start justify-between gap-4'>
                    <span>Service</span>
                    <span className='text-white'>{overview.data.health.service}</span>
                  </li>
                  <li className='flex items-start justify-between gap-4'>
                    <span>Routing strategy</span>
                    <span className='text-white'>{overview.data.routing_strategy}</span>
                  </li>
                  <li className='flex items-start justify-between gap-4'>
                    <span>Available models</span>
                    <span className='text-white'>{overview.data.available_model_count}</span>
                  </li>
                  <li className='flex items-start justify-between gap-4'>
                    <span>Providers</span>
                    <span className='text-right text-white'>
                      {hasProviders ? providerNames.join(', ') : 'None'}
                    </span>
                  </li>
                </ul>
              </article>
            </div>
          </>
        ) : null}
      </QueryState>
    </PageShell>
  )
}
