import { QueryClientProvider } from '@tanstack/react-query'
import { RouterProvider } from '@tanstack/react-router'
import { StrictMode } from 'react'
import { useEffect } from 'react'
import { createRoot } from 'react-dom/client'

import './index.css'
import { ManagementAuthProvider, useManagementAuth } from './lib/management-auth'
import { createAppQueryClient } from './lib/query-client'
import { useThemeStore } from './lib/theme'
import { router } from './routes'
function AppProviders() {
  const { clearSecret } = useManagementAuth()
  const queryClient = createAppQueryClient(clearSecret)
  const initTheme = useThemeStore((state) => state.initTheme)

  useEffect(() => {
    initTheme()
  }, [initTheme])
  return (
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  )
}
createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ManagementAuthProvider>
      <AppProviders />
    </ManagementAuthProvider>
  </StrictMode>,
)
