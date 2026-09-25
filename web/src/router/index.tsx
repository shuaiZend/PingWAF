import {
  createBrowserRouter,
  Navigate,
  useLocation,
} from 'react-router-dom'
import type { ReactElement } from 'react'
import { AppLayout } from '@/layouts/AppLayout'
import { useAuthStore } from '@/stores/authStore'

import { LoginPage } from '@/pages/LoginPage'
import { DashboardPage } from '@/pages/DashboardPage'
import { SitesListPage } from '@/pages/SitesListPage'
import { SiteDetailPage } from '@/pages/SiteDetailPage'
import { WafPage } from '@/pages/WafPage'
import { RateLimitingPage } from '@/pages/RateLimitingPage'
import { LogsPage } from '@/pages/LogsPage'
import { AgentsPage } from '@/pages/AgentsPage'
import { SettingsPage } from '@/pages/SettingsPage'
import { BotPage as BotProtectionPage } from '@/pages/sites/BotPage'
import { CcProtectionPage } from '@/pages/sites/CcProtectionPage'
import { IpRulesPage } from '@/pages/sites/IpRulesPage'
import { GeoPage } from '@/pages/sites/GeoPage'
import { CachingPage } from '@/pages/sites/CachingPage'
import { SslPage } from '@/pages/sites/SslPage'
import { TrafficPage } from '@/pages/sites/TrafficPage'
import { RewritePage } from '@/pages/sites/RewritePage'
import { ErrorPagesPage } from '@/pages/sites/ErrorPagesPage'
import { SiteSettingsPage } from '@/pages/placeholders'
import { EmptyState } from '@/components/ui/EmptyState'
import { Button } from '@/components/ui/Button'

/** Redirects unauthenticated users to the login page. */
function RequireAuth({ children }: { children: ReactElement }) {
  const isAuthenticated = useAuthStore((s) => s.isAuthenticated)
  const location = useLocation()
  if (!isAuthenticated) {
    return <Navigate to="/login" replace state={{ from: location.pathname }} />
  }
  return children
}

function NotFoundPage() {
  return (
    <EmptyState
      title="Page not found"
      description="The page you are looking for doesn't exist or has been moved."
      action={
        <Button variant="primary" onClick={() => (window.location.href = '/dashboard')}>
          Back to dashboard
        </Button>
      }
    />
  )
}

export const router = createBrowserRouter([
  {
    path: '/login',
    element: <LoginPage />,
  },
  {
    path: '/',
    element: (
      <RequireAuth>
        <AppLayout />
      </RequireAuth>
    ),
    children: [
      { index: true, element: <Navigate to="/dashboard" replace /> },
      { path: 'dashboard', element: <DashboardPage /> },
      { path: 'sites', element: <SitesListPage /> },
      {
        path: 'sites/:siteId',
        element: <SiteDetailPage />,
        children: [
          { index: true, element: <Navigate to="security/waf" replace /> },
          { path: 'security/waf', element: <WafPage /> },
          { path: 'security/rate-limiting', element: <RateLimitingPage /> },
          { path: 'security/bot', element: <BotProtectionPage /> },
          { path: 'security/cc', element: <CcProtectionPage /> },
          { path: 'security/ip-rules', element: <IpRulesPage /> },
          { path: 'security/geo', element: <GeoPage /> },
          { path: 'caching', element: <CachingPage /> },
          { path: 'ssl', element: <SslPage /> },
          { path: 'traffic', element: <TrafficPage /> },
          { path: 'rules/rewrite', element: <RewritePage /> },
          { path: 'rules/error-pages', element: <ErrorPagesPage /> },
          { path: 'settings', element: <SiteSettingsPage /> },
        ],
      },
      { path: 'logs', element: <LogsPage /> },
      { path: 'agents', element: <AgentsPage /> },
      { path: 'settings', element: <SettingsPage /> },
      { path: '*', element: <NotFoundPage /> },
    ],
  },
])

export default router
