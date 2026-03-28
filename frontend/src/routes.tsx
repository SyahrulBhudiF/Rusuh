import { createRootRoute, createRoute, createRouter } from '@tanstack/react-router'

import { AccountsPage } from './ui/pages/accounts-page'
import { AddAccountPage } from './ui/pages/add-account-page'
import { ApiKeysPage } from './ui/pages/api-keys-page'
import { ConfigPage } from './ui/pages/config-page'
import { OverviewPage } from './ui/pages/overview-page'
import { RootRouteComponent } from './ui/root-route'

const rootRoute = createRootRoute({
  component: RootRouteComponent,
})

const overviewRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/',
  component: OverviewPage,
})

const accountsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/accounts',
  component: AccountsPage,
})

const addAccountRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/accounts/add',
  component: AddAccountPage,
})

const apiKeysRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/api-keys',
  component: ApiKeysPage,
})

const configRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/config',
  component: ConfigPage,
})

const routeTree = rootRoute.addChildren([
  overviewRoute,
  addAccountRoute,
  accountsRoute,
  apiKeysRoute,
  configRoute,
])

export const router = createRouter({
  routeTree,
  defaultPreload: 'intent',
  defaultPendingMinMs: 0,
})

declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router
  }
}
