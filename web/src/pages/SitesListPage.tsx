import { useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  Plus,
  Globe,
  MagnifyingGlass,
  PencilSimple,
  Trash,
} from '@phosphor-icons/react'
import { PageHeader } from '@/components/PageHeader'
import { Card, CardBody } from '@/components/ui/Card'
import { Button } from '@/components/ui/Button'
import { Input } from '@/components/ui/Input'
import { Select } from '@/components/ui/Select'
import { Badge } from '@/components/ui/Badge'
import { Table, type Column } from '@/components/ui/Table'
import { Dialog } from '@/components/ui/Dialog'
import { ConfirmDialog } from '@/components/ui/ConfirmDialog'
import { SkeletonRows } from '@/components/ui/Skeleton'
import { EmptyState } from '@/components/ui/EmptyState'
import { useToast } from '@/components/ui/Toast'
import { ErrorState } from '@/components/ErrorState'
import { sitesApi, siteKeys } from '@/api/sites'
import { analyticsApi, analyticsKeys } from '@/api/analytics'
import { useCanWrite, useDebouncedValue } from '@/hooks'
import { formatCompactNumber, formatDateTime, formatNumber } from '@/lib/format'
import type { CreateSiteRequest, Site, SiteStatus } from '@/api/types'

const STATUS_TONE: Record<SiteStatus, 'success' | 'warning' | 'neutral'> = {
  active: 'success',
  paused: 'warning',
  pending: 'neutral',
}

const PLANS = ['free', 'pro', 'business', 'enterprise']

interface SiteFormState {
  name: string
  domain: string
  plan: string
  status: string
}

const emptyForm: SiteFormState = {
  name: '',
  domain: '',
  plan: 'free',
  status: 'active',
}

