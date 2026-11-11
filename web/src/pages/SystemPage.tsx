import { useMemo } from 'react'
import { ArrowClockwise } from '@phosphor-icons/react'
import { useTranslation } from 'react-i18next'
import { useQuery } from '@tanstack/react-query'
import { PageHeader } from '@/components/PageHeader'
import { Button } from '@/components/ui/Button'
import { Card, CardBody, CardHeader } from '@/components/ui/Card'
import { EmptyState } from '@/components/ui/EmptyState'
import { ErrorState } from '@/components/ErrorState'
import { MetricCards, MetricPills } from '@/components/system/MetricCards'
import { MemoryDetail } from '@/components/system/MemoryDetail'
import { ModuleTable } from '@/components/system/ModuleTable'
import { MethodTable } from '@/components/system/MethodTable'
import { NodeSection } from '@/components/system/NodeSection'
import { fetchSystemMetrics, fetchSystemNodes, fetchSystemProfile, debugKeys } from '@/api/debug'
import systemApi from '@/api/system'
import { formatDateTime } from '@/lib/format'

/** metrics 轮询周期（10s），staleTime 取一半。 */
const METRICS_INTERVAL_MS = 10_000
/** profile 轮询周期（60s），staleTime 取一半。 */
const PROFILE_INTERVAL_MS = 60_000
/** nodes 轮询周期（15s），staleTime 取一半。 */
const NODES_INTERVAL_MS = 15_000

/**
 * System 观测页 — `/api/v1/system/metrics` + `/api/v1/system/profile`（仅 Admin）。
 * metrics 10s 轮询、profile 60s 轮询；错误以行内重试呈现，不整页报错。
 */
