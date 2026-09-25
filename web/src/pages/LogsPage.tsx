import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useMutation, useQuery } from '@tanstack/react-query'
import {
  ShieldWarning,
  ListDashes,
  MagnifyingGlass,
  DownloadSimple,
  ArrowClockwise,
  Trash,
  Funnel,
} from '@phosphor-icons/react'
import { PageHeader } from '@/components/PageHeader'
import { Card, CardBody } from '@/components/ui/Card'
import { Button } from '@/components/ui/Button'
import { Input } from '@/components/ui/Input'
import { Select } from '@/components/ui/Select'
import { Badge } from '@/components/ui/Badge'
import { Dialog } from '@/components/ui/Dialog'
import { Table, type Column } from '@/components/ui/Table'
import { ConfirmDialog } from '@/components/ui/ConfirmDialog'
import { EmptyState } from '@/components/ui/EmptyState'
import { Pagination } from '@/components/ui/Pagination'
import { SkeletonRows } from '@/components/ui/Skeleton'
import { useToast } from '@/components/ui/Toast'
import { ErrorState } from '@/components/ErrorState'
import { downloadJson, fileTimestamp, logKeys, logsApi } from '@/api/logs'
import { useCanWrite, useDebouncedValue, useSitesList } from '@/hooks'
import { cn } from '@/lib/utils'
import {
  formatDateTime,
  formatLatency,
  formatNumber,
  formatSize,
  fromLocalInputValue,
  statusTone,
  toLocalInputValue,
} from '@/lib/format'
import type { AccessLog, LogQueryParams, SecurityEvent } from '@/api/types'

type Tab = 'security' | 'access'

const RANGE_PRESETS = [
  { value: '1', label: '1h' },
  { value: '6', label: '6h' },
  { value: '24', label: '24h' },
  { value: '72', label: '3d' },
  { value: '168', label: '7d' },
  { value: '720', label: '30d' },
  { value: 'custom', label: '…' },
]

const METHODS = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS']
const PAGE_SIZES = [25, 50, 100, 200]

const ACTION_TONE: Record<string, 'danger' | 'warning' | 'info' | 'success' | 'neutral'> = {
  block: 'danger',
  blocked: 'danger',
  challenge: 'warning',
  js_challenge: 'warning',
  log: 'info',
  allow: 'success',
  allowed: 'success',
  pass: 'success',
}

interface Filters {
  preset: string
  from: string
  to: string
  siteId: string
  clientIp: string
  path: string
  action: string
  method: string
  statusClass: string
}

function defaultFilters(): Filters {
  const to = new Date()
  const from = new Date(to.getTime() - 24 * 3600_000)
  return {
    preset: '24',
    from: toLocalInputValue(from),
    to: toLocalInputValue(to),
    siteId: '',
    clientIp: '',
    path: '',
    action: '',
    method: '',
    statusClass: '',
  }
}

