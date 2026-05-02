import { Outlet } from '@tanstack/react-router'

import { ManagementAuthGate } from '../lib/management-auth'
import { DashboardLayout } from './dashboard-layout'

export function RootRouteComponent() {
  return (
    <ManagementAuthGate>
      <DashboardLayout>
        <Outlet />
      </DashboardLayout>
    </ManagementAuthGate>
  )
}
