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
      <header className='dashboard-enter page-hero dashboard-surface relative overflow-hidden rounded-[2rem] p-6 md:rounded-[2.6rem] md:p-8'>
        <div className='flex flex-col gap-5 lg:flex-row lg:items-start lg:justify-between'>
          <div className='max-w-3xl'>
            <p className='text-muted-foreground/90 text-[0.7rem] font-medium tracking-[0.34em] uppercase'>
              {eyebrow}
            </p>
            <h2 className='text-foreground mt-4 max-w-4xl text-[2.3rem] font-semibold tracking-[-0.045em] sm:text-[2.8rem] md:text-[3.1rem]'>
              {title}
            </h2>
            <p className='text-muted-foreground mt-4 max-w-2xl text-sm leading-6 md:text-base'>
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
