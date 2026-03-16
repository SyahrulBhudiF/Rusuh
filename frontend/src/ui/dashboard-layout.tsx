import { Link, useRouterState } from '@tanstack/react-router'
import { useState, type PropsWithChildren } from 'react'

import { useManagementStatusQuery } from '../lib/management-api'
import { useManagementAuth } from '../lib/management-auth'
import { useOverviewQuery } from '../lib/query'
import { cn } from '../lib/utils'

const navItems = [
  { to: '/', label: 'Overview' },
  { to: '/accounts', label: 'Accounts' },
  { to: '/api-keys', label: 'API Keys' },
  { to: '/config', label: 'Config' },
] as const

export function DashboardLayout({ children }: PropsWithChildren<object>) {
  const pathname = useRouterState({ select: (state) => state.location.pathname })
  const overview = useOverviewQuery()
  const managementStatus = useManagementStatusQuery()
  const { clearSecret } = useManagementAuth()
  const [mobileNavOpen, setMobileNavOpen] = useState(false)

  const providerCount = overview.data?.provider_names.length ?? 0
  return (
    <div className='min-h-screen text-[var(--foreground)]'>
      <div className='border-b border-[var(--border)] bg-[color:rgba(40,30,34,0.78)] px-4 py-4 md:hidden'>
        <div className='flex items-center justify-between gap-3'>
          <div>
            <p className='text-xs tracking-[0.24em] text-[var(--muted-foreground)] uppercase'>
              Rusuh Dashboard
            </p>
            <h1 className='mt-1 text-lg font-semibold text-white'>Management UI</h1>
          </div>
          <div className='flex items-center gap-2'>
            <button
              type='button'
              onClick={clearSecret}
              className='min-h-11 rounded-xl border border-[var(--border)] bg-white/5 px-4 text-sm text-white transition hover:bg-white/10'
            >
              Lock
            </button>
            <button
              type='button'
              onClick={() => setMobileNavOpen((value) => !value)}
              aria-expanded={mobileNavOpen}
              aria-controls='dashboard-mobile-nav'
              className='min-h-11 rounded-xl border border-[var(--border)] bg-[color:rgba(210,138,54,0.16)] px-4 text-sm text-white transition hover:bg-[color:rgba(210,138,54,0.24)]'
            >
              {mobileNavOpen ? 'Close' : 'Menu'}
            </button>
          </div>
        </div>
        {mobileNavOpen ? (
          <div
            id='dashboard-mobile-nav'
            className='dashboard-enter dashboard-enter-delay-1 mt-4 space-y-3'
          >
            <nav className='grid gap-2'>
              {navItems.map((item) => {
                const active = pathname === item.to

                return (
                  <Link
                    key={item.to}
                    to={item.to}
                    onClick={() => setMobileNavOpen(false)}
                    className={cn(
                      'flex min-h-11 items-center rounded-xl border px-4 py-3 text-sm transition',
                      active
                        ? 'border-[color:rgba(210,138,54,0.28)] bg-[color:rgba(210,138,54,0.14)] text-white'
                        : 'border-transparent bg-transparent text-[var(--muted-foreground)] hover:border-[var(--border)] hover:bg-white/5 hover:text-white',
                    )}
                    activeOptions={{ exact: item.to === '/' }}
                  >
                    {item.label}
                  </Link>
                )
              })}
            </nav>
            <div className='dashboard-card rounded-[1.6rem] border border-[var(--border)] bg-[var(--card)] p-4 shadow-[0_16px_40px_rgba(0,0,0,0.18)]'>
              <p className='text-xs tracking-[0.2em] text-[var(--muted-foreground)] uppercase'>
                Status
              </p>
              <p className='mt-2 text-sm text-white'>
                {overview.data
                  ? `${overview.data.health.status} · ${overview.data.routing_strategy}`
                  : 'Loading runtime status…'}
              </p>
              <p className='mt-1 text-xs text-[var(--muted-foreground)]'>
                {managementStatus.data
                  ? `Mgmt port ${managementStatus.data.port} · ${providerCount} provider${providerCount === 1 ? '' : 's'}`
                  : managementStatus.isError
                    ? 'Management auth failed'
                    : 'Checking management access…'}
              </p>
            </div>
          </div>
        ) : null}
      </div>
      <div className='mx-auto flex min-h-screen max-w-7xl'>
        <aside className='hidden w-72 flex-col border-r border-[var(--border)] bg-[color:rgba(34,27,31,0.68)] p-6 md:flex'>
          <div>
            <p className='text-xs tracking-[0.24em] text-[var(--muted-foreground)] uppercase'>
              Rusuh Dashboard
            </p>
            <h1 className='mt-2 text-[1.95rem] font-semibold tracking-[-0.03em] text-white'>
              Management UI
            </h1>
            <p className='mt-3 text-sm text-[var(--muted-foreground)]'>
              Accounts, OAuth, config, and API key operations from one place.
            </p>
          </div>
          <nav className='mt-10 space-y-2'>
            {navItems.map((item) => {
              const active = pathname === item.to

              return (
                <Link
                  key={item.to}
                  to={item.to}
                  className={cn(
                    'flex w-full items-center rounded-xl border px-4 py-3 text-left text-sm transition',
                    active
                      ? 'border-[color:rgba(210,138,54,0.28)] bg-[color:rgba(210,138,54,0.14)] text-white'
                      : 'border-transparent bg-transparent text-[var(--muted-foreground)] hover:border-[var(--border)] hover:bg-white/5 hover:text-white',
                  )}
                  activeOptions={{ exact: item.to === '/' }}
                >
                  {item.label}
                </Link>
              )
            })}
          </nav>
          <div className='dashboard-card dashboard-enter dashboard-enter-delay-1 mt-auto space-y-3 rounded-[1.6rem] border border-[var(--border)] bg-[var(--card)] p-4 shadow-[0_16px_40px_rgba(0,0,0,0.18)]'>
            <div>
              <p className='text-xs tracking-[0.2em] text-[var(--muted-foreground)] uppercase'>
                Status
              </p>
              <p className='mt-2 text-sm text-white'>
                {overview.data
                  ? `${overview.data.health.status} · ${overview.data.routing_strategy}`
                  : 'Loading runtime status…'}
              </p>
              <p className='mt-1 text-xs text-[var(--muted-foreground)]'>
                {managementStatus.data
                  ? `Mgmt port ${managementStatus.data.port} · ${providerCount} provider${providerCount === 1 ? '' : 's'}`
                  : managementStatus.isError
                    ? 'Management auth failed'
                    : 'Checking management access…'}
              </p>
            </div>

            <button
              type='button'
              onClick={clearSecret}
              className='min-h-11 w-full rounded-xl border border-[var(--border)] bg-white/5 px-3 py-2 text-sm text-white transition hover:bg-white/10'
            >
              Lock dashboard
            </button>
          </div>
        </aside>

        <main className='dashboard-enter dashboard-enter-delay-1 flex-1 p-4 md:p-8'>
          {children}
        </main>
      </div>
    </div>
  )
}
