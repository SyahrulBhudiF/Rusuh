import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type FormEvent,
  type PropsWithChildren,
} from 'react'

const STORAGE_KEY = 'rusuh.management-secret'

type ManagementAuthContextValue = {
  secret: string
  isUnlocked: boolean
  setSecret: (value: string, persist?: boolean) => void
  clearSecret: () => void
}

const ManagementAuthContext = createContext<ManagementAuthContextValue | null>(null)

export function ManagementAuthProvider({ children }: PropsWithChildren) {
  const [secret, setSecretState] = useState('')

  useEffect(() => {
    const saved = window.sessionStorage.getItem(STORAGE_KEY)
    if (saved) {
      setSecretState(saved)
    }
  }, [])

  const setSecret = useCallback((value: string, persist = true) => {
    setSecretState(value)

    if (persist) {
      window.sessionStorage.setItem(STORAGE_KEY, value)
    }
  }, [])

  const clearSecret = useCallback(() => {
    setSecretState('')
    window.sessionStorage.removeItem(STORAGE_KEY)
  }, [])

  const value = useMemo(
    () => ({
      secret,
      isUnlocked: secret.trim().length > 0,
      setSecret,
      clearSecret,
    }),
    [clearSecret, secret, setSecret],
  )

  return <ManagementAuthContext.Provider value={value}>{children}</ManagementAuthContext.Provider>
}

export function useManagementAuth() {
  const context = useContext(ManagementAuthContext)

  if (!context) {
    throw new Error('useManagementAuth must be used inside ManagementAuthProvider')
  }

  return context
}

export function ManagementAuthGate({ children }: PropsWithChildren) {
  const { clearSecret, isUnlocked, setSecret } = useManagementAuth()
  const [value, setValue] = useState('')
  const [persist, setPersist] = useState(true)

  if (isUnlocked) {
    return <>{children}</>
  }

  function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const nextValue = value.trim()
    if (!nextValue) return
    clearSecret()
    setSecret(nextValue, persist)
  }

  return (
    <div className='flex min-h-screen items-center justify-center p-6 text-[var(--foreground)]'>
      <div className='w-full max-w-md rounded-3xl border border-[var(--border)] bg-[var(--card)] p-6 shadow-[0_24px_80px_rgba(0,0,0,0.45)]'>
        <p className='text-xs tracking-[0.24em] text-[var(--muted-foreground)] uppercase'>
          Dashboard Auth
        </p>
        <h1 className='mt-2 text-2xl font-semibold'>Enter management secret</h1>
        <p className='mt-3 text-sm text-[var(--muted-foreground)]'>
          Needed for `/v0/management/*` mutations. Stored in session storage for this tab only.
        </p>

        <form className='mt-6 space-y-4' onSubmit={onSubmit}>
          <label className='block space-y-2'>
            <span className='text-sm text-[var(--muted-foreground)]'>Secret</span>
            <input
              type='password'
              value={value}
              onChange={(event) => setValue(event.target.value)}
              className='w-full rounded-2xl border border-[var(--border)] bg-black/20 px-4 py-3 text-white ring-0 outline-none placeholder:text-zinc-500 focus:border-blue-500/50'
              placeholder='sk-...'
            />
          </label>

          <label className='flex items-center gap-3 text-sm text-[var(--muted-foreground)]'>
            <input
              type='checkbox'
              checked={persist}
              onChange={(event) => setPersist(event.target.checked)}
            />
            Keep for this tab session
          </label>

          <button
            type='submit'
            className='w-full rounded-2xl bg-blue-600 px-4 py-3 font-medium text-white hover:bg-blue-500'
          >
            Unlock dashboard
          </button>
        </form>
      </div>
    </div>
  )
}