export function LogsPage() {
  const { t } = useTranslation()
  const toast = useToast()
  const canWrite = useCanWrite()

  const [tab, setTab] = useState<Tab>('security')
  const [filters, setFilters] = useState<Filters>(defaultFilters)
  const [page, setPage] = useState(1)
  const [pageSize, setPageSize] = useState(50)
  const [selectedEvent, setSelectedEvent] = useState<SecurityEvent | null>(null)
  const [selectedLog, setSelectedLog] = useState<AccessLog | null>(null)
  const [purgeOpen, setPurgeOpen] = useState(false)
  const [purgeDays, setPurgeDays] = useState(30)

  const debouncedIp = useDebouncedValue(filters.clientIp.trim(), 400)
  const debouncedPath = useDebouncedValue(filters.path.trim(), 400)

  const { data: sites } = useSitesList()

  // Any filter change must drop back to the first page, otherwise the request
  // asks for page 5 of a completely different result set.
  useEffect(() => {
    setPage(1)
  }, [tab, filters.preset, filters.siteId, filters.action, filters.method, filters.statusClass, debouncedIp, debouncedPath, pageSize])

  const params = useMemo<LogQueryParams>(() => {
    const base: LogQueryParams = {
      page,
      page_size: pageSize,
      from: fromLocalInputValue(filters.from),
      to: fromLocalInputValue(filters.to),
    }
    if (filters.siteId) base.site_id = filters.siteId
    if (debouncedIp) base.client_ip = debouncedIp
    if (debouncedPath) base.path = debouncedPath
    if (tab === 'security') {
      if (filters.action) base.action = filters.action
    } else {
      if (filters.method) base.method = filters.method
      if (filters.statusClass) base.status_class = Number(filters.statusClass)
    }
    return base
  }, [tab, page, pageSize, filters.from, filters.to, filters.siteId, filters.action, filters.method, filters.statusClass, debouncedIp, debouncedPath])

  const securityQuery = useQuery({
    queryKey: logKeys.security(params),
    queryFn: () => logsApi.securityEvents(params),
    enabled: tab === 'security',
    placeholderData: (prev) => prev,
  })

  const accessQuery = useQuery({
    queryKey: logKeys.access(params),
    queryFn: () => logsApi.accessLogs(params),
    enabled: tab === 'access',
    placeholderData: (prev) => prev,
  })

  const active = tab === 'security' ? securityQuery : accessQuery
  const total = active.data?.total ?? 0
  const rows = active.data?.items ?? []

  const purge = useMutation({
    mutationFn: () =>
      logsApi.purge({
        older_than_days: purgeDays,
        ...(filters.siteId ? { site_id: filters.siteId } : {}),
      }),
    onSuccess: (result) => {
      toast.success(
        t('pages.logs.purgeDone'),
        t('pages.logs.purgeResult', {
          events: formatNumber(result.deleted_security_events),
          logs: formatNumber(result.deleted_access_logs),
        }),
      )
      setPurgeOpen(false)
      void securityQuery.refetch()
      void accessQuery.refetch()
    },
  })

  const setFilter = <K extends keyof Filters>(key: K, value: Filters[K]) => {
    setFilters((f) => {
      if (key === 'preset' && value !== 'custom') {
        const hours = Number(value)
        if (Number.isFinite(hours)) {
          const to = new Date()
          return {
            ...f,
            preset: value,
            to: toLocalInputValue(to),
            from: toLocalInputValue(new Date(to.getTime() - hours * 3600_000)),
          }
        }
      }
      return { ...f, [key]: value }
    })
  }

  const exportJson = () => {
    if (rows.length === 0) {
      toast.warning(t('pages.logs.exportEmpty'))
      return
    }
    const name = `pingwaf-${tab}-${fileTimestamp()}`
    downloadJson(name, {
      exported_at: new Date().toISOString(),
      kind: tab,
      filters: params,
      total,
      count: rows.length,
      items: rows,
    })
    toast.success(t('pages.logs.exported'), `${rows.length} → ${name}.json`)
  }

  const siteOptions = useMemo(
    () => [
      { value: '', label: t('pages.logs.allSites') },
      ...(sites ?? []).map((s) => ({ value: s.id, label: s.domain })),
    ],
    [sites, t],
  )

  const securityColumns: Column<SecurityEvent>[] = [
    {
      key: 'timestamp',
      header: t('pages.logs.time'),
      accessor: (r) => r.timestamp,
      sortable: true,
      width: '1%',
      cell: (r) => (
        <span className="pw-mono whitespace-nowrap text-xs text-fg-subtle">
          {formatDateTime(r.timestamp)}
        </span>
      ),
    },
    {
      key: 'client_ip',
      header: t('pages.logs.clientIp'),
      accessor: (r) => r.client_ip,
      sortable: true,
      cell: (r) => (
        <div className="min-w-0">
          <p className="pw-mono truncate text-[13px] text-fg">{r.client_ip}</p>
          {r.country_code && (
            <p className="text-[11px] text-fg-subtle">{r.country_code.toUpperCase()}</p>
          )}
        </div>
      ),
    },
    {
      key: 'method',
      header: t('pages.logs.method'),
      accessor: (r) => r.method,
      width: '1%',
      cell: (r) => <span className="pw-mono text-xs text-fg-subtle">{r.method}</span>,
    },
    {
      key: 'path',
      header: t('pages.logs.path'),
      accessor: (r) => r.path ?? '',
      cell: (r) => (
        <div className="min-w-0">
          <p className="pw-mono truncate text-[13px]">{r.path ?? '—'}</p>
          {r.host && <p className="truncate text-[11px] text-fg-subtle">{r.host}</p>}
        </div>
      ),
    },
    {
      key: 'rule',
      header: t('pages.logs.rule'),
      accessor: (r) => r.rule_name ?? '',
      cell: (r) =>
        r.rule_name || r.rule_id ? (
          <div className="min-w-0">
            <p className="truncate text-[13px]">{r.rule_name ?? r.rule_id}</p>
            {r.rule_name && r.rule_id && (
              <p className="pw-mono truncate text-[11px] text-fg-subtle">{r.rule_id}</p>
            )}
          </div>
        ) : (
          <span className="text-fg-subtle">—</span>
        ),
    },
    {
      key: 'action',
      header: t('pages.logs.action'),
      accessor: (r) => r.action,
      width: '1%',
      cell: (r) => (
        <Badge tone={ACTION_TONE[r.action] ?? 'neutral'}>
          {t(`actions.${r.action}`, r.action)}
        </Badge>
      ),
    },
    {
      key: 'score',
      header: t('pages.logs.wafScore'),
      accessor: (r) => r.score ?? 0,
      sortable: true,
      align: 'right',
      width: '1%',
      cell: (r) => (
        <span
          className={cn(
            'tabular-nums text-[13px]',
            (r.score ?? 0) >= 50 ? 'font-medium text-fg-danger' : 'text-fg-subtle',
          )}
        >
          {r.score ?? '—'}
        </span>
      ),
    },
  ]

  const accessColumns: Column<AccessLog>[] = [
    {
      key: 'timestamp',
      header: t('pages.logs.time'),
      accessor: (r) => r.timestamp,
      sortable: true,
      width: '1%',
      cell: (r) => (
        <span className="pw-mono whitespace-nowrap text-xs text-fg-subtle">
          {formatDateTime(r.timestamp)}
        </span>
      ),
    },
    {
      key: 'client_ip',
      header: t('pages.logs.clientIp'),
      accessor: (r) => r.client_ip,
      sortable: true,
      cell: (r) => (
        <div className="min-w-0">
          <p className="pw-mono truncate text-[13px] text-fg">{r.client_ip}</p>
          {r.country_code && (
            <p className="text-[11px] text-fg-subtle">{r.country_code.toUpperCase()}</p>
          )}
        </div>
      ),
    },
    {
      key: 'method',
      header: t('pages.logs.method'),
      accessor: (r) => r.method,
      width: '1%',
      cell: (r) => <span className="pw-mono text-xs text-fg-subtle">{r.method}</span>,
    },
    {
      key: 'path',
      header: t('pages.logs.path'),
      accessor: (r) => r.path ?? '',
      cell: (r) => (
        <div className="min-w-0">
          <p className="pw-mono truncate text-[13px]">{r.path ?? '—'}</p>
          {r.host && <p className="truncate text-[11px] text-fg-subtle">{r.host}</p>}
        </div>
      ),
    },
    {
      key: 'status_code',
      header: t('pages.logs.statusCode'),
      accessor: (r) => r.status_code ?? 0,
      sortable: true,
      align: 'center',
      width: '1%',
      cell: (r) =>
        r.status_code ? (
          <Badge tone={statusTone(r.status_code)}>{r.status_code}</Badge>
        ) : (
          <span className="text-fg-subtle">—</span>
        ),
    },
    {
      key: 'cache_status',
      header: t('pages.logs.cacheStatus'),
      accessor: (r) => r.cache_status ?? '',
      width: '1%',
      cell: (r) =>
        r.cache_status ? (
          <span
            className={cn(
              'pw-mono text-xs',
              r.cache_status === 'hit' ? 'text-fg-success' : 'text-fg-subtle',
            )}
          >
            {r.cache_status}
          </span>
        ) : (
          <span className="text-fg-subtle">—</span>
        ),
    },
    {
      key: 'size',
      header: t('pages.logs.size'),
      accessor: (r) => r.response_size ?? 0,
      align: 'right',
      width: '1%',
      cell: (r) => (
        <span className="tabular-nums text-[13px] text-fg-subtle">
          {r.response_size != null ? formatSize(r.response_size) : '—'}
        </span>
      ),
    },
    {
      key: 'latency',
      header: t('pages.logs.latency'),
      accessor: (r) => r.total_latency_ms ?? 0,
      sortable: true,
      align: 'right',
      width: '1%',
      cell: (r) => (
        <span className="tabular-nums text-[13px]">
          {r.total_latency_ms != null ? formatLatency(r.total_latency_ms) : '—'}
        </span>
      ),
    },
  ]

  const tabs: { value: Tab; label: string; icon: typeof ShieldWarning; count?: number }[] = [
    { value: 'security', label: t('pages.logs.securityTab'), icon: ShieldWarning },
    { value: 'access', label: t('pages.logs.accessTab'), icon: ListDashes },
  ]

  return (
    <div className="animate-slide-up">
      <PageHeader
        title={t('pages.logs.title')}
        description={t('pages.logs.description')}
        actions={
          <div className="flex items-center gap-2">
            <Button
              variant="secondary"
              loading={active.isFetching}
              onClick={() => active.refetch()}
              icon={<ArrowClockwise weight="duotone" className="h-4 w-4" />}
            >
              {t('common.refresh')}
            </Button>
            <Button
              variant="secondary"
              onClick={exportJson}
              disabled={rows.length === 0}
              icon={<DownloadSimple weight="duotone" className="h-4 w-4" />}
            >
              {t('pages.logs.export')}
            </Button>
            {canWrite && (
              <Button
                variant="ghost"
                className="hover:text-fg-danger"
                onClick={() => setPurgeOpen(true)}
                icon={<Trash weight="duotone" className="h-4 w-4" />}
              >
                {t('pages.logs.purge')}
              </Button>
            )}
          </div>
        }
      />

      {/* Tabs */}
      <div className="mb-4 flex gap-1 border-b border-line">
        {tabs.map((entry) => {
          const Icon = entry.icon
          return (
            <button
              key={entry.value}
              type="button"
              onClick={() => setTab(entry.value)}
              className={cn(
                '-mb-px inline-flex items-center gap-2 border-b-2 px-3 py-2.5 text-sm font-medium transition-colors',
                tab === entry.value
                  ? 'border-brand text-fg-strong'
                  : 'border-transparent text-fg-subtle hover:text-fg',
              )}
            >
              <Icon weight="duotone" className="h-4 w-4" />
              {entry.label}
            </button>
          )
        })}
      </div>

      {/* Filter bar */}
      <Card className="mb-4">
        <CardBody>
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
            <Select
              label={t('pages.logs.timeRange')}
              value={filters.preset}
              options={RANGE_PRESETS.map((p) => ({
                value: p.value,
                label:
                  p.value === 'custom'
                    ? t('pages.logs.customRange')
                    : t('pages.logs.lastN', { range: p.label }),
              }))}
              onChange={(e) => setFilter('preset', e.target.value)}
            />
            <Input
              type="datetime-local"
              label={t('pages.logs.from')}
              value={filters.from}
              max={filters.to}
              onChange={(e) => setFilter('from', e.target.value)}
            />
            <Input
              type="datetime-local"
              label={t('pages.logs.to')}
              value={filters.to}
              min={filters.from}
              onChange={(e) => setFilter('to', e.target.value)}
            />
            <Select
              label={t('pages.logs.site')}
              value={filters.siteId}
              options={siteOptions}
              onChange={(e) => setFilter('siteId', e.target.value)}
            />
            <Input
              label={t('pages.logs.clientIp')}
              value={filters.clientIp}
              placeholder="203.0.113.44"
              prefixIcon={<MagnifyingGlass weight="duotone" />}
              onChange={(e) => setFilter('clientIp', e.target.value)}
            />
            <Input
              label={t('pages.logs.path')}
              value={filters.path}
              placeholder="/api/v1/users"
              onChange={(e) => setFilter('path', e.target.value)}
            />
            {tab === 'security' ? (
              <Select
                label={t('pages.logs.action')}
                value={filters.action}
                options={[
                  { value: '', label: t('pages.logs.anyAction') },
                  ...['block', 'challenge', 'js_challenge', 'log', 'allow'].map((a) => ({
                    value: a,
                    label: t(`actions.${a}`, a),
                  })),
                ]}
                onChange={(e) => setFilter('action', e.target.value)}
              />
            ) : (
              <>
                <Select
                  label={t('pages.logs.method')}
                  value={filters.method}
                  options={[
                    { value: '', label: t('pages.logs.anyMethod') },
                    ...METHODS.map((m) => ({ value: m, label: m })),
                  ]}
                  onChange={(e) => setFilter('method', e.target.value)}
                />
                <Select
                  label={t('pages.logs.statusClass')}
                  value={filters.statusClass}
                  options={[
                    { value: '', label: t('pages.logs.anyStatus') },
                    { value: '2', label: '2xx' },
                    { value: '3', label: '3xx' },
                    { value: '4', label: '4xx' },
                    { value: '5', label: '5xx' },
                  ]}
                  onChange={(e) => setFilter('statusClass', e.target.value)}
                />
              </>
            )}
          </div>

          <div className="mt-3 flex items-center justify-between gap-3 border-t border-line pt-3">
            <span className="inline-flex items-center gap-1.5 text-xs text-fg-subtle">
              <Funnel weight="duotone" className="h-3.5 w-3.5" />
              {t('pages.logs.resultCount', { total: formatNumber(total) })}
            </span>
            <Button size="sm" variant="ghost" onClick={() => setFilters(defaultFilters())}>
              {t('common.reset')}
            </Button>
          </div>
        </CardBody>
      </Card>

      {/* Results */}
      {active.isError && !active.data ? (
        <ErrorState
          error={active.error}
          onRetry={() => active.refetch()}
          retrying={active.isFetching}
        />
      ) : (
        <Card>
          <CardBody className="p-0">
            {active.isPending ? (
              <SkeletonRows rows={10} columns={tab === 'security' ? 7 : 8} />
            ) : rows.length === 0 ? (
              <EmptyState
                className="py-14"
                icon={
                  tab === 'security' ? (
                    <ShieldWarning weight="duotone" className="h-8 w-8" />
                  ) : (
                    <ListDashes weight="duotone" className="h-8 w-8" />
                  )
                }
                title={t('pages.logs.empty')}
                description={t('pages.logs.emptyDescription')}
                action={
                  <Button variant="secondary" onClick={() => setFilters(defaultFilters())}>
                    {t('common.reset')}
                  </Button>
                }
              />
            ) : (
              <>
                {tab === 'security' ? (
                  <Table
                    columns={securityColumns}
                    data={rows as SecurityEvent[]}
                    rowKey={(r) => String(r.id)}
                    dense
                    onRowClick={(r) => setSelectedEvent(r)}
                  />
                ) : (
                  <Table
                    columns={accessColumns}
                    data={rows as AccessLog[]}
                    rowKey={(r) => String(r.id)}
                    dense
                    onRowClick={(r) => setSelectedLog(r)}
                  />
                )}
                <Pagination
                  page={page}
                  pageSize={pageSize}
                  total={total}
                  onChange={setPage}
                  pageSizeOptions={PAGE_SIZES}
                  onPageSizeChange={setPageSize}
                />
              </>
            )}
          </CardBody>
        </Card>
      )}

      {/* Security event detail */}
      <Dialog
        open={selectedEvent !== null}
        onClose={() => setSelectedEvent(null)}
        size="lg"
        title={t('pages.logs.eventDetail')}
        description={
          selectedEvent
            ? `${selectedEvent.method} ${selectedEvent.path ?? ''}`.trim()
            : undefined
        }
        footer={
          <Button variant="ghost" onClick={() => setSelectedEvent(null)}>
            {t('common.close')}
          </Button>
        }
      >
        {selectedEvent && (
          <div className="flex flex-col gap-4">
            <div className="flex flex-wrap items-center gap-2">
              <Badge tone={ACTION_TONE[selectedEvent.action] ?? 'neutral'}>
                {t(`actions.${selectedEvent.action}`, selectedEvent.action)}
              </Badge>
              {selectedEvent.score != null && (
                <Badge tone={selectedEvent.score >= 50 ? 'danger' : 'neutral'}>
                  {t('pages.logs.wafScore')}: {selectedEvent.score}
                </Badge>
              )}
              {selectedEvent.rule_name && <Badge tone="info">{selectedEvent.rule_name}</Badge>}
            </div>
            <DefinitionGrid
              rows={[
                [t('pages.logs.time'), formatDateTime(selectedEvent.timestamp)],
                [t('pages.logs.clientIp'), selectedEvent.client_ip],
                [t('pages.logs.country'), selectedEvent.country_code?.toUpperCase()],
                [t('pages.logs.method'), selectedEvent.method],
                [t('pages.logs.host'), selectedEvent.host],
                [t('pages.logs.path'), selectedEvent.path],
                [t('pages.logs.rule'), selectedEvent.rule_id],
                [t('pages.logs.requestId'), selectedEvent.request_id],
                [t('pages.logs.agent'), selectedEvent.agent_id],
                [t('pages.logs.site'), selectedEvent.site_id],
              ]}
            />
            {selectedEvent.waf_details && (
              <div>
                <p className="mb-1.5 text-[13px] font-medium text-fg">
                  {t('pages.logs.wafDetails')}
                </p>
                <pre className="pw-mono max-h-56 overflow-auto rounded-md border border-line bg-recessed px-3 py-2 text-xs text-fg-subtle">
                  {selectedEvent.waf_details}
                </pre>
              </div>
            )}
            {selectedEvent.user_agent && (
              <div>
                <p className="mb-1.5 text-[13px] font-medium text-fg">
                  {t('pages.logs.userAgent')}
                </p>
                <p className="pw-mono break-all rounded-md border border-line bg-recessed px-3 py-2 text-xs text-fg-subtle">
                  {selectedEvent.user_agent}
                </p>
              </div>
            )}
          </div>
        )}
      </Dialog>

      {/* Access log detail */}
      <Dialog
        open={selectedLog !== null}
        onClose={() => setSelectedLog(null)}
        size="lg"
        title={t('pages.logs.accessDetail')}
        description={selectedLog ? `${selectedLog.method} ${selectedLog.path ?? ''}`.trim() : undefined}
        footer={
          <Button variant="ghost" onClick={() => setSelectedLog(null)}>
            {t('common.close')}
          </Button>
        }
      >
        {selectedLog && (
          <div className="flex flex-col gap-4">
            <div className="flex flex-wrap items-center gap-2">
              {selectedLog.status_code && (
                <Badge tone={statusTone(selectedLog.status_code)}>{selectedLog.status_code}</Badge>
              )}
              {selectedLog.cache_status && (
                <Badge tone={selectedLog.cache_status === 'hit' ? 'success' : 'neutral'}>
                  {selectedLog.cache_status}
                </Badge>
              )}
              {selectedLog.tls_version && <Badge tone="info">{selectedLog.tls_version}</Badge>}
            </div>
            <DefinitionGrid
              rows={[
                [t('pages.logs.time'), formatDateTime(selectedLog.timestamp)],
                [t('pages.logs.clientIp'), selectedLog.client_ip],
                [t('pages.logs.country'), selectedLog.country_code?.toUpperCase()],
                [t('pages.logs.method'), selectedLog.method],
                [t('pages.logs.host'), selectedLog.host],
                [t('pages.logs.path'), selectedLog.path],
                [t('pages.logs.queryString'), selectedLog.query_string],
                [t('pages.logs.statusCode'), selectedLog.status_code?.toString()],
                [t('pages.logs.size'), selectedLog.response_size != null ? formatSize(selectedLog.response_size) : undefined],
                [t('pages.logs.upstream'), selectedLog.upstream_addr],
                [
                  t('pages.logs.upstreamLatency'),
                  selectedLog.upstream_latency_ms != null
                    ? formatLatency(selectedLog.upstream_latency_ms)
                    : undefined,
                ],
                [
                  t('pages.logs.latency'),
                  selectedLog.total_latency_ms != null
                    ? formatLatency(selectedLog.total_latency_ms)
                    : undefined,
                ],
                [t('pages.logs.cacheStatus'), selectedLog.cache_status],
                [t('pages.logs.referer'), selectedLog.referer],
                [t('pages.logs.requestId'), selectedLog.request_id],
                [t('pages.logs.agent'), selectedLog.agent_id],
              ]}
            />
            {selectedLog.user_agent && (
              <div>
                <p className="mb-1.5 text-[13px] font-medium text-fg">
                  {t('pages.logs.userAgent')}
                </p>
                <p className="pw-mono break-all rounded-md border border-line bg-recessed px-3 py-2 text-xs text-fg-subtle">
                  {selectedLog.user_agent}
                </p>
              </div>
            )}
          </div>
        )}
      </Dialog>

      {/* Purge */}
      <ConfirmDialog
        open={purgeOpen}
        onClose={() => setPurgeOpen(false)}
        onConfirm={() => purge.mutate()}
        title={t('pages.logs.purgeTitle')}
        description={t('pages.logs.purgeDescription')}
        confirmLabel={t('pages.logs.purge')}
        loading={purge.isPending}
      >
        <div className="flex flex-col gap-3">
          <Input
            type="number"
            label={t('pages.logs.olderThanDays')}
            value={purgeDays}
            min={1}
            max={3650}
            hint={t('pages.logs.purgeScope', {
              scope: filters.siteId
                ? (sites ?? []).find((s) => s.id === filters.siteId)?.domain ?? filters.siteId
                : t('pages.logs.allSites'),
            })}
            onChange={(e) => setPurgeDays(Number(e.target.value))}
          />
        </div>
      </ConfirmDialog>
    </div>
  )
}

/** Two-column label/value list used by both detail dialogs. */
function DefinitionGrid({ rows }: { rows: [string, string | null | undefined][] }) {
  const visible = rows.filter(([, value]) => value !== null && value !== undefined && value !== '')
  if (visible.length === 0) return null
  return (
    <dl className="grid grid-cols-1 gap-x-6 gap-y-2 sm:grid-cols-2">
      {visible.map(([label, value]) => (
        <div key={label} className="flex min-w-0 items-baseline justify-between gap-3 border-b border-line/60 pb-1.5">
          <dt className="shrink-0 text-xs text-fg-subtle">{label}</dt>
          <dd className="pw-mono min-w-0 truncate text-right text-[13px] text-fg">{value}</dd>
        </div>
      ))}
    </dl>
  )
}

export default LogsPage
