import * as React from 'react'
import * as RadixUi from '@radix-ui/react-dialog'
import { cn } from '@/lib/utils'

const DialogPrimitive = RadixUi

function Dialog({ ...props }: React.ComponentProps<typeof DialogPrimitive.Root>) {
  return <DialogPrimitive.Root data-slot='dialog' {...props} />
}

function DialogTrigger({
  ...props
}: React.ComponentProps<typeof DialogPrimitive.Trigger>) {
  return <DialogPrimitive.Trigger data-slot='dialog-trigger' {...props} />
}

function DialogPortal({ ...props }: React.ComponentProps<typeof DialogPrimitive.Portal>) {
  return <DialogPrimitive.Portal data-slot='dialog-portal' {...props} />
}

function DialogOverlay({
  className,
  ...props
}: React.ComponentProps<typeof DialogPrimitive.Overlay>) {
  return (
    <DialogPrimitive.Overlay
      data-slot='dialog-overlay'
      className={cn(
        'data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0 fixed inset-0 z-50 bg-black/10 duration-100 supports-backdrop-filter:backdrop-blur-xs',
        className,
      )}
      {...props}
    />
  )
}

function DialogContent({
  className,
  size = 'default',
  ...props
}: React.ComponentProps<typeof DialogPrimitive.Content> & {
  size?: 'default' | 'sm' | 'lg'
}) {
  return (
    <DialogPortal>
      <DialogOverlay />
      <DialogPrimitive.Content
        data-slot='dialog-content'
        data-size={size}
        className={cn(
          'group/dialog-content bg-background ring-foreground/10 data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95 fixed top-1/2 left-1/2 z-50 grid w-full -translate-x-1/2 -translate-y-1/2 gap-4 rounded-xl p-4 ring-1 duration-100 outline-none data-[size=default]:max-w-xs data-[size=sm]:max-w-sm data-[size=lg]:max-w-lg',
          className,
        )}
        {...props}
      />
    </DialogPortal>
  )
}

function DialogHeader({ className, ...props }: React.ComponentProps<'div'>) {
  return (
    <div
      data-slot='dialog-header'
      className={cn(
        'grid grid-rows-[auto_1fr] place-items-center gap-1.5 text-center has-data-[slot=dialog-media]:grid-rows-[auto_auto_1fr] has-data-[slot=dialog-media]:gap-x-4 sm:group-data-[size=default]/dialog-content:place-items-start sm:group-data-[size=default]/dialog-content:text-left sm:group-data-[size=default]/dialog-content:has-data-[slot=dialog-media]:grid-rows-[auto_1fr]',
        className,
      )}
      {...props}
    />
  )
}

function DialogFooter({ className, ...props }: React.ComponentProps<'div'>) {
  return (
    <div
      data-slot='dialog-footer'
      className={cn(
        'bg-muted/50 -mx-4 -mb-4 flex flex-col-reverse gap-2 rounded-b-xl border-t p-4 group-data-[size=sm]/dialog-content:grid group-data-[size=sm]/dialog-content:grid-cols-2 sm:flex-row sm:justify-end',
        className,
      )}
      {...props}
    />
  )
}

function DialogTitle({
  className,
  ...props
}: React.ComponentProps<typeof DialogPrimitive.Title>) {
  return (
    <DialogPrimitive.Title
      data-slot='dialog-title'
      className={cn(
        'text-base font-medium sm:group-data-[size=default]/dialog-content:group-has-data-[slot=dialog-media]/dialog-content:col-start-2',
        className,
      )}
      {...props}
    />
  )
}

function DialogDescription({
  className,
  ...props
}: React.ComponentProps<typeof DialogPrimitive.Description>) {
  return (
    <DialogPrimitive.Description
      data-slot='dialog-description'
      className={cn(
        'text-muted-foreground *:[a]:hover:text-foreground text-sm text-balance md:text-pretty *:[a]:underline *:[a]:underline-offset-3',
        className,
      )}
      {...props}
    />
  )
}

export {
  Dialog,
  DialogTrigger,
  DialogPortal,
  DialogOverlay,
  DialogContent,
  DialogHeader,
  DialogFooter,
  DialogTitle,
  DialogDescription,
}