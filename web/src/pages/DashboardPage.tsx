import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useQuery } from '@tanstack/react-query'
import { Link } from 'react-router-dom'
import {
  AreaChart,
  Area,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  Legend,
} from 'recharts'
import {
  Globe,
  Desktop,
  ArrowsDownUp,
  ShieldSlash,
  Gauge,
  Database,
  ArrowRight,
  Lightning,
  Clock,
} from '@phosphor-icons/react'
import { PageHeader } from '@/components/PageHeader'
import { Card, CardBody, CardHeader } from '@/components/ui/Card'
import { Badge } from '@/components/ui/Badge'
import { Button } from '@/components/ui/Button'
import { Select } from '@/components/ui/Select'
import { EmptyState } from '@/components/ui/EmptyState'
import { Table, type Column } from '@/components/ui/Table'
import { ErrorState } from '@/components/ErrorState'
import { Skeleton, SkeletonRows, SkeletonStat } from '@/components/ui/Skeleton'
import { analyticsApi, analyticsKeys } from '@/api/analytics'
import { agentsApi, agentKeys } from '@/api/agents'
import { useSitesList } from '@/hooks'
import { cn } from '@/lib/utils'
import {
  formatBucket,
  formatCompactNumber,
  formatLatency,
  formatNumber,
  formatPercent,
  formatRelative,
} from '@/lib/format'
import type { RangeQuery, TopIp, TopRule } from '@/api/types'

/** Preset windows, in hours. The API caps a single range at 31 days. */
const RANGE_OPTIONS = [
  { value: '1', label: '1h' },
  { value: '6', label: '6h' },
  { value: '24', label: '24h' },
  { value: '72', label: '3d' },
  { value: '168', label: '7d' },
  { value: '720', label: '30d' },
]

/** 30s polling cadence, matching the dashboard's "live" promise. */
const REFRESH_MS = 30_000

function intervalFor(hours: number): 'minute' | 'hour' | 'day' {
  if (hours <= 2) return 'minute'
  if (hours <= 96) return 'hour'
  return 'day'
}

