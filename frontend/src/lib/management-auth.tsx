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

import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'

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
    <div className='bg-background text-foreground flex min-h-screen items-center justify-center p-4 sm:p-6'>
      <Card className='w-full max-w-md rounded-3xl shadow-lg'>
        <CardContent className='p-5 sm:p-6'>
          <p className='text-muted-foreground text-xs tracking-[0.24em] uppercase'>
            Dashboard Auth
          </p>
          <h1 className='mt-2 text-2xl font-semibold tracking-[-0.02em]'>
            Enter management secret
          </h1>
          <p className='text-muted-foreground mt-3 text-sm leading-6'>
            Needed for `/v0/management/*` mutations. Stored in session storage for this tab only.
          </p>
          <form className='mt-6 space-y-4' onSubmit={onSubmit}>
            <label className='block space-y-2'>
              <span className='text-muted-foreground text-sm'>Secret</span>
              <Input
                type='password'
                value={value}
                onChange={(event) => setValue(event.target.value)}
                className='h-11 rounded-2xl'
                placeholder='sk-...'
              />
            </label>

            <div className='border-border bg-background/50 rounded-2xl border p-3'>
              <label className='text-muted-foreground flex items-start gap-3 text-sm leading-6'>
                <input
                  type='checkbox'
                  checked={persist}
                  onChange={(event) => setPersist(event.target.checked)}
                  className='mt-1 h-4 w-4 shrink-0 accent-current'
                />
                <span>
                  Keep for this tab session
                  <span className='text-muted-foreground/80 block text-xs'>
                    Auto-clears when the tab session ends.
                  </span>
                </span>
              </label>
            </div>
            <Button type='submit' className='h-11 w-full rounded-2xl'>
              Unlock dashboard
            </Button>
          </form>
        </CardContent>
      </Card>
    </div>
  )
}
