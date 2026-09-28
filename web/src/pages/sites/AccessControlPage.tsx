import { Navigate, useLocation, useParams, useSearchParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { FunnelSimple, GlobeHemisphereWest } from '@phosphor-icons/react'
import { PageHeader } from '@/components/PageHeader'
import { Tabs } from '@/components/ui/Tabs'
import { IpRulesPanel } from './IpRulesPanel'
import { GeoPanel } from './GeoPanel'

type AccessTab = 'ip' | 'geo'

/**
 * Access restrictions for one site, as one tab with a module per mechanism.
 *
 * IP rules and geo restrictions are the first two modules; credential-based
 * restrictions (basic auth, OAuth) will land here as further tabs rather than
 * as new top-level site tabs.
 */
export function AccessControlPage() {
  const { t } = useTranslation()
  const [searchParams, setSearchParams] = useSearchParams()
  const tab: AccessTab = searchParams.get('tab') === 'geo' ? 'geo' : 'ip'

  const setTab = (next: string) => {
    const params = new URLSearchParams(searchParams)
    params.set('tab', next)
    setSearchParams(params, { replace: true })
  }

  return (
    <div className="animate-slide-up">
      <PageHeader
        title={t('pages.accessControl.title')}
        description={t('pages.accessControl.description')}
      />

      <Tabs
        variant="pill"
        className="mb-5"
        value={tab}
        onChange={setTab}
        items={[
          {
            value: 'ip',
            label: t('pages.accessControl.tabIpRules'),
            icon: <FunnelSimple weight="duotone" className="h-4 w-4" />,
          },
          {
            value: 'geo',
            label: t('pages.accessControl.tabGeo'),
            icon: <GlobeHemisphereWest weight="duotone" className="h-4 w-4" />,
          },
        ]}
      />

      {tab === 'ip' ? <IpRulesPanel /> : <GeoPanel />}
    </div>
  )
}

/** Forwards a pre-merge route to the matching module here, query intact. */
export function LegacySecurityRedirect({ tab }: { tab: AccessTab }) {
  const { siteId = '' } = useParams<{ siteId: string }>()
  const location = useLocation()
  const params = new URLSearchParams(location.search)
  params.set('tab', tab)
  return (
    <Navigate to={`/sites/${siteId}/security/access?${params.toString()}`} replace />
  )
}

export default AccessControlPage
