import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { useParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  Lightning,
  Plus,
  PencilSimple,
  Trash,
  ArrowClockwise,
  Database,
  Target,
  Stack,
  Broom,
} from '@phosphor-icons/react'
import { PageHeader } from '@/components/PageHeader'
import { Card, CardBody, CardHeader } from '@/components/ui/Card'
import { Button } from '@/components/ui/Button'
import { Input } from '@/components/ui/Input'
import { Switch } from '@/components/ui/Switch'
import { Dialog } from '@/components/ui/Dialog'
import { Textarea } from '@/components/ui/Textarea'
import { Table, type Column } from '@/components/ui/Table'
import { ConfirmDialog } from '@/components/ui/ConfirmDialog'
import { EmptyState } from '@/components/ui/EmptyState'
import { SkeletonRows, SkeletonStat } from '@/components/ui/Skeleton'
import { useToast } from '@/components/ui/Toast'
import { ErrorState } from '@/components/ErrorState'
import { cachingApi, cacheKeys } from '@/api/caching'
import { useCanWrite } from '@/hooks'
import { cn } from '@/lib/utils'
import { formatNumber, formatPercent, formatSize } from '@/lib/format'
import type { CacheRule, CreateCacheRuleRequest } from '@/api/types'

interface FormState {
  name: string
  match_expression: string
  edge_ttl_seconds: number
  browser_ttl_seconds: number
  disk_quota_mb: number
  cache_eligible: boolean
  respect_origin: boolean
  enabled: boolean
}

const emptyForm = (): FormState => ({
  name: '',
  match_expression: '',
  edge_ttl_seconds: 3600,
  browser_ttl_seconds: 300,
  disk_quota_mb: 0,
  cache_eligible: true,
  respect_origin: true,
  enabled: true,
})

function formFromRule(rule: CacheRule): FormState {
  return {
    name: rule.name,
    match_expression: rule.match_expression,
    edge_ttl_seconds: rule.edge_ttl_seconds,
    browser_ttl_seconds: rule.browser_ttl_seconds,
    disk_quota_mb: rule.disk_quota_mb,
    cache_eligible: rule.cache_eligible,
    respect_origin: rule.respect_origin,
    enabled: rule.enabled,
  }
}

type Translate = ReturnType<typeof useTranslation>['t']

function formatTtl(seconds: number, t: Translate): string {
  if (seconds <= 0) return t('pages.caching.respectOrigin')
  if (seconds >= 3600 && seconds % 3600 === 0) return t('pages.caching.hours', { n: seconds / 3600 })
  if (seconds >= 60 && seconds % 60 === 0) return t('pages.caching.minutes', { n: seconds / 60 })
  return t('pages.caching.seconds', { n: seconds })
}

