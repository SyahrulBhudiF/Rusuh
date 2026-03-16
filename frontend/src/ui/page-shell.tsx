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
      <header className='dashboard-enter rounded-[1.9rem] border border-[var(--border)] bg-[color:rgba(34,27,31,0.82)] p-5 shadow-[0_18px_50px_rgba(0,0,0,0.22)] md:p-6'>
        <div className='flex flex-col gap-4 md:flex-row md:items-start md:justify-between'>
          <div>
            <p className='text-xs tracking-[0.24em] text-[var(--muted-foreground)] uppercase'>
              {eyebrow}
            </p>
            <h2 className='mt-2 text-[2.15rem] font-semibold tracking-[-0.03em] text-white'>
              {title}
            </h2>
            <p className='mt-3 max-w-2xl text-sm leading-6 text-[var(--muted-foreground)] md:text-base'>
              {description}
            </p>
          </div>
          {actions ? (
            <div className='dashboard-enter dashboard-enter-delay-1 flex flex-col gap-3 sm:flex-row sm:flex-wrap sm:justify-end'>
              {actions}
            </div>
          ) : null}
        </div>
      </header>

      <section className='dashboard-enter dashboard-enter-delay-2 mt-6'>{children}</section>
    </>
  )
}
