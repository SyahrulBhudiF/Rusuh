import type { PropsWithChildren, ReactNode } from 'react'

export function PageShell({
  eyebrow,
  title,
  description,
  actions,
  children,
}: PropsWithChildren<{
  eyebrow: string
  title: string
  description: string
  actions?: ReactNode
}>) {
  return (
    <>
      <header className='dashboard-enter border-border bg-card/90 rounded-[1.5rem] border p-5 shadow-sm md:rounded-[1.75rem] md:p-6'>
        <div className='flex flex-col gap-5 lg:flex-row lg:items-start lg:justify-between'>
          <div className='max-w-3xl'>
            <p className='text-muted-foreground text-xs tracking-[0.24em] uppercase'>{eyebrow}</p>
            <h2 className='text-foreground mt-2 text-[1.9rem] font-semibold tracking-[-0.03em] sm:text-[2.1rem] md:text-[2.35rem]'>
              {title}
            </h2>
            <p className='text-muted-foreground mt-3 text-sm leading-6 md:text-base'>
              {description}
            </p>
          </div>
          {actions ? (
            <div className='dashboard-enter dashboard-enter-delay-1 flex w-full flex-col gap-3 sm:flex-row sm:flex-wrap lg:w-auto lg:justify-end lg:self-center'>
              {actions}
            </div>
          ) : null}
        </div>
      </header>
      <section className='dashboard-enter dashboard-enter-delay-2 mt-5 md:mt-6'>{children}</section>
    </>
  )
}
