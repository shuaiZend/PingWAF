import { useEffect } from 'react'
import { NavLink, Outlet, useParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import {
  Globe,
  Shield,
  Lightning,
  Lock,
  ChartLine,
  Sliders,
  Gauge,
  CaretLeft,
  Robot,
  Cloud,
  IdentificationCard,
  GlobeHemisphereWest,
  FileHtml,
} from '@phosphor-icons/react'
import { Link } from 'react-router-dom'
import { cn } from '@/lib/utils'
import { Badge } from '@/components/ui/Badge'
import { Skeleton } from '@/components/ui/Skeleton'
import { ErrorState } from '@/components/ErrorState'
import { useSiteDetail } from '@/hooks'
import { useSiteStore } from '@/stores/siteStore'
import type { SiteStatus } from '@/api/types'

const STATUS_TONE: Record<SiteStatus, 'success' | 'warning' | 'neutral'> = {
  active: 'success',
  paused: 'warning',
  pending: 'neutral',
}

export function SiteDetailPage() {
  const { t } = useTranslation()
  const { siteId } = useParams<{ siteId: string }>()
  const setCurrentSiteId = useSiteStore((s) => s.setCurrentSiteId)

  useEffect(() => {
    if (siteId) setCurrentSiteId(siteId)
  }, [siteId, setCurrentSiteId])

  const detail = useSiteDetail(siteId)
  const site = detail.data?.site

  const base = `/sites/${siteId}`
  const tabs = [
    { to: `${base}/security/waf`, label: t('nav.securityWaf'), icon: Shield },
    {
      to: `${base}/security/rate-limiting`,
      label: t('nav.securityRateLimiting'),
      icon: Gauge,
    },
    { to: `${base}/security/bot`, label: t('nav.securityBot'), icon: Robot },
    { to: `${base}/security/cc`, label: t('nav.securityCc'), icon: Cloud },
    { to: `${base}/security/ip-rules`, label: t('nav.securityIpRules'), icon: IdentificationCard },
    { to: `${base}/security/geo`, label: t('nav.securityGeo'), icon: GlobeHemisphereWest },
    { to: `${base}/caching`, label: t('nav.caching'), icon: Lightning },
    { to: `${base}/ssl`, label: t('nav.ssl'), icon: Lock },
    { to: `${base}/traffic`, label: t('nav.traffic'), icon: ChartLine },
    { to: `${base}/rules/rewrite`, label: t('pages.rewrite.title'), icon: Sliders },
    { to: `${base}/rules/error-pages`, label: t('pages.errorPages.title'), icon: FileHtml },
  ]

  // The server answers 404 for sites the caller may not see — surface that
  // instead of rendering an empty shell around a broken id.
  if (detail.isError && !detail.data) {
    return (
      <div className="animate-slide-up">
        <ErrorState
          error={detail.error}
          onRetry={() => detail.refetch()}
          retrying={detail.isFetching}
          action={
            <Link to="/sites">
              <span className="inline-flex items-center gap-1.5 text-[13px] font-medium text-link">
                <CaretLeft weight="bold" className="h-3.5 w-3.5" />
                {t('pages.sites.title')}
              </span>
            </Link>
          }
        />
      </div>
    )
  }

  return (
    <div className="animate-slide-up">
      {/* Site header */}
      <div className="mb-5 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex min-w-0 items-center gap-3">
          <span className="flex h-11 w-11 shrink-0 items-center justify-center rounded-lg bg-brand-soft text-brand">
            <Globe weight="duotone" className="h-6 w-6" />
          </span>
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              {site ? (
                <h1 className="pw-mono truncate text-lg font-semibold tracking-tight text-fg-strong">
                  {site.domain}
                </h1>
              ) : (
                <Skeleton className="h-6 w-56 rounded-md" />
              )}
              {site && (
                <Badge tone={STATUS_TONE[site.status as SiteStatus] ?? 'neutral'} dot>
                  {t(`status.${site.status}`, site.status)}
                </Badge>
              )}
            </div>
            <p className="truncate text-[13px] text-fg-subtle">
              {site ? (
                <>
                  {site.name}
                  <span className="mx-1.5 text-fg-subtle/50">·</span>
                  <span className="capitalize">{t(`plans.${site.plan}`, site.plan)}</span>
                </>
              ) : (
                t('pages.siteDetail.overview')
              )}
            </p>
          </div>
        </div>

        {/* Live counters straight from SiteDetail */}
        {detail.data && (
          <div className="flex shrink-0 items-center gap-4 text-[13px]">
            <Counter
              label={t('nav.securityWaf')}
              value={detail.data.rule_count}
            />
            <Counter
              label={t('nav.securityRateLimiting')}
              value={detail.data.rate_limit_count}
            />
            <Counter label={t('nav.ssl')} value={detail.data.ssl ? 1 : 0} />
            <Counter label={t('pages.siteDetail.upstreams')} value={detail.data.upstreams.length} />
          </div>
        )}
      </div>

      {/* Tab nav */}
      <div className="mb-6 flex gap-1 overflow-x-auto border-b border-line">
        {tabs.map((tab) => {
          const Icon = tab.icon
          return (
            <NavLink
              key={tab.to}
              to={tab.to}
              className={({ isActive }) =>
                cn(
                  '-mb-px inline-flex shrink-0 items-center gap-2 border-b-2 px-3 py-2.5 text-sm font-medium transition-colors',
                  isActive
                    ? 'border-brand text-fg-strong'
                    : 'border-transparent text-fg-subtle hover:text-fg',
                )
              }
            >
              <Icon weight="duotone" className="h-4 w-4" />
              {tab.label}
            </NavLink>
          )
        })}
      </div>

      <Outlet />
    </div>
  )
}

function Counter({ label, value }: { label: string; value: number }) {
  return (
    <div className="text-center">
      <p className="tabular-nums text-base font-semibold leading-tight text-fg-strong">
        {value}
      </p>
      <p className="text-[11px] leading-tight text-fg-subtle">{label}</p>
    </div>
  )
}
