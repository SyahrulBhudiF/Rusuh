import * as React from 'react'

import { cn } from '@/lib/utils'

function Textarea({ className, ...props }: React.ComponentProps<'textarea'>) {
  return (
    <textarea
      data-slot='textarea'
      className={cn(
        'border-input placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-ring/50 disabled:bg-input/60 aria-invalid:border-destructive aria-invalid:ring-destructive/20 dark:bg-input/50 dark:disabled:bg-input/80 dark:aria-invalid:border-destructive/50 dark:aria-invalid:ring-destructive/40 flex field-sizing-content min-h-28 w-full rounded-[calc(var(--radius)+4px)] border bg-transparent px-4 py-3 text-base transition-colors outline-none focus-visible:ring-3 disabled:cursor-not-allowed disabled:opacity-50 aria-invalid:ring-3 md:text-sm',
        className,
      )}
      {...props}
    />
  )
}

export { Textarea }
