import { Link, useRouterState } from '@tanstack/react-router'
import { LaptopMinimal, Moon, Sun } from 'lucide-react'
import { type PropsWithChildren, useState } from 'react'

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
    <div className='dashboard-sidebar-pill inline-flex w-fit max-w-full items-center gap-1 overflow-hidden rounded-full p-1 ring-1 ring-border/60'>
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
              'shrink rounded-full',
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
  const pathname = useRouterState({
    select: (state) => state.location.pathname,
  })
  const overview = useOverviewQuery()
  const managementStatus = useManagementStatusQuery()
  const { clearSecret } = useManagementAuth()
  const [mobileNavOpen, setMobileNavOpen] = useState(false)
  const theme = useThemeStore((state) => state.theme)
  const resolvedTheme = useThemeStore((state) => state.resolvedTheme)
  const setTheme = useThemeStore((state) => state.setTheme)
  const toggleTheme = useThemeStore((state) => state.toggleTheme)

  const cycleTheme = () => {
    if (theme === 'light') {
      setTheme('dark')
      return
    }
    if (theme === 'dark') {
      setTheme('system')
      return
    }
    setTheme('light')
  }

  const providerCount = overview.data?.provider_names.length ?? 0
  return (
    <div className='dashboard-app bg-background text-foreground min-h-screen'>
      <div className='flex min-h-screen w-full gap-6 overflow-x-clip px-0 py-0'>
        <aside className='dashboard-sidebar hidden w-72 flex-col px-5 py-6 md:flex lg:w-80 lg:px-6 lg:py-7'>
          <div>
            <p className='text-muted-foreground/90 text-[0.7rem] font-medium tracking-[0.34em] uppercase'>
              Rusuh Dashboard
            </p>
            <h1 className='mt-3 text-[2rem] font-semibold tracking-[-0.045em] lg:text-[2.2rem]'>
              Control Center
            </h1>
          </div>
          <nav className='mt-10 space-y-2'>
            {navItems.map((item) => {
              const active = pathname === item.to
              return (
                <Link
                  key={item.to}
                  to={item.to}
                  className={cn(
                    'flex min-h-11 w-full items-center rounded-2xl px-4 text-left text-sm font-medium transition-colors',
                    active
                      ? 'bg-primary text-white font-semibold shadow-[0_16px_40px_rgba(12,16,40,0.45)]'
                      : 'text-muted-foreground hover:bg-muted/70 hover:text-foreground',
                  )}
                  activeOptions={{ exact: item.to === '/' }}
                >
                  {item.label}
                </Link>
              )
            })}
          </nav>
          <div className='dashboard-sidebar-footer space-y-3'>
            <div className='text-xs space-y-1'>
              <p className='dashboard-sidebar-meta'>
                {overview.data
                  ? `${overview.data.health.status} · ${overview.data.routing_strategy}`
                  : 'Loading runtime status…'}
              </p>
              <p className='dashboard-sidebar-meta'>
                {managementStatus.data
                  ? `Management port ${managementStatus.data.port}`
                  : managementStatus.isError
                    ? 'Management auth failed'
                    : 'Checking management access…'}
              </p>
              <p className='dashboard-sidebar-meta'>
                Theme: {theme === 'system' ? `system (${resolvedTheme})` : theme}
              </p>
            </div>
            <div className='dashboard-divider' />
            <div className='flex items-center justify-between text-xs'>
              <span className='dashboard-sidebar-meta'>System Status</span>
              <span className='text-emerald-300'>Online</span>
            </div>
            <div className='flex items-center gap-2'>
              <Badge variant='outline' className='rounded-full px-3 py-1 text-xs'>
                {providerCount} provider{providerCount === 1 ? '' : 's'}
              </Badge>
            </div>
            <div className='grid gap-2'>
              <Button
                type='button'
                variant='outline'
                onClick={cycleTheme}
                className='dashboard-sidebar-button w-full'
              >
                Theme ({theme === 'system' ? `system ${resolvedTheme}` : theme})
              </Button>
              <Button
                type='button'
                variant='destructive'
                onClick={clearSecret}
                className='dashboard-sidebar-button w-full'
              >
                Lock
              </Button>
            </div>
          </div>
        </aside>
        <div className='flex min-h-screen flex-1 flex-col px-3 py-4 md:px-6 md:py-6'>
          <div className='dashboard-panel sticky top-3 z-20 mx-3 mt-0 px-4 py-3 backdrop-blur md:hidden'>
            <div className='flex items-center justify-between gap-3'>
              <div>
                <p className='text-muted-foreground text-xs tracking-[0.24em] uppercase'>
                  Rusuh Dashboard
                </p>
                <h1 className='mt-1 text-lg font-semibold'>Control Center</h1>
              </div>
              <div className='flex items-center justify-end gap-2'>
              <Button
                type='button'
                variant='outline'
                size='sm'
                onClick={clearSecret}
                className='rounded-full px-4'
              >
                Lock
              </Button>
              <Button
                type='button'
                variant='outline'
                size='sm'
                onClick={cycleTheme}
                className='rounded-full px-4'
              >
                Theme ({theme === 'system' ? `system ${resolvedTheme}` : theme})
              </Button>
                <Button
                  type='button'
                  size='sm'
                  onClick={() => setMobileNavOpen((value) => !value)}
                  aria-expanded={mobileNavOpen}
                  aria-controls='dashboard-mobile-nav'
                  className='rounded-full px-4'
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
                          'flex min-h-11 items-center rounded-2xl px-4 text-sm font-medium transition-colors',
                          active
                            ? 'bg-primary text-white font-semibold'
                            : 'text-muted-foreground hover:bg-muted/70 hover:text-foreground',
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

          <main className='dashboard-enter dashboard-enter-delay-1 flex-1 px-2 pb-4 pt-4 md:px-4 md:pb-6 md:pt-6 xl:px-6 xl:pt-8'>
            {children}
          </main>
        </div>
      </div>
    </div>
  )
}
