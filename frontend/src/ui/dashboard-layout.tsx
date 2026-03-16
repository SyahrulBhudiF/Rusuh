import { Link, useRouterState } from '@tanstack/react-router'
import { LaptopMinimal, Moon, Sun } from 'lucide-react'
import { useState, type PropsWithChildren } from 'react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'

import { useManagementStatusQuery } from '../lib/management-api'
import { useManagementAuth } from '../lib/management-auth'
import { useOverviewQuery } from '../lib/query'
import { useThemeStore } from '../lib/theme'
import { cn } from '../lib/utils'

const navItems = [
  { to: '/', label: 'Overview' },
  { to: '/accounts', label: 'Accounts' },
  { to: '/api-keys', label: 'API Keys' },
  { to: '/config', label: 'Config' },
] as const

type ThemeSegmentedControlProps = {
  theme: 'light' | 'dark' | 'system'
  onChange: (theme: 'light' | 'dark' | 'system') => void
}

function ThemeSegmentedControl({ theme, onChange }: ThemeSegmentedControlProps) {
  const options = [
    { value: 'light' as const, label: 'Light', icon: Sun },
    { value: 'dark' as const, label: 'Dark', icon: Moon },
    { value: 'system' as const, label: 'System', icon: LaptopMinimal },
  ]

  return (
    <div className='bg-muted/60 ring-border/70 inline-flex w-fit max-w-full items-center gap-1 overflow-hidden rounded-2xl p-1 ring-1'>
      {options.map((option) => {
        const Icon = option.icon
        const active = theme === option.value
        return (
          <Button
            key={option.value}
            type='button'
            variant='ghost'
            size='icon-sm'
            onClick={() => onChange(option.value)}
            className={cn(
              'shrink rounded-xl',
              active
                ? 'bg-background text-foreground ring-border/70 shadow-sm ring-1'
                : 'text-muted-foreground hover:text-foreground',
            )}
          >
            <Icon className='size-4 shrink-0' />
            <span className='sr-only'>{option.label} mode</span>
          </Button>
        )
      })}
    </div>
  )
}

