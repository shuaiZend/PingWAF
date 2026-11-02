import { useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  ArrowsClockwise,
  Certificate as CertificateIcon,
  FileArrowUp,
  Globe,
  Info,
  Lightning,
  Link as LinkIcon,
  LinkBreak,
  Plus,
  Trash,
  Warning,
} from '@phosphor-icons/react'
import { PageHeader } from '@/components/PageHeader'
import { ErrorState } from '@/components/ErrorState'
import { CertWarningsBadge } from '@/components/ssl/CertWarnings'
import { Badge, type BadgeTone } from '@/components/ui/Badge'
import { Button } from '@/components/ui/Button'
import { Card, CardBody, CardHeader } from '@/components/ui/Card'
import { ConfirmDialog } from '@/components/ui/ConfirmDialog'
import { Dialog } from '@/components/ui/Dialog'
import { EmptyState } from '@/components/ui/EmptyState'
import { Input } from '@/components/ui/Input'
import { Select } from '@/components/ui/Select'
import { SkeletonStat } from '@/components/ui/Skeleton'
import { Switch } from '@/components/ui/Switch'
import { Table, type Column } from '@/components/ui/Table'
import { Tabs } from '@/components/ui/Tabs'
import { Textarea } from '@/components/ui/Textarea'
import { useToast } from '@/components/ui/Toast'
import { errorMessage } from '@/api/errors'
import { ApiError } from '@/api/client'
import { sslApi, sslKeys, daysUntilExpiry, expiryTone } from '@/api/ssl'
import { useCanWrite, useNow, useSitesList } from '@/hooks'
import { formatDateTime } from '@/lib/format'
import { bindingKind, deleteBlockers, mergeBindResults } from '@/lib/certFlow'
import {
  ACME_CHALLENGE_TYPES,
  ACME_DNS_PROVIDERS,
  CERT_EVENT_TYPES,
  CERTIFICATE_STATUSES,
  type CertificateEvent,
  type CertificateListQuery,
  type CertificateWithSite,
  type CertificateUsage,
  type CreateSslRequest,
} from '@/api/types'

/** One per-site outcome of the batch bind; failures keep the dialog open. */
interface BindAttempt {
  siteId: string
  ok: boolean
  /** Human-readable failure reason; present when `ok` is false. */
  error?: string
}

const STATUS_TONE: Record<string, BadgeTone> = {
  active: 'success',
  pending: 'info',
  expired: 'warning',
  failed: 'danger',
}

type Mode = 'acme' | 'manual'

interface FormState {
  mode: Mode
  siteId: string
  domain: string
  autoRenew: boolean
  acmeEmail: string
  challengeType: string
  dnsProvider: string
  dnsConfig: Array<{ key: string; value: string }>
  certPem: string
  keyPem: string
  activate: boolean
}

const emptyForm = (): FormState => ({
  mode: 'acme',
  siteId: '',
  domain: '',
  autoRenew: true,
  acmeEmail: '',
  challengeType: 'http-01',
  dnsProvider: '',
  dnsConfig: [],
  certPem: '',
  keyPem: '',
  activate: true,
})

/** ACME-issued rows carry an email or auto-renewal; the rest were uploaded. */
function isAcme(cert: CertificateWithSite): boolean {
  return cert.auto_renew || Boolean(cert.acme_email)
}