export function DashboardPage() {
  const { t } = useTranslation()
  const [hours, setHours] = useState(24)
  const [siteId, setSiteId] = useState<string>('')

  const { data: sites } = useSitesList()

  // Recomputed only when the window or the site filter changes, so the 30s
  // refetch keeps a stable query key and does not invalidate the cache each tick.
  const range = useMemo<RangeQuery>(() => {
    const to = new Date()
    const from = new Date(to.getTime() - hours * 3600_000)
    return {
      from: from.toISOString(),
      to: to.toISOString(),
      interval: intervalFor(hours),
      ...(siteId ? { site_id: siteId } : {}),
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hours, siteId])

  const summary = useQuery({
    queryKey: analyticsKeys.summary(range),
    queryFn: () => analyticsApi.summary(range),
    refetchInterval: REFRESH_MS,
  })

  const traffic = useQuery({
    queryKey: analyticsKeys.traffic(range),
    queryFn: () => analyticsApi.requestsOverTime(range),
    refetchInterval: REFRESH_MS,
  })

  const topIps = useQuery({
    queryKey: analyticsKeys.topIps(range),
    queryFn: () => analyticsApi.topIps({ ...range, limit: 8 }),
    refetchInterval: REFRESH_MS,
  })

  const topRules = useQuery({
    queryKey: analyticsKeys.topRules(range),
    queryFn: () => analyticsApi.topRules({ ...range, limit: 6 }),
    refetchInterval: REFRESH_MS,
  })

  const topSites = useQuery({
    queryKey: analyticsKeys.sites(range),
    queryFn: () => analyticsApi.sitesOverview(range),
    refetchInterval: REFRESH_MS,
    enabled: !siteId,
  })

  const agents = useQuery({
    queryKey: agentKeys.list(),
    queryFn: () => agentsApi.list(),
    select: (page) => page.items,
    refetchInterval: REFRESH_MS,
  })

  const siteOptions = useMemo(
    () => [
      { value: '', label: t('pages.dashboard.allSites') },
      ...(sites ?? []).map((s) => ({ value: s.id, label: s.domain })),
    ],
    [sites, t],
  )

  const spanMs = hours * 3600_000
  const chartData = useMemo(
    () =>
      (traffic.data ?? []).map((b) => ({
        label: formatBucket(b.bucket, spanMs),
        requests: b.requests,
        blocked: b.blocked,
        cache_hits: b.cache_hits,
        errors: b.client_errors + b.server_errors,
      })),
    [traffic.data, spanMs],
  )

  const s = summary.data
  const onlineAgents = (agents.data ?? []).filter((a) => a.status === 'online').length
  const totalAgents = agents.data?.length ?? 0

  const stats = [
    {
      key: 'requests',
      label: t('pages.dashboard.requestsWindow'),
      value: s ? formatCompactNumber(s.requests) : undefined,
      sub: s ? t('pages.dashboard.uniqueIps', { count: formatCompactNumber(s.unique_ips) }) : undefined,
      icon: ArrowsDownUp,
      tone: 'text-link bg-focus/10',
    },
    {
      key: 'blocked',
      label: t('pages.dashboard.blockedWindow'),
      value: s ? formatCompactNumber(s.blocked_requests) : undefined,
      sub: s
        ? t('pages.dashboard.attackers', { count: formatCompactNumber(s.distinct_attackers) })
        : undefined,
      icon: ShieldSlash,
      tone: 'text-fg-danger bg-danger/12',
    },
    {
      key: 'security',
      label: t('pages.dashboard.securityEvents'),
      value: s ? formatCompactNumber(s.security_events) : undefined,
      sub: s
        ? t('pages.dashboard.rulesTriggered', { count: formatCompactNumber(s.rules_triggered) })
        : undefined,
      icon: Lightning,
      tone: 'text-brand bg-brand-soft',
    },
    {
      key: 'agents',
      label: t('pages.dashboard.activeAgents'),
      value: agents.data ? `${onlineAgents}/${totalAgents}` : undefined,
      sub: agents.data
        ? t('pages.dashboard.agentsOnline', { count: onlineAgents })
        : undefined,
      icon: Desktop,
      tone: 'text-fg-success bg-success/12',
    },
    {
      key: 'cache',
      label: t('pages.dashboard.cacheHitRate'),
      value: s ? formatPercent(s.cache_hit_rate) : undefined,
      sub: s ? `${formatCompactNumber(s.cache_hits)} ${t('nav.caching')}` : undefined,
      icon: Database,
      tone: 'text-fg-success bg-success/12',
    },
    {
      key: 'latency',
      label: t('pages.dashboard.avgLatency'),
      value: s ? formatLatency(s.avg_latency_ms) : undefined,
      sub: s
        ? `${t('pages.dashboard.errors')}: ${formatCompactNumber(s.client_errors + s.server_errors)}`
        : undefined,
      icon: Gauge,
      tone: 'text-fg bg-recessed',
    },
  ]

  const ipColumns: Column<TopIp>[] = [
    {
      key: 'client_ip',
      header: t('pages.logs.clientIp'),
      accessor: (r) => r.client_ip,
      cell: (r) => <span className="pw-mono text-[13px] text-fg">{r.client_ip}</span>,
    },
    {
      key: 'country',
      header: t('pages.dashboard.country'),
      accessor: (r) => r.country_code ?? '',
      cell: (r) =>
        r.country_code ? (
          <span className="text-[13px] text-fg-subtle">{r.country_code.toUpperCase()}</span>
        ) : (
          <span className="text-fg-subtle">—</span>
        ),
    },
    {
      key: 'requests',
      header: t('pages.dashboard.hits'),
      accessor: (r) => r.requests,
      align: 'right',
      sortable: true,
      cell: (r) => <span className="tabular-nums">{formatNumber(r.requests)}</span>,
    },
    {
      key: 'blocked',
      header: t('pages.dashboard.blocked'),
      accessor: (r) => r.blocked,
      align: 'right',
      sortable: true,
      cell: (r) => (
        <span className={cn('tabular-nums', r.blocked > 0 && 'font-medium text-fg-danger')}>
          {formatNumber(r.blocked)}
        </span>
      ),
    },
  ]

  const ruleColumns: Column<TopRule>[] = [
    {
      key: 'rule_name',
      header: t('common.name'),
      accessor: (r) => r.rule_name,
      cell: (r) => (
        <div className="min-w-0">
          <p className="truncate text-[13px] font-medium text-fg">{r.rule_name || r.rule_id}</p>
          <p className="pw-mono truncate text-xs text-fg-subtle">{r.rule_id}</p>
        </div>
      ),
    },
    {
      key: 'action',
      header: t('pages.waf.action'),
      accessor: (r) => r.action,
      cell: (r) => (
        <Badge tone={r.action === 'block' ? 'danger' : r.action === 'log' ? 'info' : 'warning'}>
          {t(`actions.${r.action}`, r.action)}
        </Badge>
      ),
    },
    {
      key: 'hits',
      header: t('pages.dashboard.hits'),
      accessor: (r) => r.hits,
      align: 'right',
      sortable: true,
      cell: (r) => <span className="tabular-nums">{formatNumber(r.hits)}</span>,
    },
    {
      key: 'unique_ips',
      header: t('pages.dashboard.uniqueIpsShort'),
      accessor: (r) => r.unique_ips,
      align: 'right',
      cell: (r) => <span className="tabular-nums">{formatNumber(r.unique_ips)}</span>,
    },
  ]

  return (
    <div className="animate-slide-up">
      <PageHeader
        title={t('pages.dashboard.title')}
        description={t('pages.dashboard.description')}
        actions={
          <div className="flex items-center gap-2">
            <span className="hidden items-center gap-1.5 text-xs text-fg-subtle sm:inline-flex">
              <Clock weight="duotone" className="h-3.5 w-3.5" />
              {t('pages.dashboard.autoRefresh')}
            </span>
            <Select
              aria-label={t('pages.dashboard.siteFilter')}
              className="h-9 w-44"
              value={siteId}
              options={siteOptions}
              onChange={(e) => setSiteId(e.target.value)}
            />
            <Select
              aria-label={t('pages.dashboard.timeRange')}
              className="h-9 w-24"
              value={String(hours)}
              options={RANGE_OPTIONS}
              onChange={(e) => setHours(Number(e.target.value))}
            />
            <Button
              variant="secondary"
              size="md"
              loading={summary.isFetching}
              onClick={() => {
                void summary.refetch()
                void traffic.refetch()
                void topIps.refetch()
                void topRules.refetch()
              }}
            >
              {t('common.refresh')}
            </Button>
          </div>
        }
      />

      {/* Stat cards */}
      {summary.isError && !summary.data ? (
        <ErrorState error={summary.error} onRetry={() => summary.refetch()} retrying={summary.isFetching} />
      ) : (
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-3 2xl:grid-cols-6">
          {stats.map((stat) => {
            const Icon = stat.icon
            return (
              <Card key={stat.key} className="p-4">
                <div className="flex items-start justify-between">
                  <span className={cn('flex h-9 w-9 items-center justify-center rounded-lg', stat.tone)}>
                    <Icon weight="duotone" className="h-[18px] w-[18px]" />
                  </span>
                </div>
                {stat.value === undefined ? (
                  <div className="mt-3">
                    <Skeleton className="h-7 w-20" />
                    <Skeleton className="mt-2 h-3 w-24" />
                  </div>
                ) : (
                  <>
                    <p className="mt-3 text-2xl font-semibold tracking-tight text-fg-strong tabular-nums">
                      {stat.value}
                    </p>
                    <p className="mt-0.5 text-[13px] text-fg-subtle">{stat.label}</p>
                    {stat.sub && (
                      <p className="mt-1 text-xs text-fg-subtle/80 tabular-nums">{stat.sub}</p>
                    )}
                  </>
                )}
              </Card>
            )
          })}
        </div>
      )}

      {/* Traffic chart */}
      <Card className="mt-4">
        <CardHeader
          title={t('pages.dashboard.trafficOverview')}
          description={t('pages.dashboard.trafficOverviewDescription', { hours })}
          action={
            summary.data ? (
              <span className="text-xs text-fg-subtle tabular-nums">
                {formatNumber(summary.data.requests)} {t('pages.traffic.requests')}
              </span>
            ) : undefined
          }
        />
        <CardBody>
          {traffic.isPending ? (
            <div className="flex h-72 items-end gap-1.5">
              {Array.from({ length: 24 }).map((_, i) => (
                <Skeleton
                  key={i}
                  className="flex-1"
                  style={{ height: `${28 + ((i * 37) % 60)}%` }}
                />
              ))}
            </div>
          ) : traffic.isError ? (
            <ErrorState
              variant="inline"
              error={traffic.error}
              onRetry={() => traffic.refetch()}
              retrying={traffic.isFetching}
            />
          ) : chartData.length === 0 ? (
            <EmptyState
              icon={<ArrowsDownUp weight="duotone" className="h-8 w-8" />}
              title={t('pages.dashboard.noTraffic')}
              description={t('pages.dashboard.noTrafficDescription')}
              className="py-10"
            />
          ) : (
            <div className="h-72 w-full">
              <ResponsiveContainer width="100%" height="100%">
                <AreaChart data={chartData} margin={{ top: 8, right: 8, left: -16, bottom: 0 }}>
                  <defs>
                    <linearGradient id="reqFill" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="0%" stopColor="#f6821f" stopOpacity={0.35} />
                      <stop offset="100%" stopColor="#f6821f" stopOpacity={0} />
                    </linearGradient>
                    <linearGradient id="blockFill" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="0%" stopColor="#e54d2a" stopOpacity={0.3} />
                      <stop offset="100%" stopColor="#e54d2a" stopOpacity={0} />
                    </linearGradient>
                  </defs>
                  <CartesianGrid strokeDasharray="3 3" stroke="var(--color-border-line)" vertical={false} />
                  <XAxis
                    dataKey="label"
                    tick={{ fontSize: 11, fill: 'var(--color-text-subtle)' }}
                    tickLine={false}
                    axisLine={{ stroke: 'var(--color-border-line)' }}
                    minTickGap={24}
                  />
                  <YAxis
                    tick={{ fontSize: 11, fill: 'var(--color-text-subtle)' }}
                    tickLine={false}
                    axisLine={false}
                    tickFormatter={(v: number) => formatCompactNumber(v)}
                  />
                  <Tooltip
                    contentStyle={{
                      background: 'var(--color-bg-elevated)',
                      border: '1px solid var(--color-border-line)',
                      borderRadius: 8,
                      fontSize: 12,
                      color: 'var(--color-text-default)',
                    }}
                    formatter={(value) => formatNumber(Number(value ?? 0))}
                  />
                  <Legend wrapperStyle={{ fontSize: 12 }} iconType="circle" />
                  <Area
                    type="monotone"
                    dataKey="requests"
                    name={t('pages.traffic.requests')}
                    stroke="#f6821f"
                    strokeWidth={2}
                    fill="url(#reqFill)"
                  />
                  <Area
                    type="monotone"
                    dataKey="blocked"
                    name={t('pages.dashboard.blockedShort')}
                    stroke="#e54d2a"
                    strokeWidth={2}
                    fill="url(#blockFill)"
                  />
                  <Area
                    type="monotone"
                    dataKey="cache_hits"
                    name={t('nav.caching')}
                    stroke="#18794e"
                    strokeWidth={2}
                    fill="transparent"
                  />
                </AreaChart>
              </ResponsiveContainer>
            </div>
          )}
        </CardBody>
      </Card>

      <div className="mt-4 grid grid-cols-1 gap-4 xl:grid-cols-2">
        {/* Top threat IPs */}
        <Card>
          <CardHeader
            title={t('pages.dashboard.topThreatIps')}
            description={t('pages.dashboard.topThreatIpsDescription')}
            action={
              <Link to="/logs">
                <Button variant="ghost" size="sm" icon={<ArrowRight weight="bold" className="h-3.5 w-3.5" />}>
                  {t('common.viewAll')}
                </Button>
              </Link>
            }
          />
          <CardBody className="px-0 py-0">
            {topIps.isPending ? (
              <SkeletonRows rows={5} columns={4} />
            ) : topIps.isError ? (
              <div className="p-4">
                <ErrorState variant="inline" error={topIps.error} onRetry={() => topIps.refetch()} />
              </div>
            ) : (topIps.data ?? []).length === 0 ? (
              <EmptyState
                className="py-10"
                icon={<ShieldSlash weight="duotone" className="h-7 w-7" />}
                title={t('pages.dashboard.noThreats')}
                description={t('pages.dashboard.noThreatsDescription')}
              />
            ) : (
              <Table columns={ipColumns} data={topIps.data ?? []} rowKey={(r) => r.client_ip} dense />
            )}
          </CardBody>
        </Card>

        {/* Top triggered rules */}
        <Card>
          <CardHeader
            title={t('pages.dashboard.topRules')}
            description={t('pages.dashboard.topRulesDescription')}
          />
          <CardBody className="px-0 py-0">
            {topRules.isPending ? (
              <SkeletonRows rows={5} columns={4} />
            ) : topRules.isError ? (
              <div className="p-4">
                <ErrorState variant="inline" error={topRules.error} onRetry={() => topRules.refetch()} />
              </div>
            ) : (topRules.data ?? []).length === 0 ? (
              <EmptyState
                className="py-10"
                icon={<Lightning weight="duotone" className="h-7 w-7" />}
                title={t('pages.dashboard.noRulesTriggered')}
                description={t('pages.dashboard.noRulesTriggeredDescription')}
              />
            ) : (
              <Table columns={ruleColumns} data={topRules.data ?? []} rowKey={(r) => r.rule_id} dense />
            )}
          </CardBody>
        </Card>
      </div>

      {/* Agent fleet */}
      <Card className="mt-4">
        <CardHeader
          title={t('pages.dashboard.agentFleet')}
          description={t('pages.dashboard.agentFleetDescription')}
          action={
            <Link to="/agents">
              <Button variant="ghost" size="sm" icon={<ArrowRight weight="bold" className="h-3.5 w-3.5" />}>
                {t('common.viewAll')}
              </Button>
            </Link>
          }
        />
        <CardBody>
          {agents.isPending ? (
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-4">
              {Array.from({ length: 4 }).map((_, i) => (
                <SkeletonStat key={i} />
              ))}
            </div>
          ) : agents.isError ? (
            <ErrorState variant="inline" error={agents.error} onRetry={() => agents.refetch()} />
          ) : (agents.data ?? []).length === 0 ? (
            <EmptyState
              className="py-8"
              icon={<Desktop weight="duotone" className="h-7 w-7" />}
              title={t('pages.agents.empty')}
              description={t('pages.dashboard.noAgentsDescription')}
            />
          ) : (
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-4">
              {(agents.data ?? []).slice(0, 8).map((agent) => (
                <Link
                  key={agent.id}
                  to="/agents"
                  className="group rounded-lg border border-line bg-recessed/40 p-3.5 transition-all duration-150 hover:border-fill hover:bg-recessed"
                >
                  <div className="flex items-start justify-between gap-2">
                    <span className="flex min-w-0 items-center gap-2">
                      <span
                        className={cn(
                          'h-2 w-2 shrink-0 rounded-full',
                          agent.status === 'online'
                            ? 'bg-success'
                            : agent.status === 'degraded'
                              ? 'bg-warning'
                              : 'bg-fg-subtle/50',
                        )}
                      />
                      <span className="truncate text-[13px] font-medium text-fg-strong">
                        {agent.hostname}
                      </span>
                    </span>
                    <Badge
                      size="sm"
                      tone={
                        agent.status === 'online'
                          ? 'success'
                          : agent.status === 'degraded'
                            ? 'warning'
                            : 'neutral'
                      }
                    >
                      {t(`status.${agent.status}`, agent.status)}
                    </Badge>
                  </div>
                  <p className="pw-mono mt-2 truncate text-xs text-fg-subtle">{agent.ip_address}</p>
                  <p className="mt-1.5 text-xs text-fg-subtle">
                    {agent.site_domain ?? t('pages.dashboard.unassigned')}
                  </p>
                  <p className="mt-1 text-xs text-fg-subtle/80">
                    {agent.last_heartbeat
                      ? formatRelative(agent.last_heartbeat)
                      : t('pages.agents.neverSeen')}
                  </p>
                </Link>
              ))}
            </div>
          )}
        </CardBody>
      </Card>

      {/* Traffic by site */}
      {!siteId && (
        <Card className="mt-4">
          <CardHeader
            title={t('pages.dashboard.topSites')}
            description={t('pages.dashboard.topSitesDescription')}
            action={
              <Link to="/sites">
                <Button variant="ghost" size="sm" icon={<ArrowRight weight="bold" className="h-3.5 w-3.5" />}>
                  {t('nav.sites')}
                </Button>
              </Link>
            }
          />
          <CardBody className="px-0 py-0">
            {topSites.isPending ? (
              <SkeletonRows rows={4} columns={5} />
            ) : (topSites.data ?? []).length === 0 ? (
              <EmptyState
                className="py-10"
                icon={<Globe weight="duotone" className="h-7 w-7" />}
                title={t('pages.sites.emptyTitle')}
                description={t('pages.sites.emptyDescription')}
              />
            ) : (
              <div className="divide-y divide-line">
                {(topSites.data ?? []).map((site) => {
                  const max = Math.max(1, ...(topSites.data ?? []).map((x) => x.requests))
                  return (
                    <Link
                      key={site.site_id}
                      to={`/sites/${site.site_id}/security/waf`}
                      className="flex items-center gap-4 px-5 py-3 transition-colors hover:bg-recessed/60"
                    >
                      <div className="min-w-0 flex-1">
                        <p className="truncate text-[13px] font-medium text-fg">{site.name}</p>
                        <p className="pw-mono truncate text-xs text-fg-subtle">{site.domain}</p>
                      </div>
                      <div className="hidden w-40 sm:block">
                        <div className="h-1.5 overflow-hidden rounded-full bg-recessed">
                          <div
                            className="h-full rounded-full bg-brand transition-[width] duration-500"
                            style={{ width: `${Math.max(2, (site.requests / max) * 100)}%` }}
                          />
                        </div>
                      </div>
                      <div className="w-20 text-right text-[13px] tabular-nums text-fg">
                        {formatCompactNumber(site.requests)}
                      </div>
                      <div className="w-20 text-right text-[13px] tabular-nums text-fg-danger">
                        {formatCompactNumber(site.blocked)}
                      </div>
                      <Badge
                        size="sm"
                        tone={site.status === 'active' ? 'success' : site.status === 'paused' ? 'warning' : 'neutral'}
                      >
                        {t(`status.${site.status}`, site.status)}
                      </Badge>
                    </Link>
                  )
                })}
              </div>
            )}
          </CardBody>
        </Card>
      )}
    </div>
  )
}