export function SystemPage() {
  const { t } = useTranslation()

  const metrics = useQuery({
    queryKey: debugKeys.metrics(),
    queryFn: fetchSystemMetrics,
    refetchInterval: METRICS_INTERVAL_MS,
    staleTime: METRICS_INTERVAL_MS / 2,
    retry: false,
  })
  const profile = useQuery({
    queryKey: debugKeys.profile(),
    queryFn: fetchSystemProfile,
    refetchInterval: PROFILE_INTERVAL_MS,
    staleTime: PROFILE_INTERVAL_MS / 2,
    retry: false,
  })

  const nodes = useQuery({
    queryKey: debugKeys.nodes(),
    queryFn: fetchSystemNodes,
    refetchInterval: NODES_INTERVAL_MS,
    staleTime: NODES_INTERVAL_MS / 2,
    retry: false,
  })

  const build = useQuery({
    queryKey: debugKeys.version(),
    queryFn: systemApi.version,
    // Only a deploy changes it; one fetch per visit is enough.
    staleTime: 5 * 60_000,
    retry: false,
  })

  const collectedAt = metrics.data?.cpu.collected_at_ms ?? metrics.data?.memory.collected_at_ms

  /**
   * Agents whose reported build differs from this control plane.
   *
   * Compared against the console's own build metadata rather than a hard-coded
   * string: `null` means "never reported", which is not a mismatch but is worth
   * showing, so those rows are kept in a separate bucket.
   */
  const fleet = useMemo(() => {
    const rows = nodes.data?.agents ?? []
    const current = build.data?.version
    return {
      total: rows.length,
      unknown: rows.filter((a) => !a.version),
      outdated: current ? rows.filter((a) => a.version && a.version !== current) : [],
    }
  }, [nodes.data, build.data])

  return (
    <div>
      <PageHeader
        title={t('system.title')}
        description={t('system.subtitle')}
        actions={
          <>
            {collectedAt != null && (
              <span className="hidden text-xs text-fg-subtle tabular-nums sm:inline">
                {t('system.collectedAt', { time: formatDateTime(collectedAt) })}
              </span>
            )}
            <Button
              variant="secondary"
              size="sm"
              icon={
                <ArrowClockwise
                  weight="duotone"
                  className={`h-4 w-4 ${metrics.isFetching || profile.isFetching || nodes.isFetching ? 'animate-spin-slow' : ''}`}
                />
              }
              onClick={() => {
                void metrics.refetch()
                void profile.refetch()
                void nodes.refetch()
              }}
            >
              {t('system.refresh')}
            </Button>
          </>
        }
      />

      {/* Build identity: this control plane, and how many agents are not on
          it. A node that fails to auto-upgrade shows up here first. */}
      <div className="mb-4">
        <Card>
          <CardHeader title={t('system.versionTitle')} />
          <CardBody>
            <dl className="grid grid-cols-1 gap-x-6 gap-y-3 sm:grid-cols-3">
              <div>
                <dt className="text-[11px] uppercase tracking-wide text-fg-subtle">
                  {t('system.systemVersionControlPlane')}
                </dt>
                <dd className="pw-mono text-[13px] text-fg-strong">
                  {build.data?.version ?? '—'}
                </dd>
              </div>
              <div>
                <dt className="text-[11px] uppercase tracking-wide text-fg-subtle">
                  {t('system.systemVersionApi')}
                </dt>
                <dd className="pw-mono text-[13px] text-fg">{build.data?.api ?? '—'}</dd>
              </div>
              <div>
                <dt className="text-[11px] uppercase tracking-wide text-fg-subtle">
                  {t('system.systemVersionAgents')}
                </dt>
                <dd className="text-[13px]">
                  {fleet.total === 0 ? (
                    <span className="text-fg-subtle">
                      {t('system.systemVersionNoAgents')}
                    </span>
                  ) : fleet.outdated.length > 0 ? (
                    <span className="text-fg-warning">
                      {t('system.systemVersionAgentsOutdated', {
                        count: fleet.outdated.length,
                        total: fleet.total,
                      })}
                    </span>
                  ) : (
                    <span className="text-fg">
                      {t('system.systemVersionAgentsCurrent', {
                        total: fleet.total,
                      })}
                    </span>
                  )}
                </dd>
              </div>
            </dl>
            {fleet.outdated.length > 0 && (
              <div className="mt-3 flex flex-wrap gap-1.5">
                {fleet.outdated.map((agent) => (
                  <span
                    key={agent.id}
                    className="pw-mono rounded border border-warning/40 bg-warning/10 px-1.5 py-0.5 text-[11px] text-fg-warning"
                  >
                    {agent.name} · {agent.version}
                  </span>
                ))}
              </div>
            )}
            {fleet.unknown.length > 0 && (
              <p className="mt-2 text-[11px] text-fg-subtle">
                {t('system.systemVersionAgentsUnknown', {
                  count: fleet.unknown.length,
                })}
              </p>
            )}
          </CardBody>
        </Card>
      </div>

      {metrics.isError ? (
        <div className="mb-4">
          <ErrorState
            variant="inline"
            error={metrics.error}
            onRetry={() => void metrics.refetch()}
            retrying={metrics.isFetching}
          />
        </div>
      ) : (
        <>
          <p className="mb-2 text-xs font-medium uppercase tracking-wide text-fg-subtle">
            {t('system.overview')}
          </p>
          <MetricCards metrics={metrics.data} loading={metrics.isFetching} />
          <div className="mt-3">
            <MetricPills metrics={metrics.data} />
          </div>
          <div className="mt-3">
            <MemoryDetail memory={metrics.data?.memory} />
          </div>
        </>
      )}

      <NodeSection
        nodes={
          nodes.data
            ? { control: nodes.data.control, boots: nodes.data.boots }
            : undefined
        }
        error={nodes.isError ? nodes.error : null}
        loading={nodes.isFetching}
        onRetry={() => void nodes.refetch()}
      />

      <div className="mt-4 grid grid-cols-1 gap-4 xl:grid-cols-2">
        <Card>
          <CardHeader
            title={t('system.modules')}
            description={t(
              'system.samplingHint',
              '内存列为当前持有值（0 表示尚未上报，—/peak 为峰值口径）；各占比为占所属模块/全部归因时间的比例，子阶段份额不应相加。',
            )}
          />
          <CardBody>
            {profile.isError ? (
              <ErrorState
                variant="inline"
                error={profile.error}
                onRetry={() => void profile.refetch()}
                retrying={profile.isFetching}
              />
            ) : (
              <ModuleTable modules={profile.data?.modules} loading={profile.isLoading} />
            )}
          </CardBody>
        </Card>

        <Card>
          <CardHeader title={t('system.topMethods')} />
          <CardBody>
            {profile.isError ? (
              <ErrorState
                variant="inline"
                error={profile.error}
                onRetry={() => void profile.refetch()}
                retrying={profile.isFetching}
              />
            ) : (
              <MethodTable methods={profile.data?.methods} loading={profile.isLoading} />
            )}
          </CardBody>
        </Card>
      </div>

      {!metrics.isLoading && !metrics.data && !metrics.isError && (
        <div className="mt-4">
          <EmptyState title={t('system.empty')} description={t('system.loadError')} />
        </div>
      )}
    </div>
  )
}

export default SystemPage
