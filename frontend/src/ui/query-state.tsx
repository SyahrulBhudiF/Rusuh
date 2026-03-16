import type { ReactNode } from 'react'

export function QueryState({
  isLoading,
  isError,
  error,
  children,
}: {
  isLoading: boolean
  isError: boolean
  error: Error | null
  children: ReactNode
}) {
  if (isLoading) {
    return (
      <div className='dashboard-loading border-border bg-card/80 text-muted-foreground rounded-3xl border p-8 text-sm shadow-sm'>
        <p className='text-foreground'>Loading…</p>
        <p className='mt-2'>Fetching the latest dashboard data.</p>
      </div>
    )
  }
  if (isError) {
    return (
      <div className='dashboard-enter border-destructive/30 bg-destructive/10 text-destructive dark:text-destructive-foreground rounded-3xl border p-8 text-sm shadow-sm'>
        <p className='text-foreground font-medium'>Request failed</p>
        <p className='mt-2'>{error?.message ?? 'Try again in a moment.'}</p>
      </div>
    )
  }

  return <>{children}</>
}