export function GlobalSslPage() {
  const { t } = useTranslation()
  const toast = useToast()
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const canWrite = useCanWrite()
  const now = useNow(60_000)

  const [siteFilter, setSiteFilter] = useState('')
  const [statusFilter, setStatusFilter] = useState('')
  const [tab, setTab] = useState('certificates')
  const [dialogOpen, setDialogOpen] = useState(false)
  const [form, setForm] = useState<FormState>(emptyForm())
  const [formError, setFormError] = useState<string | null>(null)
  const [pendingDelete, setPendingDelete] = useState<CertificateWithSite | null>(null)
  const [eventTypeFilter, setEventTypeFilter] = useState('')
  const [bindTarget, setBindTarget] = useState<CertificateWithSite | null>(null)
  const [bindSelection, setBindSelection] = useState<string[]>([])
  const [bindError, setBindError] = useState<string | null>(null)
  /** Per-site outcomes of the last batch bind; non-empty keeps the dialog open. */
  const [bindResults, setBindResults] = useState<BindAttempt[]>([])
  /** Site whose unbind hit a 409 (it still serves the certificate). */
  const [unbindConflictSite, setUnbindConflictSite] = useState<string | null>(null)
  /** Bindings queued for the two-step unbind confirmation. */
  const [pendingUnbind, setPendingUnbind] = useState<{ certId: string; siteId: string } | null>(null)

  const sitesQuery = useSitesList()
  const sites = sitesQuery.data ?? []

  const filter: CertificateListQuery = {
    ...(siteFilter ? { site_id: siteFilter } : {}),
    ...(statusFilter ? { status: statusFilter } : {}),
  }

  const summaryQuery = useQuery({
    queryKey: sslKeys.summary(),
    queryFn: () => sslApi.summary(),
    // Issuance is asynchronous: while any order is pending, keep the
    // numbers moving so a success (or the stale-pending failure) shows up
    // without a manual refresh.
    refetchInterval: (query) =>
      (query.state.data?.pending ?? 0) > 0 ? 10_000 : false,
  })
  const certsQuery = useQuery({
    queryKey: sslKeys.global(filter),
    queryFn: () => sslApi.listAll(filter),
    refetchInterval: (query) =>
      query.state.data?.items.some((cert) => cert.status === 'pending')
        ? 10_000
        : false,
  })

  const certs = useMemo(() => certsQuery.data?.items ?? [], [certsQuery.data])
  const hasPending = certs.some((cert) => cert.status === 'pending')

  const eventsQuery = useQuery({
    queryKey: sslKeys.events({
      event_type: eventTypeFilter || undefined,
    }),
    queryFn: () =>
      sslApi.listAllEvents({
        event_type: eventTypeFilter || undefined,
      }),
    // The issuance log grows while an order runs; poll alongside the list.
    refetchInterval: hasPending ? 10_000 : false,
  })

  const summary = summaryQuery.data

  // `usage` gives the owner / linked / serving split (with `selected` flags)
  // that the list rows do not carry; the dialogs fall back to `bound_sites`
  // when it cannot be fetched.
  const bindUsageQuery = useQuery({
    queryKey: sslKeys.usage(bindTarget?.id ?? 'none'),
    queryFn: () => sslApi.usage(bindTarget!.id),
    enabled: bindTarget !== null,
  })
  const deleteUsageQuery = useQuery({
    queryKey: sslKeys.usage(pendingDelete?.id ?? 'none'),
    queryFn: () => sslApi.usage(pendingDelete!.id),
    enabled: pendingDelete !== null,
  })
  const bindUsage: CertificateUsage | undefined = bindUsageQuery.data
  const deleteUsage = deleteUsageQuery.data
  // Conservative delete guard: while `usage` is loading or failed we cannot
  // prove the certificate is unused, so treat it as blocked instead of
  // letting a delete through to a 409. `bound_sites` from the list row is a
  // fallback signal when the usage query has not resolved yet.
  const deleteGuard = deleteBlockers(
    pendingDelete ?? { site_id: null },
    deleteUsage,
  )
  const deleteBlocked = pendingDelete === null || deleteGuard.blocked
  /** True only when we know which sites pin the cert (serving / linked). */
  const deleteHasBlockingSites =
    deleteGuard.reason === 'serving' || deleteGuard.reason === 'linked'
  /** First blocking site to navigate to when the confirm turns into "go handle". */
  const firstBlockingSite = deleteGuard.sites[0]
  const filtersActive = Boolean(siteFilter || statusFilter)

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: sslKeys.all })
    queryClient.invalidateQueries({ queryKey: ['sites'] })
  }

  const createCert = useMutation({
    mutationFn: (payload: CreateSslRequest) =>
      sslApi.createGlobal(payload),
    onSuccess: (cert) => {
      toast.success(
        t('pages.ssl.certCreated'),
        form.mode === 'acme'
          ? t('pages.sslGlobal.acmeStarted', { domain: cert.domain })
          : cert.domain,
      )
      setDialogOpen(false)
      setForm(emptyForm())
      invalidate()
      if (cert.warnings && cert.warnings.length > 0) {
        toast.warning(
          t('pages.ssl.certDomainWarnings', { count: cert.warnings.length }),
          cert.warnings.join('\n'),
        )
      }
    },
    onError: (e) => setFormError(errorMessage(e)),
  })

  const renewCert = useMutation({
    mutationFn: (id: string) => sslApi.renewGlobal(id),
    onSuccess: (_ack, _id) => {
      toast.success(t('pages.ssl.renewStarted'), t('pages.sslGlobal.renewHint'))
      invalidate()
    },
    onError: (e) => toast.error(t('pages.sslGlobal.renewFailed'), errorMessage(e)),
  })

  const deleteCert = useMutation({
    mutationFn: (id: string) => sslApi.deleteGlobal(id),
    onSuccess: () => {
      toast.success(t('pages.ssl.certDeleted'), pendingDelete?.domain)
      setPendingDelete(null)
      invalidate()
    },
    onError: (e) => toast.error(t('pages.sslGlobal.deleteFailed'), errorMessage(e)),
  })

  /** Error text for one failed bind attempt; 409 means the site owns the cert. */
  const bindFailMessage = (e: unknown): string => {
    if (e instanceof ApiError) {
      if (e.status === 400) return errorMessage(e)
      if (e.status === 409) return t('pages.sslGlobal.alreadyOwner')
      return errorMessage(e)
    }
    return e instanceof Error ? e.message : String(e)
  }

  /**
   * One request per newly selected site, collected independently — a single
   * failure must not abort the batch. Requests run **serially** so each site
   * triggers at most one config touch / agent push at a time instead of N
   * concurrent config bumps. Successes invalidate, failures stay in the
   * dialog's result area.
   */
  const bindCert = useMutation({
    mutationFn: async (ids: string[]) => {
      const target = bindTarget
      if (!target) return { target: null, results: [] as BindAttempt[] }
      const results: BindAttempt[] = []
      for (const siteId of ids) {
        try {
          await sslApi.bind(target.id, { site_id: siteId })
          results.push({ siteId, ok: true })
        } catch (e) {
          results.push({ siteId, ok: false, error: bindFailMessage(e) })
        }
      }
      return { target, results }
    },
    onSuccess: ({ target, results }) => {
      const failures = results.filter((r) => !r.ok)
      if (failures.length === 0) {
        toast.success(t('pages.sslGlobal.bindSites'), target?.domain)
        setBindTarget(null)
        setBindSelection([])
        setBindError(null)
        setBindResults([])
        invalidate()
        return
      }
      // Keep the dialog open with the per-site outcomes; drop the sites that
      // succeeded from the selection so a retry only resubmits the rest.
      setBindResults(results)
      setBindSelection((prev) =>
        prev.filter((siteId) => failures.some((f) => f.siteId === siteId)),
      )
      invalidate()
    },
  })

  const unbindCert = useMutation({
    mutationFn: ({ certId, siteId }: { certId: string; siteId: string }) =>
      sslApi.unbind(certId, siteId),
    onSuccess: () => {
      setBindError(null)
      setUnbindConflictSite(null)
      setPendingUnbind(null)
      invalidate()
    },
    onError: (e, { siteId }) => {
      // 409 — the site still serves this certificate; guide to its SSL page
      // inline instead of only toasting.
      if (e instanceof ApiError && e.status === 409) {
        setUnbindConflictSite(siteId)
        setPendingUnbind(null)
        toast.error(t('pages.sslGlobal.unbindBlocked'), errorMessage(e))
        return
      }
      toast.error(t('pages.sslGlobal.bindSites'), errorMessage(e))
    },
  })

  const openCreate = () => {
    setForm(emptyForm())
    setFormError(null)
    setDialogOpen(true)
  }

  const submit = () => {
    const domain = form.domain.trim()
    const siteId = form.siteId
    // ACME issuance serves a site's hostnames through its challenge, so it
    // needs a site up front; manual PEM uploads can stay unbound for now.
    if (form.mode === 'acme' && !siteId) {
      setFormError(t('pages.sslGlobal.acmeNeedsSite'))
      return
    }
    if (!domain) {
      setFormError(t('pages.ssl.domainRequired'))
      return
    }
    const payload: CreateSslRequest = {
      ...(siteId ? { site_id: siteId } : {}),
      domain,
      // `activate` switches the site's TLS choice, which needs a site.
      ...(siteId ? { activate: form.activate } : {}),
    }
    if (form.mode === 'acme') {
      payload.auto_renew = form.autoRenew
      payload.acme_challenge_type = form.challengeType
      const email = form.acmeEmail.trim()
      if (email) payload.acme_email = email
      if (form.challengeType === 'dns-01' && form.dnsProvider) {
        payload.acme_dns_provider = form.dnsProvider
        const config = Object.fromEntries(
          form.dnsConfig
            .map((row) => [row.key.trim(), row.value.trim()] as const)
            .filter(([key]) => key.length > 0),
        )
        if (Object.keys(config).length > 0) {
          payload.acme_dns_config = config
        }
      }
    } else {
      const certPem = form.certPem.trim()
      const keyPem = form.keyPem.trim()
      if (!certPem || !keyPem) {
        setFormError(t('pages.ssl.pemRequired'))
        return
      }
      payload.cert_pem = certPem
      payload.key_pem = keyPem
      payload.auto_renew = false
    }
    setFormError(null)
    createCert.mutate(payload)
  }

  const expiryCell = (cert: CertificateWithSite) => {
    const days = daysUntilExpiry(cert.expires_at, now)
    if (days === null) return <span className="text-fg-subtle">—</span>
    return (
      <div className="flex flex-col gap-0.5">
        <span className="text-fg">{formatDateTime(cert.expires_at)}</span>
        <Badge tone={expiryTone(days)} size="sm">
          {days < 0
            ? t('pages.sslGlobal.expiredAgo', { days: Math.abs(days) })
            : t('pages.ssl.daysLeft', { days })}
        </Badge>
      </div>
    )
  }

  const actionCell = (cert: CertificateWithSite) => (
    <div className="flex items-center justify-end gap-1">
      <Button
        size="sm"
        variant="ghost"
        disabled={!canWrite}
        onClick={() => {
          setBindTarget(cert)
          setBindSelection([])
          setBindError(null)
          setBindResults([])
          setUnbindConflictSite(null)
        }}
      >
        <LinkIcon weight="duotone" className="h-4 w-4" />
        {t('pages.sslGlobal.bindSites')}
      </Button>
      {isAcme(cert) && (
        <Button
          size="sm"
          variant="ghost"
          disabled={!canWrite || renewCert.isPending}
          loading={renewCert.isPending && renewCert.variables === cert.id}
          onClick={() => renewCert.mutate(cert.id)}
        >
          <ArrowsClockwise weight="duotone" className="h-4 w-4" />
          {t('pages.ssl.renew')}
        </Button>
      )}
      <Button
        size="icon"
        variant="ghost"
        aria-label={t('common.delete')}
        disabled={!canWrite || deleteCert.isPending}
        onClick={() => setPendingDelete(cert)}
      >
        <Trash weight="duotone" className="h-4 w-4 text-fg-danger" />
      </Button>
    </div>
  )

  const columns: Column<CertificateWithSite>[] = [
    {
      key: 'domain',
      header: t('pages.ssl.domain'),
      accessor: (r) => r.domain,
      sortable: true,
      cell: (r) => (
        <div className="flex flex-col gap-0.5">
          <span className="flex items-center gap-2 font-medium text-fg-strong">
            {r.domain}
            <CertWarningsBadge warnings={r.warnings} />
          </span>
          <span className="text-xs text-fg-subtle">
            {t(`pages.sslGlobal.status_${r.status}`, { defaultValue: r.status })}
          </span>
        </div>
      ),
    },
    {
      key: 'site',
      header: t('pages.sslGlobal.site'),
      accessor: (r) => r.site_domain ?? '',
      cell: (r) => {
        // Owner site first (its domain), then the share count. A certificate
        // with no owner and no shares is the only "unbound" case.
        if (r.site_id === null && (r.bound_count ?? 0) === 0) {
          return (
            <Badge tone="warning" size="sm" dot>
              {t('pages.sslGlobal.unbound')}
            </Badge>
          )
        }
        return (
          <div className="flex flex-col gap-0.5">
            <span className="text-fg">
              {r.site_id
                ? t('pages.sslGlobal.ownerLine', { domain: r.site_domain ?? '—' })
                : t('pages.sslGlobal.unbound')}
            </span>
            {(r.bound_count ?? 0) > 0 && (
              <span className="text-xs text-fg-subtle">
                {t('pages.sslGlobal.sharedCount', { count: r.bound_count })}
              </span>
            )}
          </div>
        )
      },
    },
    {
      key: 'type',
      header: t('common.type'),
      accessor: (r) => (isAcme(r) ? 'acme' : 'manual'),
      cell: (r) =>
        isAcme(r) ? (
          <Badge tone="brand" size="sm">
            <Lightning weight="duotone" className="h-3.5 w-3.5" />
            {t('pages.ssl.acme')}
          </Badge>
        ) : (
          <Badge tone="neutral" size="sm">
            <FileArrowUp weight="duotone" className="h-3.5 w-3.5" />
            {t('pages.ssl.manualUpload')}
          </Badge>
        ),
    },
    {
      key: 'issuer',
      header: t('pages.ssl.issuer'),
      accessor: (r) => r.issuer ?? '',
      cell: (r) => (
        <span className="text-fg-subtle">{r.issuer || t('pages.ssl.unknownIssuer')}</span>
      ),
    },
    {
      key: 'status',
      header: t('common.status'),
      accessor: (r) => r.status,
      cell: (r) => {
        // `updated_at` is when the order was last (re)armed; the control
        // plane fails a certificate that stays pending past its grace
        // window, so a long wait is worth highlighting before that.
        const waitingMinutes =
          r.status === 'pending'
            ? Math.max(
                0,
                Math.floor(
                  (now - new Date(r.updated_at).getTime()) / 60_000,
                ),
              )
            : 0
        return (
          <div className="flex flex-col items-start gap-0.5">
            <Badge tone={STATUS_TONE[r.status] ?? 'neutral'} dot size="sm">
              {t(`pages.sslGlobal.status_${r.status}`, {
                defaultValue: r.status,
              })}
            </Badge>
            {r.status === 'pending' && (
              <span
                className={
                  waitingMinutes >= 10
                    ? 'text-xs text-fg-warning'
                    : 'text-xs text-fg-subtle'
                }
              >
                {t('pages.sslGlobal.pendingFor', {
                  minutes: waitingMinutes,
                })}
              </span>
            )}
            {r.status === 'failed' && r.failure_reason && (
              <span
                className="line-clamp-2 max-w-56 text-xs text-fg-danger"
                title={r.failure_reason}
              >
                {r.failure_reason}
              </span>
            )}
          </div>
        )
      },
    },
    {
      key: 'expires',
      header: t('pages.ssl.expires'),
      accessor: (r) => r.expires_at ?? '',
      sortable: true,
      cell: expiryCell,
    },
    {
      key: 'actions',
      header: '',
      align: 'right',
      width: '200px',
      cell: actionCell,
    },
  ]

  const EVENT_TONE: Record<string, BadgeTone> = {
    created: 'success',
    renewal_requested: 'info',
    renewed: 'success',
    failed: 'danger',
    issuance_failed: 'danger',
    deleted: 'warning',
    acme_raw: 'brand',
  }

  const events = useMemo(() => eventsQuery.data?.items ?? [], [eventsQuery.data])

  const logColumns: Column<CertificateEvent>[] = [
    {
      key: 'created_at',
      header: t('pages.sslGlobal.eventTime'),
      accessor: (r) => r.created_at,
      sortable: true,
      width: '180px',
      cell: (r) => (
        <span className="text-fg">{formatDateTime(r.created_at)}</span>
      ),
    },
    {
      key: 'event_type',
      header: t('pages.sslGlobal.eventType'),
      accessor: (r) => r.event_type,
      cell: (r) => (
        <Badge tone={EVENT_TONE[r.event_type] ?? 'neutral'} dot size="sm">
          {t(`pages.sslGlobal.event_${r.event_type}`, { defaultValue: r.event_type })}
        </Badge>
      ),
    },
    {
      key: 'domain',
      header: t('pages.sslGlobal.eventDomain'),
      accessor: (r) => r.domain ?? '',
      cell: (r) => (
        <span className="pw-mono text-[13px] text-fg-strong">
          {r.domain ?? '—'}
        </span>
      ),
    },
    {
      key: 'site_domain',
      header: t('pages.sslGlobal.eventSite'),
      accessor: (r) => r.site_domain ?? '',
      cell: (r) => (
        <span className="text-fg-subtle">{r.site_domain ?? '—'}</span>
      ),
    },
    {
      key: 'message',
      header: t('pages.sslGlobal.eventMessage'),
      accessor: (r) => r.message,
      cell: (r) => (
        <span
          className={
            r.event_type === 'acme_raw'
              ? 'pw-mono text-[12px] text-fg-subtle line-clamp-2'
              : 'text-[13px] text-fg-subtle line-clamp-2'
          }
        >
          {r.message}
        </span>
      ),
    },
  ]

  const siteOptions = [
    { value: '', label: t('pages.sslGlobal.allSites') },
    ...sites.map((s) => ({ value: s.id, label: s.domain })),
  ]
  const statusOptions = [
    { value: '', label: t('pages.sslGlobal.allStatuses') },
    ...CERTIFICATE_STATUSES.map((s) => ({
      value: s,
      label: t(`pages.sslGlobal.status_${s}`, { defaultValue: s }),
    })),
  ]

  const stats = [
    { key: 'total', label: t('pages.sslGlobal.statTotal'), value: summary?.total },
    { key: 'active', label: t('pages.sslGlobal.statActive'), value: summary?.active },
    {
      key: 'expiring',
      label: t('pages.sslGlobal.statExpiring'),
      value: summary?.expiring_soon,
      tone: 'warning' as const,
    },
    {
      key: 'problem',
      label: t('pages.sslGlobal.statProblem'),
      value: summary ? summary.failed + summary.expired : undefined,
      tone: 'danger' as const,
    },
  ]

  const error = certsQuery.error ?? summaryQuery.error

  return (
    <div className="animate-slide-up">
      <PageHeader
        title={t('pages.sslGlobal.title')}
        description={t('pages.sslGlobal.description')}
        actions={
          canWrite && (
            <Button variant="primary" onClick={openCreate}>
              <Plus weight="bold" className="h-4 w-4" />
              {t('pages.sslGlobal.addCertificate')}
            </Button>
          )
        }
      />

      <div className="mb-6 flex flex-wrap items-center gap-2 rounded-lg border border-line bg-recessed/40 px-3 py-2 text-[13px] text-fg-subtle">
        <Info weight="duotone" className="h-4 w-4 shrink-0 text-link" />
        <span>{t('pages.sslGlobal.scopeNote')}</span>
      </div>

      <div className="mb-6 grid grid-cols-2 gap-4 lg:grid-cols-4">
        {stats.map((stat) =>
          stat.value === undefined ? (
            <SkeletonStat key={stat.key} />
          ) : (
            <Card key={stat.key}>
              <CardBody className="flex flex-col gap-1 py-4">
                <span className="text-[13px] text-fg-subtle">{stat.label}</span>
                <span
                  className={
                    stat.tone === 'danger' && stat.value > 0
                      ? 'text-2xl font-semibold text-fg-danger'
                      : stat.tone === 'warning' && stat.value > 0
                        ? 'text-2xl font-semibold text-fg-warning'
                        : 'text-2xl font-semibold text-fg-strong'
                  }
                >
                  {stat.value}
                </span>
              </CardBody>
            </Card>
          ),
        )}
      </div>

      {error ? (
        <ErrorState
          error={error}
          onRetry={() => {
            certsQuery.refetch()
            summaryQuery.refetch()
          }}
          retrying={certsQuery.isFetching || summaryQuery.isFetching}
        />
      ) : (
        <Tabs
          value={tab}
          onChange={setTab}
          items={[
            {
              value: 'certificates',
              label: t('pages.sslGlobal.tabCertificates'),
              icon: <CertificateIcon weight="duotone" className="h-4 w-4" />,
            },
            {
              value: 'log',
              label: t('pages.sslGlobal.tabLog'),
              icon: <ArrowsClockwise weight="duotone" className="h-4 w-4" />,
            },
          ]}
        />
      )}

      {!error && tab === 'certificates' && (
        <Card className="mt-4">
          <CardHeader
            title={t('pages.ssl.certificates')}
            description={t('pages.sslGlobal.certificatesHint')}
            action={
              <div className="flex flex-wrap items-center gap-2">
                <Select
                  aria-label={t('pages.sslGlobal.site')}
                  className="h-9 w-44"
                  options={siteOptions}
                  value={siteFilter}
                  onChange={(e) => setSiteFilter(e.target.value)}
                />
                <Select
                  aria-label={t('common.status')}
                  className="h-9 w-36"
                  options={statusOptions}
                  value={statusFilter}
                  onChange={(e) => setStatusFilter(e.target.value)}
                />
                <Button
                  size="icon"
                  variant="secondary"
                  aria-label={t('common.refresh')}
                  loading={certsQuery.isFetching}
                  onClick={() => certsQuery.refetch()}
                >
                  <ArrowsClockwise weight="duotone" className="h-4 w-4" />
                </Button>
              </div>
            }
          />
          <CardBody className="p-0">
            <Table
              columns={columns}
              data={certs}
              rowKey={(r) => r.id}
              loading={certsQuery.isLoading}
              pageSize={20}
              empty={
                <EmptyState
                  icon={<CertificateIcon weight="duotone" />}
                  title={filtersActive ? t('pages.sslGlobal.noMatch') : t('pages.ssl.empty')}
                  description={
                    filtersActive
                      ? t('pages.sslGlobal.noMatchHint')
                      : t('pages.sslGlobal.emptyDescription')
                  }
                  action={
                    filtersActive ? (
                      <Button
                        variant="secondary"
                        onClick={() => {
                          setSiteFilter('')
                          setStatusFilter('')
                        }}
                      >
                        {t('pages.sslGlobal.clearFilters')}
                      </Button>
                    ) : (
                      canWrite && (
                        <Button variant="primary" onClick={openCreate}>
                          <Plus weight="bold" className="h-4 w-4" />
                          {t('pages.sslGlobal.addCertificate')}
                        </Button>
                      )
                    )
                  }
                />
              }
            />
          </CardBody>
        </Card>
      )}

      {!error && tab === 'log' && (
        <Card className="mt-4">
          <CardHeader
            title={t('pages.sslGlobal.tabLog')}
            description={t('pages.sslGlobal.logHint')}
            action={
              <Select
                aria-label={t('pages.sslGlobal.eventType')}
                className="h-9 w-44"
                options={[
                  { value: '', label: t('pages.sslGlobal.eventFilterAll') },
                  ...CERT_EVENT_TYPES.map((et) => ({
                    value: et,
                    label: t(`pages.sslGlobal.event_${et}`, { defaultValue: et }),
                  })),
                ]}
                value={eventTypeFilter}
                onChange={(e) => setEventTypeFilter(e.target.value)}
              />
            }
          />
          <CardBody className="p-0">
            <Table
              columns={logColumns}
              data={events}
              rowKey={(r) => r.id}
              loading={eventsQuery.isLoading}
              pageSize={20}
              empty={
                <EmptyState
                  icon={<ArrowsClockwise weight="duotone" />}
                  title={t('pages.sslGlobal.logEmpty')}
                  description={t('pages.sslGlobal.logEmptyHint')}
                />
              }
            />
          </CardBody>
        </Card>
      )}

      <Dialog
        open={dialogOpen}
        onClose={() => setDialogOpen(false)}
        title={t('pages.sslGlobal.newCertificateTitle')}
        description={t('pages.ssl.addCertDescription')}
        size="lg"
        footer={
          <div className="flex items-center justify-end gap-2">
            <Button variant="secondary" onClick={() => setDialogOpen(false)}>
              {t('common.cancel')}
            </Button>
            <Button
              variant="primary"
              loading={createCert.isPending}
              // ACME challenges need a site up front — block before the 400.
              disabled={form.mode === 'acme' && !form.siteId}
              onClick={submit}
            >
              {form.mode === 'acme'
                ? t('pages.sslGlobal.applyCertificate')
                : t('pages.sslGlobal.uploadCertificate')}
            </Button>
          </div>
        }
      >
        <div className="flex flex-col gap-4">
          <Tabs
            variant="pill"
            value={form.mode}
            onChange={(v) => {
              setForm((f) => ({ ...f, mode: v as Mode }))
              setFormError(null)
            }}
            items={[
              {
                value: 'acme',
                label: t('pages.ssl.acme'),
                icon: <Lightning weight="duotone" className="h-4 w-4" />,
              },
              {
                value: 'manual',
                label: t('pages.ssl.manualUpload'),
                icon: <FileArrowUp weight="duotone" className="h-4 w-4" />,
              },
            ]}
          />

          <div className="grid gap-4 sm:grid-cols-2">
            <Select
              label={t('pages.sslGlobal.site')}
              hint={t('pages.sslGlobal.siteHint')}
              options={[
                // Unbound upload: the certificate is stored without a site and
                // can be bound to one or more sites afterwards.
                { value: '', label: t('pages.sslGlobal.noSiteOption') },
                ...sites.map((s) => ({ value: s.id, label: `${s.domain} · ${s.name}` })),
              ]}
              value={form.siteId}
              onChange={(e) => {
                const siteId = e.target.value
                // Autofill the domains from the site's own configuration so
                // the operator does not re-type them (and never misses an
                // alternate domain, which would leave it unprotected).
                const domain = sites
                  .filter((s) => s.id === siteId)
                  .flatMap((s) => [s.domain, ...s.alternate_domains])
                  .join(',')
                setForm((f) => ({ ...f, siteId, domain: domain || f.domain }))
              }}
            />
            <Input
              label={t('pages.ssl.domain')}
              placeholder="example.com"
              containerClassName="pw-mono"
              value={form.domain}
              hint={
                form.siteId
                  ? t('pages.ssl.domainAutofillHint')
                  : t('pages.sslGlobal.domainHint')
              }
              onChange={(e) => setForm((f) => ({ ...f, domain: e.target.value }))}
            />
          </div>

          {/* Site selection is optional: without it the certificate is stored
              unbound and can be bound to one or more sites afterwards. */}
          <p className="-mt-2 flex items-start gap-1.5 text-xs leading-relaxed text-fg-subtle">
            <Info weight="duotone" className="mt-0.5 h-3.5 w-3.5 shrink-0" />
            {t('pages.sslGlobal.multiSiteNote')}
          </p>

          {form.mode === 'acme' && !form.siteId && (
            <p
              role="alert"
              className="flex items-start gap-1.5 text-xs font-medium text-fg-warning"
            >
              <Warning weight="duotone" className="mt-0.5 h-3.5 w-3.5 shrink-0" />
              {t('pages.sslGlobal.acmeNeedsSite')}
            </p>
          )}

          {form.mode === 'acme' ? (
            <>
              <div className="grid gap-4 sm:grid-cols-2">
                <Input
                  label={t('pages.ssl.acmeEmail')}
                  hint={t('pages.ssl.acmeEmailHint')}
                  placeholder="admin@example.com"
                  type="email"
                  value={form.acmeEmail}
                  onChange={(e) => setForm((f) => ({ ...f, acmeEmail: e.target.value }))}
                />
                <Select
                  label={t('pages.ssl.challengeType')}
                  options={ACME_CHALLENGE_TYPES.map((c) => ({ value: c, label: c }))}
                  value={form.challengeType}
                  onChange={(e) => setForm((f) => ({ ...f, challengeType: e.target.value }))}
                />
              </div>
              {form.challengeType === 'dns-01' && (
                <>
                  <Select
                    label={t('pages.ssl.dnsProvider')}
                    hint={t('pages.sslGlobal.dnsProviderHint')}
                    options={[
                      { value: '', label: t('pages.ssl.manualDns') },
                      ...ACME_DNS_PROVIDERS.map((p) => ({ value: p, label: p })),
                    ]}
                    value={form.dnsProvider}
                    onChange={(e) => setForm((f) => ({ ...f, dnsProvider: e.target.value }))}
                  />
                  {form.dnsProvider && form.dnsProvider !== 'manual' && (
                    <div className="flex flex-col gap-2">
                      <div className="flex items-center justify-between">
                        <span className="text-[13px] font-medium text-fg">
                          {t('pages.sslGlobal.dnsConfigLabel')}
                        </span>
                        <Button
                          size="sm"
                          variant="ghost"
                          type="button"
                          onClick={() =>
                            setForm((f) => ({
                              ...f,
                              dnsConfig: [...f.dnsConfig, { key: '', value: '' }],
                            }))
                          }
                        >
                          {t('common.add')}
                        </Button>
                      </div>
                      <p className="text-xs text-fg-subtle">
                        {t('pages.sslGlobal.dnsConfigHint')}
                      </p>
                      {form.dnsProvider === 'huawei' && (
                        <p className="text-xs text-fg-subtle">
                          {t('pages.sslGlobal.dnsConfigRegionRequired')}
                        </p>
                      )}
                      {form.dnsConfig.map((row, i) => (
                        <div key={i} className="flex items-start gap-2">
                          <Input
                            placeholder="token"
                            value={row.key}
                            onChange={(e) =>
                              setForm((f) => ({
                                ...f,
                                dnsConfig: f.dnsConfig.map((r, j) =>
                                  j === i ? { ...r, key: e.target.value } : r,
                                ),
                              }))
                            }
                          />
                          <Input
                            placeholder="••••••"
                            value={row.value}
                            onChange={(e) =>
                              setForm((f) => ({
                                ...f,
                                dnsConfig: f.dnsConfig.map((r, j) =>
                                  j === i ? { ...r, value: e.target.value } : r,
                                ),
                              }))
                            }
                          />
                          <Button
                            size="sm"
                            variant="ghost"
                            type="button"
                            onClick={() =>
                              setForm((f) => ({
                                ...f,
                                dnsConfig: f.dnsConfig.filter((_, j) => j !== i),
                              }))
                            }
                          >
                            {t('common.delete')}
                          </Button>
                        </div>
                      ))}
                    </div>
                  )}
                </>
              )}
              <Switch
                checked={form.autoRenew}
                onCheckedChange={(v) => setForm((f) => ({ ...f, autoRenew: v }))}
                label={t('pages.ssl.autoRenew')}
                description={t('pages.ssl.autoRenewHint')}
              />
            </>
          ) : (
            <>
              <Textarea
                label={t('pages.ssl.certPem')}
                hint={t('pages.sslGlobal.certPemHint')}
                mono
                rows={5}
                placeholder="-----BEGIN CERTIFICATE-----"
                value={form.certPem}
                onChange={(e) => setForm((f) => ({ ...f, certPem: e.target.value }))}
              />
              <Textarea
                label={t('pages.ssl.keyPem')}
                hint={t('pages.sslGlobal.keyPemHint')}
                mono
                rows={5}
                placeholder="-----BEGIN PRIVATE KEY-----"
                value={form.keyPem}
                onChange={(e) => setForm((f) => ({ ...f, keyPem: e.target.value }))}
              />
            </>
          )}

          {form.siteId && (
            <div className="rounded-lg border border-line bg-recessed/40 px-3 py-2.5">
              <Switch
                checked={form.activate}
                onCheckedChange={(v) => setForm((f) => ({ ...f, activate: v }))}
                label={t('pages.sslGlobal.activate')}
                description={t('pages.sslGlobal.activateHint')}
              />
            </div>
          )}

          {formError && (
            <p className="text-[13px] font-medium text-fg-danger" role="alert">
              {formError}
            </p>
          )}
        </div>
      </Dialog>

      {/* Binding dialog: three row states — owner, bound, bindable. The
          `usage` query supplies owner/linked/selected; the list row's
          `bound_sites` is the fallback. */}
      <Dialog
        open={bindTarget !== null}
        onClose={() => setBindTarget(null)}
        title={t('pages.sslGlobal.bindDialogTitle')}
        description={
          bindTarget
            ? `${t('pages.sslGlobal.ownerLine', {
                domain:
                  bindUsage?.owner?.domain ?? bindTarget.site_domain ?? '—',
              })} · ${
                (bindUsage?.linked_sites.length ?? bindTarget.bound_count ?? 0) > 0
                  ? t('pages.sslGlobal.sharedCount', {
                      count:
                        bindUsage?.linked_sites.length ??
                        bindTarget.bound_count ??
                        0,
                    })
                  : t('pages.sslGlobal.notShared')
              }`
            : undefined
        }
        size="md"
        footer={
          <div className="flex items-center justify-end gap-2">
            <Button variant="secondary" onClick={() => setBindTarget(null)}>
              {t('common.close')}
            </Button>
            <Button
              variant="primary"
              disabled={bindSelection.length === 0 || bindCert.isPending}
              loading={bindCert.isPending}
              onClick={() => bindCert.mutate(bindSelection)}
            >
              {t('pages.sslGlobal.bindSelectedCount', { count: bindSelection.length })}
            </Button>
          </div>
        }
      >
        <div className="flex flex-col gap-2">
          {sites.length === 0 ? (
            <p className="text-[13px] text-fg-subtle">{t('pages.sslGlobal.bindEmpty')}</p>
          ) : (
            sites.map((s) => {
              // Owner takes priority over bound, bound over bindable; the
              // inlined bound_sites are the fallback when usage is missing
              // and must not upgrade an owner row to "bound" (unbindable).
              const kind = bindingKind(
                bindTarget ?? { site_id: null },
                bindUsage,
                s.id,
                (bindTarget?.bound_sites ?? []).map((b) => b.id),
              )
              const owner = kind === 'owner'
              const linked = kind === 'bound'
              // Owner rows are pinned; bound rows are checked but unbindable;
              // everything else is a fresh checkbox.
              const checked = owner || linked || bindSelection.includes(s.id)
              return (
                <div
                  key={s.id}
                  className="flex flex-col gap-1 rounded-md border border-line bg-elevated px-3 py-2"
                >
                  <div className="flex items-center justify-between gap-2">
                    <label className="flex min-w-0 items-center gap-2">
                      <input
                        type="checkbox"
                        className="h-4 w-4 accent-[var(--color-brand)]"
                        disabled={!canWrite || bindCert.isPending || owner || linked}
                        checked={checked}
                        onChange={(e) =>
                          setBindSelection((prev) =>
                            e.target.checked
                              ? [...prev, s.id]
                              : prev.filter((id) => id !== s.id),
                          )
                        }
                      />
                      <span className="min-w-0 truncate">
                        <span className="pw-mono text-[13px] font-medium text-fg-strong">
                          {s.domain}
                        </span>
                        <span className="ml-2 text-xs text-fg-subtle">{s.name}</span>
                      </span>
                    </label>
                    {owner ? (
                      <Badge tone="brand" size="sm">
                        {t('pages.sslGlobal.ownerBadge')}
                      </Badge>
                    ) : linked ? (
                      <Button
                        size="sm"
                        variant="ghost"
                        className="hover:text-fg-danger"
                        disabled={!canWrite || bindCert.isPending || unbindCert.isPending}
                        onClick={() =>
                          bindTarget &&
                          setPendingUnbind({ certId: bindTarget.id, siteId: s.id })
                        }
                      >
                        <LinkBreak weight="duotone" className="h-4 w-4" />
                        {t('pages.sslGlobal.unbind')}
                      </Button>
                    ) : null}
                  </div>
                  {unbindConflictSite === s.id && (
                    <div className="flex flex-wrap items-center gap-2 rounded-md border border-warning/40 bg-warning/10 px-2.5 py-1.5 text-xs text-fg-warning">
                      <span>{t('pages.sslGlobal.unbindInUse')}</span>
                      <Button
                        size="sm"
                        variant="secondary"
                        onClick={() => navigate(`/sites/${s.id}/ssl`)}
                      >
                        {t('pages.sslGlobal.goToSiteSsl')}
                      </Button>
                    </div>
                  )}
                </div>
              )
            })
          )}
          {bindResults.length > 0 && (
            <div
              role="alert"
              className="flex flex-col gap-1 rounded-md border border-danger/40 bg-danger/8 px-3 py-2"
            >
              <p className="text-[13px] font-medium text-fg-danger">
                {t('pages.sslGlobal.batchFailed', {
                  count: mergeBindResults(bindResults).failed.length,
                })}
              </p>
              {mergeBindResults(bindResults)
                .failed.map((r) => (
                  <p key={r.siteId} className="text-xs text-fg-danger">
                    {sites.find((s) => s.id === r.siteId)?.domain ?? r.siteId} —{' '}
                    {r.reason || t('pages.sslGlobal.batchResult')}
                  </p>
                ))}
            </div>
          )}
          {bindError && (
            <p
              role="alert"
              className="rounded-md border border-danger/40 bg-danger/8 px-3 py-2 text-[13px] text-fg-danger"
            >
              {bindError}
            </p>
          )}
        </div>
      </Dialog>

      {/* Two-step unbind: the site falls back to its own certificate (or none). */}
      <ConfirmDialog
        open={pendingUnbind !== null}
        onClose={() => setPendingUnbind(null)}
        onConfirm={() => pendingUnbind && unbindCert.mutate(pendingUnbind)}
        title={t('pages.sslGlobal.unbindTitle')}
        description={t('pages.sslGlobal.unbindConfirmDescription')}
        confirmLabel={t('pages.sslGlobal.unbind')}
        loading={unbindCert.isPending}
      />

      <ConfirmDialog
        open={pendingDelete !== null}
        onClose={() => setPendingDelete(null)}
        onConfirm={() => {
          if (!pendingDelete) return
          if (deleteHasBlockingSites) {
            // "Go handle it": jump to the first blocking site instead of
            // deleting; the confirm button is never disabled for this branch.
            if (firstBlockingSite) navigate(`/sites/${firstBlockingSite.id}/ssl`)
            setPendingDelete(null)
            return
          }
          if (deleteBlocked) {
            // usage 未返回（unknown）：无法给出跳转目标，只关闭弹窗；
            // usageUnavailable 提示已在弹窗内说明原因。
            setPendingDelete(null)
            return
          }
          deleteCert.mutate(pendingDelete.id)
        }}
        title={t('pages.ssl.deleteTitle')}
        description={t('pages.ssl.deleteDescription')}
        confirmLabel={
          deleteHasBlockingSites
            ? t('pages.sslGlobal.goHandle')
            : deleteBlocked
              ? t('common.close')
              : t('common.delete')
        }
        disabled={deleteCert.isPending}
        loading={deleteCert.isPending}
      >
        {pendingDelete && (
          <>
            <div className="flex items-center gap-2 rounded-md bg-recessed px-3 py-2 text-[13px]">
              <Globe weight="duotone" className="h-4 w-4 shrink-0 text-fg-subtle" />
              <span className="pw-mono text-fg">{pendingDelete.domain}</span>
            </div>
            {/* Usage query failed: we cannot prove the certificate is unused,
                so the delete stays blocked with an explainer. */}
            {deleteUsageQuery.isError && (
              <div
                role="alert"
                className="rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-[13px] text-fg-warning"
              >
                {t('pages.sslGlobal.usageUnavailable')}
              </div>
            )}
            {/* Blocked delete (only when we know the pinning sites): show
                them, each clickable so the operator can go fix it there. */}
            {deleteHasBlockingSites && (
              <div className="flex flex-col gap-1.5 rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-[13px]">
                {(deleteUsage?.serving_sites ?? []).length > 0 && (
                  <div className="flex flex-wrap items-center gap-1.5">
                    <span className="font-medium text-fg-warning">
                      {t('pages.sslGlobal.deleteBlockedServing')}
                    </span>
                    {deleteUsage!.serving_sites.map((site) => (
                      <button
                        key={site.id}
                        type="button"
                        className="pw-mono text-xs text-link underline underline-offset-2"
                        onClick={() => navigate(`/sites/${site.id}/ssl`)}
                      >
                        {site.domain}
                      </button>
                    ))}
                  </div>
                )}
                {(deleteUsage?.linked_sites ?? []).length > 0 && (
                  <div className="flex flex-wrap items-center gap-1.5">
                    <span className="font-medium text-fg-warning">
                      {t('pages.sslGlobal.deleteBlockedLinked')}
                    </span>
                    {deleteUsage!.linked_sites.map((site) => (
                      <button
                        key={site.id}
                        type="button"
                        className="pw-mono text-xs text-link underline underline-offset-2"
                        onClick={() => navigate(`/sites/${site.id}/ssl`)}
                      >
                        {site.domain}
                      </button>
                    ))}
                  </div>
                )}
              </div>
            )}
          </>
        )}
      </ConfirmDialog>
    </div>
  )
}

export default GlobalSslPage