export function CachingPage() {
  const { t } = useTranslation()
  const toast = useToast()
  const queryClient = useQueryClient()
  const canWrite = useCanWrite()
  const { siteId = '' } = useParams<{ siteId: string }>()

  const [dialogOpen, setDialogOpen] = useState(false)
  const [editing, setEditing] = useState<CacheRule | null>(null)
  const [form, setForm] = useState<FormState>(emptyForm())
  const [error, setError] = useState<string | null>(null)
  const [pendingDelete, setPendingDelete] = useState<CacheRule | null>(null)
  const [purgeUrls, setPurgeUrls] = useState('')
  const [purgeAllOpen, setPurgeAllOpen] = useState(false)

  const rulesQuery = useQuery({
    queryKey: cacheKeys.rules(siteId),
    queryFn: () => cachingApi.listRules(siteId),
    enabled: Boolean(siteId),
    select: (page) => page.items,
  })

  const statsQuery = useQuery({
    queryKey: cacheKeys.stats(siteId),
    queryFn: () => cachingApi.stats(siteId),
    enabled: Boolean(siteId),
  })

  const rules = useMemo(
    () => [...(rulesQuery.data ?? [])].sort((a, b) => a.name.localeCompare(b.name)),
    [rulesQuery.data],
  )

  useEffect(() => {
    if (!dialogOpen) return
    setError(null)
    setForm(editing ? formFromRule(editing) : emptyForm())
  }, [dialogOpen, editing])

  const invalidate = () => {
    void queryClient.invalidateQueries({ queryKey: cacheKeys.all(siteId) })
  }

  const save = useMutation({
    mutationFn: (payload: CreateCacheRuleRequest) =>
      editing
        ? cachingApi.updateRule(siteId, editing.id, payload)
        : cachingApi.createRule(siteId, payload),
    onSuccess: (rule) => {
      toast.success(
        editing ? t('pages.caching.ruleUpdated') : t('pages.caching.ruleCreated'),
        rule.name,
      )
      closeDialog()
      invalidate()
    },
    onError: (e) => setError(e instanceof Error ? e.message : String(e)),
  })

  const toggle = useMutation({
    mutationFn: ({ rule, enabled }: { rule: CacheRule; enabled: boolean }) =>
      cachingApi.toggleEnabled(siteId, rule.id, enabled),
    onSuccess: () => invalidate(),
  })

  const remove = useMutation({
    mutationFn: (id: string) => cachingApi.deleteRule(siteId, id),
    onSuccess: (_d, id) => {
      toast.success(t('pages.caching.ruleDeleted'), rules.find((r) => r.id === id)?.name)
      setPendingDelete(null)
      invalidate()
    },
  })

  const purge = useMutation({
    mutationFn: (payload: { urls?: string[]; all?: boolean }) =>
      cachingApi.purge({ site_id: siteId, ...payload }),
    onSuccess: (result, payload) => {
      toast.success(
        payload.all ? t('pages.caching.purgedAll') : t('pages.caching.purgedUrls'),
        t('pages.caching.purgedCount', { count: formatNumber(result.purged) }),
      )
      if (!payload.all) setPurgeUrls('')
      setPurgeAllOpen(false)
      invalidate()
    },
  })

  const closeDialog = () => {
    setDialogOpen(false)
    setEditing(null)
    setForm(emptyForm())
    setError(null)
  }

  const submit = () => {
    setError(null)
    const name = form.name.trim()
    if (!name) {
      setError(t('pages.caching.nameRequired'))
      return
    }
    save.mutate({
      name,
      match_expression: form.match_expression.trim(),
      edge_ttl_seconds: form.edge_ttl_seconds,
      browser_ttl_seconds: form.browser_ttl_seconds,
      disk_quota_mb: form.disk_quota_mb,
      cache_eligible: form.cache_eligible,
      respect_origin: form.respect_origin,
      enabled: form.enabled,
    })
  }

  const submitPurgeUrls = () => {
    const urls = purgeUrls
      .split('\n')
      .map((u) => u.trim())
      .filter(Boolean)
    if (urls.length === 0) {
      toast.warning(t('pages.caching.purgeEmpty'))
      return
    }
    purge.mutate({ urls })
  }

  const columns: Column<CacheRule>[] = [
    {
      key: 'name',
      header: t('common.name'),
      accessor: (r) => r.name,
      sortable: true,
      cell: (r) => (
        <div className="min-w-0">
          <p className="truncate text-[13px] font-medium text-fg-strong">{r.name}</p>
          <p className="pw-mono truncate text-xs text-fg-subtle">
            {r.match_expression || t('pages.caching.allRequests')}
          </p>
        </div>
      ),
    },
    {
      key: 'edge_ttl',
      header: t('pages.caching.edgeTtl'),
      accessor: (r) => r.edge_ttl_seconds,
      align: 'right',
      width: '1%',
      cell: (r) => (
        <span className="tabular-nums text-[13px] text-fg">
          {formatTtl(r.edge_ttl_seconds, t)}
        </span>
      ),
    },
    {
      key: 'browser_ttl',
      header: t('pages.caching.browserTtl'),
      accessor: (r) => r.browser_ttl_seconds,
      align: 'right',
      width: '1%',
      cell: (r) => (
        <span className="tabular-nums text-[13px] text-fg-subtle">
          {formatTtl(r.browser_ttl_seconds, t)}
        </span>
      ),
    },
    {
      key: 'quota',
      header: t('pages.caching.diskQuota'),
      accessor: (r) => r.disk_quota_mb,
      align: 'right',
      width: '1%',
      cell: (r) => (
        <span className="tabular-nums text-[13px] text-fg-subtle">
          {r.disk_quota_mb > 0 ? `${formatNumber(r.disk_quota_mb)} MB` : t('pages.caching.unlimited')}
        </span>
      ),
    },
    {
      key: 'enabled',
      header: t('common.enabled'),
      accessor: (r) => (r.enabled ? 1 : 0),
      width: '1%',
      cell: (r) => (
        <Switch
          size="sm"
          checked={r.enabled}
          disabled={!canWrite || toggle.isPending}
          aria-label={`${t('common.enabled')}: ${r.name}`}
          onCheckedChange={(enabled) => toggle.mutate({ rule: r, enabled })}
        />
      ),
    },
    {
      key: 'row-actions',
      header: '',
      align: 'right',
      width: '1%',
      cell: (r) => (
        <div className="flex items-center justify-end gap-1">
          <Button
            size="icon"
            variant="ghost"
            aria-label={t('common.edit')}
            disabled={!canWrite}
            onClick={() => {
              setEditing(r)
              setDialogOpen(true)
            }}
            icon={<PencilSimple weight="duotone" className="h-4 w-4" />}
          />
          <Button
            size="icon"
            variant="ghost"
            className="hover:text-fg-danger"
            aria-label={t('common.delete')}
            disabled={!canWrite}
            onClick={() => setPendingDelete(r)}
            icon={<Trash weight="duotone" className="h-4 w-4" />}
          />
        </div>
      ),
    },
  ]

  const stats = statsQuery.data
  const diskPct =
    stats && stats.disk_quota_bytes > 0
      ? Math.min(100, (stats.disk_usage_bytes / stats.disk_quota_bytes) * 100)
      : 0

  return (
    <div className="animate-slide-up">
      <PageHeader
        title={t('pages.caching.title')}
        description={t('pages.caching.description')}
        actions={
          <div className="flex items-center gap-2">
            <Button
              variant="secondary"
              loading={rulesQuery.isFetching}
              onClick={() => {
                rulesQuery.refetch()
                statsQuery.refetch()
              }}
              icon={<ArrowClockwise weight="duotone" className="h-4 w-4" />}
            >
              {t('common.refresh')}
            </Button>
            {canWrite && (
              <Button
                variant="primary"
                icon={<Plus weight="bold" className="h-4 w-4" />}
                onClick={() => {
                  setEditing(null)
                  setDialogOpen(true)
                }}
              >
                {t('pages.caching.addRule')}
              </Button>
            )}
          </div>
        }
      />

      {/* Stats */}
      <div className="mb-6 grid grid-cols-1 gap-4 sm:grid-cols-3">
        {statsQuery.isPending && !stats ? (
          <>
            <SkeletonStat />
            <SkeletonStat />
            <SkeletonStat />
          </>
        ) : (
          <>
            <StatCard
              icon={<Target weight="duotone" className="h-4 w-4" />}
              label={t('pages.caching.hitRate')}
              value={formatPercent(stats?.hit_rate ?? 0)}
              hint={t('pages.caching.hitsMisses', {
                hits: formatNumber(stats?.hits ?? 0),
                misses: formatNumber(stats?.misses ?? 0),
              })}
            />
            <StatCard
              icon={<Database weight="duotone" className="h-4 w-4" />}
              label={t('pages.caching.diskUsage')}
              value={formatSize(stats?.disk_usage_bytes ?? 0)}
              hint={
                stats && stats.disk_quota_bytes > 0
                  ? `${t('pages.caching.of')} ${formatSize(stats.disk_quota_bytes)}`
                  : t('pages.caching.unlimited')
              }
              progress={stats && stats.disk_quota_bytes > 0 ? diskPct : undefined}
            />
            <StatCard
              icon={<Stack weight="duotone" className="h-4 w-4" />}
              label={t('pages.caching.cachedItems')}
              value={formatNumber(stats?.total_items ?? 0)}
              hint={t('pages.caching.cachedItemsHint')}
            />
          </>
        )}
      </div>

      {rulesQuery.isError && !rulesQuery.data ? (
        <ErrorState
          error={rulesQuery.error}
          onRetry={() => rulesQuery.refetch()}
          retrying={rulesQuery.isFetching}
        />
      ) : (
        <Card className="mb-6">
          <CardHeader
            title={t('pages.caching.rulesTitle')}
            description={t('pages.caching.rulesHint')}
          />
          <CardBody className="p-0">
            {rulesQuery.isPending ? (
              <SkeletonRows rows={4} columns={6} />
            ) : rules.length === 0 ? (
              <EmptyState
                className="border-0 py-12"
                icon={<Lightning weight="duotone" className="h-8 w-8" />}
                title={t('pages.caching.empty')}
                description={t('pages.caching.emptyDescription')}
                action={
                  canWrite ? (
                    <Button
                      variant="primary"
                      icon={<Plus weight="bold" className="h-4 w-4" />}
                      onClick={() => {
                        setEditing(null)
                        setDialogOpen(true)
                      }}
                    >
                      {t('pages.caching.addRule')}
                    </Button>
                  ) : undefined
                }
              />
            ) : (
              <Table
                columns={columns}
                data={rules}
                rowKey={(r) => r.id}
                dense
                onRowClick={
                  canWrite
                    ? (r) => {
                        setEditing(r)
                        setDialogOpen(true)
                      }
                    : undefined
                }
              />
            )}
          </CardBody>
        </Card>
      )}

      {/* Purge */}
      <Card>
        <CardHeader
          title={t('pages.caching.purgeTitle')}
          description={t('pages.caching.purgeHint')}
        />
        <CardBody className="flex flex-col gap-4">
          <Textarea
            label={t('pages.caching.purgeUrls')}
            mono
            rows={4}
            disabled={!canWrite}
            value={purgeUrls}
            placeholder={'https://example.com/index.html\nhttps://example.com/assets/app.css'}
            hint={t('pages.caching.purgeUrlsHint')}
            onChange={(e) => setPurgeUrls(e.target.value)}
          />
          <div className="flex flex-wrap items-center gap-2">
            <Button
              variant="primary"
              disabled={!canWrite || purge.isPending}
              loading={purge.isPending}
              icon={<Broom weight="duotone" className="h-4 w-4" />}
              onClick={submitPurgeUrls}
            >
              {t('pages.caching.purgeUrls')}
            </Button>
            <Button
              variant="danger"
              disabled={!canWrite || purge.isPending}
              onClick={() => setPurgeAllOpen(true)}
            >
              {t('pages.caching.purgeAll')}
            </Button>
          </div>
        </CardBody>
      </Card>

      {/* Create / edit */}
      <Dialog
        open={dialogOpen}
        onClose={save.isPending ? () => undefined : closeDialog}
        size="lg"
        title={editing ? t('pages.caching.editRule') : t('pages.caching.addRule')}
        description={t('pages.caching.dialogDescription')}
        footer={
          <>
            <Button variant="ghost" onClick={closeDialog} disabled={save.isPending}>
              {t('common.cancel')}
            </Button>
            <Button variant="primary" onClick={submit} loading={save.isPending}>
              {editing ? t('common.save') : t('common.create')}
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-4">
          <Input
            label={t('common.name')}
            value={form.name}
            autoFocus
            placeholder={t('pages.caching.namePlaceholder')}
            onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))}
            required
          />
          <Input
            label={t('pages.caching.matchExpression')}
            value={form.match_expression}
            className="pw-mono text-[13px]"
            placeholder='http.request.uri.path starts_with "/static"'
            hint={t('pages.caching.matchExpressionHint')}
            onChange={(e) => setForm((f) => ({ ...f, match_expression: e.target.value }))}
          />
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
            <Input
              type="number"
              label={t('pages.caching.edgeTtl')}
              value={form.edge_ttl_seconds}
              min={0}
              hint={t('pages.caching.ttlSecondsHint')}
              onChange={(e) =>
                setForm((f) => ({ ...f, edge_ttl_seconds: Number(e.target.value) }))
              }
            />
            <Input
              type="number"
              label={t('pages.caching.browserTtl')}
              value={form.browser_ttl_seconds}
              min={0}
              onChange={(e) =>
                setForm((f) => ({ ...f, browser_ttl_seconds: Number(e.target.value) }))
              }
            />
            <Input
              type="number"
              label={t('pages.caching.diskQuota')}
              value={form.disk_quota_mb}
              min={0}
              hint={t('pages.caching.diskQuotaHint')}
              onChange={(e) =>
                setForm((f) => ({ ...f, disk_quota_mb: Number(e.target.value) }))
              }
            />
          </div>
          <Switch
            checked={form.cache_eligible}
            onCheckedChange={(cache_eligible) => setForm((f) => ({ ...f, cache_eligible }))}
            label={t('pages.caching.cacheEligible')}
            description={t('pages.caching.cacheEligibleHint')}
          />
          <Switch
            checked={form.respect_origin}
            onCheckedChange={(respect_origin) => setForm((f) => ({ ...f, respect_origin }))}
            label={t('pages.caching.respectOriginLabel')}
            description={t('pages.caching.respectOriginHint')}
          />
          <Switch
            checked={form.enabled}
            onCheckedChange={(enabled) => setForm((f) => ({ ...f, enabled }))}
            label={t('common.enabled')}
          />
          {error && (
            <p
              role="alert"
              className="rounded-md border border-danger/40 bg-danger/8 px-3 py-2 text-[13px] text-fg-danger"
            >
              {error}
            </p>
          )}
        </div>
      </Dialog>

      <ConfirmDialog
        open={pendingDelete !== null}
        onClose={() => setPendingDelete(null)}
        onConfirm={() => pendingDelete && remove.mutate(pendingDelete.id)}
        title={t('pages.caching.deleteTitle')}
        description={t('pages.caching.deleteDescription')}
        confirmLabel={t('common.delete')}
        loading={remove.isPending}
      >
        {pendingDelete && (
          <div className="rounded-md border border-line bg-recessed px-3 py-2">
            <p className="text-[13px] font-medium text-fg-strong">{pendingDelete.name}</p>
          </div>
        )}
      </ConfirmDialog>

      <ConfirmDialog
        open={purgeAllOpen}
        onClose={() => setPurgeAllOpen(false)}
        onConfirm={() => purge.mutate({ all: true })}
        title={t('pages.caching.purgeAllTitle')}
        description={t('pages.caching.purgeAllDescription')}
        confirmLabel={t('pages.caching.purgeAll')}
        loading={purge.isPending}
      />
    </div>
  )
}

function StatCard({
  icon,
  label,
  value,
  hint,
  progress,
}: {
  icon: ReactNode
  label: string
  value: string
  hint?: string
  progress?: number
}) {
  return (
    <Card padded>
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <p className="text-xs font-medium uppercase tracking-wide text-fg-subtle">{label}</p>
          <p className="mt-1.5 text-2xl font-semibold tabular-nums text-fg-strong">{value}</p>
          {hint && <p className="mt-1 truncate text-xs text-fg-subtle">{hint}</p>}
        </div>
        <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-brand-soft text-brand">
          {icon}
        </span>
      </div>
      {progress !== undefined && (
        <div className="mt-3 h-1.5 w-full overflow-hidden rounded-full bg-recessed">
          <div
            className={cn(
              'h-full rounded-full transition-all',
              progress > 85 ? 'bg-danger' : 'bg-brand',
            )}
            style={{ width: `${progress}%` }}
          />
        </div>
      )}
    </Card>
  )
}

export default CachingPage
