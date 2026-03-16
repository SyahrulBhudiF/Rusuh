import { useQueryClient } from '@tanstack/react-query'
import { useMemo } from 'react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'

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
          className='h-11 rounded-xl px-4'
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
            <div className='grid gap-4 lg:grid-cols-[minmax(0,1.35fr)_minmax(280px,0.85fr)]'>
              <section className='space-y-4'>
                <div className='grid gap-3 sm:grid-cols-2 xl:grid-cols-3'>
                  {overview.data.cards.map((card) => (
                    <div key={card.label} className='border-border rounded-2xl border p-4'>
                      <p className='text-muted-foreground text-sm'>{card.label}</p>
                      <p className='text-foreground mt-2 text-3xl font-semibold'>{card.value}</p>
                      <p className='text-muted-foreground mt-2 text-sm leading-6'>{card.hint}</p>
                    </div>
                  ))}
                </div>

                <div className='space-y-3'>
                  <div className='flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between'>
                    <div>
                      <h3 className='text-lg font-semibold'>Accounts overview</h3>
                      <p className='text-muted-foreground text-sm'>
                        By provider and lifecycle state.
                      </p>
                    </div>
                    <Badge
                      variant='outline'
                      className='dashboard-status w-fit rounded-full px-3 py-1 text-xs'
                    >
                      {accountSummaries?.length ?? 0} provider(s)
                    </Badge>
                  </div>
                  {summaryRows.length > 0 ? (
                    <div className='space-y-2'>
                      {summaryRows.map((summary) => (
                        <div
                          key={summary.provider}
                          className='border-border rounded-2xl border p-4'
                        >
                          <div className='flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between'>
                            <div>
                              <p className='text-foreground font-medium'>{summary.provider}</p>
                              <p className='text-muted-foreground mt-1 text-sm'>
                                Total {summary.total}
                              </p>
                            </div>
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
                                <span className='text-muted-foreground text-sm'>
                                  No status data
                                </span>
                              )}
                            </div>
                          </div>
                        </div>
                      ))}
                    </div>
                  ) : (
                    <div className='bg-muted/40 text-muted-foreground rounded-2xl p-4 text-sm'>
                      No account summaries available yet.
                    </div>
                  )}
                </div>
              </section>

              <section className='space-y-3'>
                <div>
                  <h3 className='text-lg font-semibold'>Runtime facts</h3>
                  <p className='text-muted-foreground mt-1 text-sm leading-6'>
                    Current service health, routing posture, and model surface.
                  </p>
                </div>
                <Badge
                  variant='outline'
                  className='dashboard-status w-fit rounded-full px-3 py-1 text-xs'
                >
                  {hasProviders ? 'Providers online' : 'No providers'}
                </Badge>
                <dl className='space-y-2'>
                  <div className='flex items-start justify-between gap-4 py-2 text-sm'>
                    <dt className='text-muted-foreground'>Health</dt>
                    <dd className='text-foreground text-right'>{overview.data.health.status}</dd>
                  </div>
                  <div className='flex items-start justify-between gap-4 py-2 text-sm'>
                    <dt className='text-muted-foreground'>Service</dt>
                    <dd className='text-foreground text-right'>{overview.data.health.service}</dd>
                  </div>
                  <div className='flex items-start justify-between gap-4 py-2 text-sm'>
                    <dt className='text-muted-foreground'>Routing</dt>
                    <dd className='text-foreground text-right'>{overview.data.routing_strategy}</dd>
                  </div>
                  <div className='flex items-start justify-between gap-4 py-2 text-sm'>
                    <dt className='text-muted-foreground'>Models</dt>
                    <dd className='text-foreground text-right'>
                      {overview.data.available_model_count}
                    </dd>
                  </div>
                  <div className='flex items-start justify-between gap-4 py-2 text-sm'>
                    <dt className='text-muted-foreground'>Providers</dt>
                    <dd className='text-foreground text-right'>
                      {hasProviders ? providerNames.join(', ') : 'None'}
                    </dd>
                  </div>
                </dl>
              </section>
            </div>
          </>
        ) : null}
      </QueryState>
    </PageShell>
  )
}