export function SitesListPage() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const toast = useToast()
  const queryClient = useQueryClient()
  const canWrite = useCanWrite()

  const [query, setQuery] = useState('')
  const debouncedQuery = useDebouncedValue(query.trim(), 350)

  const [dialogOpen, setDialogOpen] = useState(false)
  const [editing, setEditing] = useState<Site | null>(null)
  const [form, setForm] = useState<SiteFormState>(emptyForm)
  const [formError, setFormError] = useState<string | null>(null)
  const [pendingDelete, setPendingDelete] = useState<Site | null>(null)

  const sitesQuery = useQuery({
    queryKey: siteKeys.list({ search: debouncedQuery || undefined }),
    queryFn: () => sitesApi.list({ search: debouncedQuery || undefined }),
    select: (page) => page.items,
  })

  // Per-site request/blocked counters live in analytics, not in `/sites`.
  const trafficQuery = useQuery({
    queryKey: analyticsKeys.sites({}),
    queryFn: () => analyticsApi.sitesOverview(),
    staleTime: 60_000,
  })

  const trafficBySite = useMemo(() => {
    const map = new Map<string, { requests: number; blocked: number }>()
    for (const row of trafficQuery.data ?? []) {
      map.set(row.site_id, { requests: row.requests, blocked: row.blocked })
    }
    return map
  }, [trafficQuery.data])

  const sites = sitesQuery.data ?? []

  const invalidate = () => {
    void queryClient.invalidateQueries({ queryKey: siteKeys.all })
    void queryClient.invalidateQueries({ queryKey: analyticsKeys.all })
  }

  const createSite = useMutation({
    mutationFn: (data: CreateSiteRequest) => sitesApi.create(data),
    onSuccess: (site) => {
      toast.success(t('pages.sites.created'), site.domain)
      closeDialog()
      invalidate()
      navigate(`/sites/${site.id}/security/waf`)
    },
  })

  const updateSite = useMutation({
    mutationFn: ({ id, data }: { id: string; data: CreateSiteRequest }) =>
      sitesApi.update(id, data),
    onSuccess: (site) => {
      toast.success(t('pages.sites.updated'), site.domain)
      closeDialog()
      invalidate()
    },
  })

  const deleteSite = useMutation({
    mutationFn: (id: string) => sitesApi.delete(id),
    onSuccess: (_data, id) => {
      const removed = sites.find((s) => s.id === id)
      toast.success(t('pages.sites.deleted'), removed?.domain)
      setPendingDelete(null)
      invalidate()
    },
  })

  const openCreate = () => {
    setEditing(null)
    setForm(emptyForm)
    setFormError(null)
    setDialogOpen(true)
  }

  const openEdit = (site: Site) => {
    setEditing(site)
    setForm({
      name: site.name,
      domain: site.domain,
      plan: site.plan,
      status: site.status,
    })
    setFormError(null)
    setDialogOpen(true)
  }

  const closeDialog = () => {
    setDialogOpen(false)
    setEditing(null)
    setForm(emptyForm)
    setFormError(null)
  }

  const submit = () => {
    setFormError(null)
    const domain = form.domain.trim().toLowerCase().replace(/^https?:\/\//, '').replace(/\/.*$/, '')
    const name = form.name.trim()

    if (!name) {
      setFormError(t('pages.sites.nameRequired'))
      return
    }
    if (!/^(?=.{1,253}$)([a-z0-9]([a-z0-9-]*[a-z0-9])?\.)+[a-z]{2,}$/.test(domain)) {
      setFormError(t('pages.sites.domainInvalid'))
      return
    }

    const payload: CreateSiteRequest = { name, domain, plan: form.plan, status: form.status }
    if (editing) updateSite.mutate({ id: editing.id, data: payload })
    else createSite.mutate(payload)
  }

  const pending = createSite.isPending || updateSite.isPending

  const columns: Column<Site>[] = [
    {
      key: 'domain',
      header: t('pages.sites.domain'),
      accessor: (r) => r.domain,
      sortable: true,
      cell: (r) => (
        <div className="flex items-center gap-3">
          <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-brand-soft text-brand">
            <Globe weight="duotone" className="h-4 w-4" />
          </span>
          <div className="min-w-0">
            <p className="pw-mono truncate font-medium text-fg-strong">{r.domain}</p>
            <p className="truncate text-xs text-fg-subtle">{r.name}</p>
          </div>
        </div>
      ),
    },
    {
      key: 'status',
      header: t('common.status'),
      accessor: (r) => r.status,
      cell: (r) => (
        <Badge tone={STATUS_TONE[r.status as SiteStatus] ?? 'neutral'} dot>
          {t(`status.${r.status}`, r.status)}
        </Badge>
      ),
    },
    {
      key: 'plan',
      header: t('pages.sites.plan'),
      accessor: (r) => r.plan,
      cell: (r) => <span className="text-[13px] capitalize text-fg-subtle">{r.plan}</span>,
    },
    {
      key: 'requests',
      header: t('pages.sites.requests'),
      align: 'right',
      sortable: true,
      accessor: (r) => trafficBySite.get(r.id)?.requests ?? 0,
      cell: (r) => (
        <span className="tabular-nums text-[13px]">
          {trafficQuery.isPending
            ? '…'
            : formatCompactNumber(trafficBySite.get(r.id)?.requests ?? 0)}
        </span>
      ),
    },
    {
      key: 'blocked',
      header: t('pages.sites.blocked'),
      align: 'right',
      sortable: true,
      accessor: (r) => trafficBySite.get(r.id)?.blocked ?? 0,
      cell: (r) => (
        <span className="tabular-nums text-[13px] text-fg-danger">
          {trafficQuery.isPending
            ? '…'
            : formatCompactNumber(trafficBySite.get(r.id)?.blocked ?? 0)}
        </span>
      ),
    },
    {
      key: 'created_at',
      header: t('pages.sites.added'),
      accessor: (r) => r.created_at,
      sortable: true,
      cell: (r) => (
        <span className="text-[13px] text-fg-subtle">{formatDateTime(r.created_at)}</span>
      ),
    },
    {
      key: 'actions',
      header: '',
      align: 'right',
      width: '1%',
      cell: (r) => (
        <div className="flex items-center justify-end gap-1" onClick={(e) => e.stopPropagation()}>
          <Button
            size="icon"
            variant="ghost"
            aria-label={t('common.edit')}
            disabled={!canWrite}
            onClick={() => openEdit(r)}
            icon={<PencilSimple weight="duotone" className="h-4 w-4" />}
          />
          <Button
            size="icon"
            variant="ghost"
            aria-label={t('common.delete')}
            disabled={!canWrite}
            className="hover:text-fg-danger"
            onClick={() => setPendingDelete(r)}
            icon={<Trash weight="duotone" className="h-4 w-4" />}
          />
        </div>
      ),
    },
  ]

  return (
    <div className="animate-slide-up">
      <PageHeader
        title={t('pages.sites.title')}
        description={t('pages.sites.description')}
        actions={
          canWrite ? (
            <Button
              variant="primary"
              icon={<Plus weight="bold" className="h-4 w-4" />}
              onClick={openCreate}
            >
              {t('pages.sites.addSite')}
            </Button>
          ) : undefined
        }
      />

      <div className="mb-4 flex flex-wrap items-center gap-3">
        <div className="w-full max-w-sm">
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t('pages.sites.searchPlaceholder')}
            prefixIcon={<MagnifyingGlass weight="duotone" />}
            aria-label={t('common.search')}
          />
        </div>
        {!sitesQuery.isPending && (
          <span className="text-[13px] text-fg-subtle tabular-nums">
            {t('pages.sites.count', { count: sites.length })}
          </span>
        )}
      </div>

      {sitesQuery.isError && !sitesQuery.data ? (
        <ErrorState
          error={sitesQuery.error}
          onRetry={() => sitesQuery.refetch()}
          retrying={sitesQuery.isFetching}
        />
      ) : (
        <Card>
          <CardBody className="p-0">
            {sitesQuery.isPending ? (
              <SkeletonRows rows={6} columns={6} />
            ) : sites.length === 0 ? (
              debouncedQuery ? (
                <EmptyState
                  className="py-14"
                  icon={<MagnifyingGlass weight="duotone" className="h-8 w-8" />}
                  title={t('pages.sites.noResults')}
                  description={t('pages.sites.noResultsDescription', { query: debouncedQuery })}
                  action={
                    <Button variant="secondary" onClick={() => setQuery('')}>
                      {t('common.reset')}
                    </Button>
                  }
                />
              ) : (
                <EmptyState
                  className="py-14"
                  icon={<Globe weight="duotone" className="h-8 w-8" />}
                  title={t('pages.sites.emptyTitle')}
                  description={t('pages.sites.emptyDescription')}
                  action={
                    canWrite ? (
                      <Button
                        variant="primary"
                        icon={<Plus weight="bold" className="h-4 w-4" />}
                        onClick={openCreate}
                      >
                        {t('pages.sites.addSite')}
                      </Button>
                    ) : undefined
                  }
                />
              )
            ) : (
              <Table
                columns={columns}
                data={sites}
                rowKey={(r) => r.id}
                pageSize={20}
                onRowClick={(r) => navigate(`/sites/${r.id}/security/waf`)}
              />
            )}
          </CardBody>
        </Card>
      )}

      {/* Create / edit dialog */}
      <Dialog
        open={dialogOpen}
        onClose={pending ? () => undefined : closeDialog}
        title={editing ? t('pages.sites.editSite') : t('pages.sites.addSite')}
        description={editing ? t('pages.sites.editDescription') : t('pages.sites.createDescription')}
        footer={
          <>
            <Button variant="ghost" onClick={closeDialog} disabled={pending}>
              {t('common.cancel')}
            </Button>
            <Button variant="primary" onClick={submit} loading={pending}>
              {editing ? t('common.save') : t('common.create')}
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-4">
          <Input
            label={t('common.name')}
            value={form.name}
            placeholder={t('pages.sites.namePlaceholder')}
            onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))}
            autoFocus
            required
          />
          <Input
            label={t('pages.sites.domain')}
            value={form.domain}
            placeholder="example.com"
            hint={t('pages.sites.domainHint')}
            error={formError ?? undefined}
            prefixIcon={<Globe weight="duotone" />}
            onChange={(e) => setForm((f) => ({ ...f, domain: e.target.value }))}
            required
          />
          <div className="grid grid-cols-2 gap-3">
            <Select
              label={t('pages.sites.plan')}
              value={form.plan}
              options={PLANS.map((p) => ({ value: p, label: t(`plans.${p}`, p) }))}
              onChange={(e) => setForm((f) => ({ ...f, plan: e.target.value }))}
            />
            <Select
              label={t('common.status')}
              value={form.status}
              options={['active', 'paused', 'pending'].map((s) => ({
                value: s,
                label: t(`status.${s}`, s),
              }))}
              onChange={(e) => setForm((f) => ({ ...f, status: e.target.value }))}
            />
          </div>
        </div>
      </Dialog>

      {/* Delete confirmation */}
      <ConfirmDialog
        open={pendingDelete !== null}
        onClose={() => setPendingDelete(null)}
        onConfirm={() => pendingDelete && deleteSite.mutate(pendingDelete.id)}
        title={t('pages.sites.deleteTitle')}
        description={t('pages.sites.deleteDescription')}
        confirmLabel={t('common.delete')}
        loading={deleteSite.isPending}
      >
        {pendingDelete && (
          <div className="rounded-md border border-line bg-recessed px-3 py-2">
            <p className="pw-mono text-[13px] font-medium text-fg-strong">{pendingDelete.domain}</p>
            <p className="mt-0.5 text-xs text-fg-subtle">
              {t('pages.sites.deleteMeta', {
                requests: formatNumber(trafficBySite.get(pendingDelete.id)?.requests ?? 0),
              })}
            </p>
          </div>
        )}
      </ConfirmDialog>
    </div>
  )
}