export function DashboardLayout({ children }: PropsWithChildren<object>) {
  const pathname = useRouterState({ select: (state) => state.location.pathname })
  const overview = useOverviewQuery()
  const managementStatus = useManagementStatusQuery()
  const { clearSecret } = useManagementAuth()
  const [mobileNavOpen, setMobileNavOpen] = useState(false)
  const theme = useThemeStore((state) => state.theme)
  const resolvedTheme = useThemeStore((state) => state.resolvedTheme)
  const setTheme = useThemeStore((state) => state.setTheme)

  const providerCount = overview.data?.provider_names.length ?? 0
  return (
    <div className='bg-background text-foreground min-h-screen'>
      <div className='mx-auto flex min-h-screen max-w-[1440px] overflow-x-clip'>
        <aside className='border-border/70 hidden w-72 flex-col border-r px-5 py-6 md:flex lg:w-80 lg:px-6 lg:py-7'>
          <div>
            <p className='text-muted-foreground text-xs tracking-[0.24em] uppercase'>
              Rusuh Dashboard
            </p>
            <h1 className='mt-2 text-[1.75rem] font-semibold tracking-[-0.03em] lg:text-[1.9rem]'>
              Management UI
            </h1>
          </div>
          <nav className='mt-10 space-y-1'>
            {navItems.map((item) => {
              const active = pathname === item.to
              return (
                <Link
                  key={item.to}
                  to={item.to}
                  className={cn(
                    'flex min-h-11 w-full items-center rounded-xl px-4 text-left text-sm font-medium transition-colors',
                    active
                      ? 'bg-accent text-foreground'
                      : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground',
                  )}
                  activeOptions={{ exact: item.to === '/' }}
                >
                  {item.label}
                </Link>
              )
            })}
          </nav>
          <div className='text-muted-foreground mt-auto space-y-2 text-sm'>
            <p>
              {overview.data
                ? `${overview.data.health.status} · ${overview.data.routing_strategy}`
                : 'Loading runtime status…'}
            </p>
            <p className='text-xs leading-5'>
              {managementStatus.data
                ? `Mgmt port ${managementStatus.data.port} · ${providerCount} provider${providerCount === 1 ? '' : 's'}`
                : managementStatus.isError
                  ? 'Management auth failed'
                  : 'Checking management access…'}
            </p>
            <p className='text-xs leading-5'>
              Theme: {theme === 'system' ? `system (${resolvedTheme})` : theme}
            </p>
            <div className='flex flex-col gap-2 pt-2 xl:flex-row xl:items-center xl:justify-between'>
              <Badge variant='secondary' className='w-fit rounded-full'>
                {providerCount} provider{providerCount === 1 ? '' : 's'}
              </Badge>
              <div className='flex flex-wrap items-center gap-2 xl:justify-end'>
                <ThemeSegmentedControl theme={theme} onChange={setTheme} />
                <Button
                  type='button'
                  variant='outline'
                  onClick={clearSecret}
                  className='w-fit rounded-xl'
                >
                  Lock
                </Button>
              </div>
            </div>
          </div>
        </aside>
        <div className='flex min-h-screen flex-1 flex-col'>
          <div className='border-border bg-background/90 sticky top-0 z-20 border-b px-4 py-3 backdrop-blur md:hidden'>
            <div className='flex items-center justify-between gap-3'>
              <div>
                <p className='text-muted-foreground text-xs tracking-[0.24em] uppercase'>
                  Rusuh Dashboard
                </p>
                <h1 className='mt-1 text-lg font-semibold'>Management UI</h1>
              </div>
              <div className='flex items-center justify-end gap-2'>
                <ThemeSegmentedControl theme={theme} onChange={setTheme} />
                <Button
                  type='button'
                  variant='outline'
                  size='sm'
                  onClick={clearSecret}
                  className='rounded-xl px-3'
                >
                  Lock
                </Button>
                <Button
                  type='button'
                  size='sm'
                  onClick={() => setMobileNavOpen((value) => !value)}
                  aria-expanded={mobileNavOpen}
                  aria-controls='dashboard-mobile-nav'
                  className='rounded-xl px-3'
                >
                  {mobileNavOpen ? 'Close' : 'Menu'}
                </Button>
              </div>
            </div>
            {mobileNavOpen ? (
              <div id='dashboard-mobile-nav' className='dashboard-enter mt-4 space-y-2'>
                <nav className='grid gap-2'>
                  {navItems.map((item) => {
                    const active = pathname === item.to
                    return (
                      <Link
                        key={item.to}
                        to={item.to}
                        onClick={() => setMobileNavOpen(false)}
                        className={cn(
                          'flex min-h-11 items-center rounded-xl px-4 text-sm font-medium transition-colors',
                          active
                            ? 'bg-accent text-foreground'
                            : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground',
                        )}
                        activeOptions={{ exact: item.to === '/' }}
                      >
                        {item.label}
                      </Link>
                    )
                  })}
                </nav>
                <div className='text-muted-foreground space-y-1 px-1 text-sm'>
                  <p>
                    {overview.data
                      ? `${overview.data.health.status} · ${overview.data.routing_strategy}`
                      : 'Loading runtime status…'}
                  </p>
                  <p className='text-xs leading-5'>
                    {managementStatus.data
                      ? `Mgmt port ${managementStatus.data.port} · ${providerCount} provider${providerCount === 1 ? '' : 's'}`
                      : managementStatus.isError
                        ? 'Management auth failed'
                        : 'Checking management access…'}
                  </p>
                  <p className='text-xs leading-5'>
                    Theme: {theme === 'system' ? `system (${resolvedTheme})` : theme}
                  </p>
                </div>
              </div>
            ) : null}
          </div>

          <main className='dashboard-enter dashboard-enter-delay-1 flex-1 p-4 md:p-5 xl:p-8'>
            {children}
          </main>
        </div>
      </div>
    </div>
  )
}
