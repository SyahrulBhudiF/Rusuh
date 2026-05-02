import type { ReactNode } from 'react'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'

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
      <div className='dashboard-loading dashboard-panel text-muted-foreground rounded-3xl p-8 text-sm'>
        <p className='text-foreground'>Loading…</p>
        <p className='mt-2'>Fetching the latest dashboard data.</p>
      </div>
    )
  }
  if (isError) {
    return (
      <Alert variant='destructive' className='dashboard-enter rounded-3xl p-8 shadow-sm'>
        <AlertTitle className='text-foreground'>Request failed</AlertTitle>
        <AlertDescription className='mt-2'>
          {error?.message ?? 'Try again in a moment.'}
        </AlertDescription>
      </Alert>
    )
  }

  return <>{children}</>
}
