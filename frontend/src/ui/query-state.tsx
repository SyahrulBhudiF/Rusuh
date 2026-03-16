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
      <div className='dashboard-loading rounded-3xl border border-[var(--border)] bg-[var(--card)]/60 p-8 text-sm text-[var(--muted-foreground)]'>
        <p className='text-white'>Loading…</p>
        <p className='mt-2'>Fetching the latest dashboard data.</p>
      </div>
    )
  }

  if (isError) {
    return (
      <div className='dashboard-enter rounded-3xl border border-red-500/30 bg-red-500/10 p-8 text-sm text-red-100'>
        <p className='font-medium text-white'>Request failed</p>
        <p className='mt-2'>{error?.message ?? 'Try again in a moment.'}</p>
      </div>
    )
  }

  return <>{children}</>
}
