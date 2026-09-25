import { useEffect, useMemo, useState } from 'react'
import { useParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  Lock,
  Plus,
  Trash,
  ArrowClockwise,
  ArrowsClockwise,
  Info,
  Certificate,
  ShieldCheck,
} from '@phosphor-icons/react'
import { PageHeader } from '@/components/PageHeader'
import { Card, CardBody, CardHeader } from '@/components/ui/Card'
import { Button } from '@/components/ui/Button'
import { Input } from '@/components/ui/Input'
import { Select } from '@/components/ui/Select'
import { Switch } from '@/components/ui/Switch'
import { Badge } from '@/components/ui/Badge'
import { Dialog } from '@/components/ui/Dialog'
import { Tabs } from '@/components/ui/Tabs'
import { Textarea } from '@/components/ui/Textarea'
import { Table, type Column } from '@/components/ui/Table'
import { ConfirmDialog } from '@/components/ui/ConfirmDialog'
import { EmptyState } from '@/components/ui/EmptyState'
import { SkeletonRows, SkeletonStat } from '@/components/ui/Skeleton'
import { useToast } from '@/components/ui/Toast'
import { ErrorState } from '@/components/ErrorState'
import { sslApi, sslKeys, daysUntilExpiry, expiryTone } from '@/api/ssl'
import { useCanWrite, useNow } from '@/hooks'
import { formatDateTime } from '@/lib/format'
import {
  ACME_CHALLENGE_TYPES,
  ACME_DNS_PROVIDERS,
  TLS_VERSIONS,
  type CreateSslRequest,
  type SslCertificate,
  type SslSettings,
} from '@/api/types'

type Mode = 'acme' | 'manual'

interface FormState {
  mode: Mode
  domain: string
  auto_renew: boolean
  acme_email: string
  acme_challenge_type: string
  acme_dns_provider: string
  cert_pem: string
  key_pem: string
}

const emptyForm = (): FormState => ({
  mode: 'acme',
  domain: '',
  auto_renew: true,
  acme_email: '',
  acme_challenge_type: 'http-01',
  acme_dns_provider: 'cloudflare',
  cert_pem: '',
  key_pem: '',
})

export function SslPage() {
  const { t } = useTranslation()
  const toast = useToast()
  const queryClient = useQueryClient()
  const canWrite = useCanWrite()
  const now = useNow(60_000)
  const { siteId = '' } = useParams<{ siteId: string }>()

  const [dialogOpen, setDialogOpen] = useState(false)
  const [form, setForm] = useState<FormState>(emptyForm())
  const [error, setError] = useState<string | null>(null)
  const [pendingDelete, setPendingDelete] = useState<SslCertificate | null>(null)
  const [details, setDetails] = useState<SslCertificate | null>(null)
  const [settings, setSettings] = useState<SslSettings | null>(null)

  const certsQuery = useQuery({
    queryKey: sslKeys.list(siteId),
    queryFn: () => sslApi.list(siteId),
    enabled: Boolean(siteId),
  })

  const settingsQuery = useQuery({
    queryKey: sslKeys.settings(siteId),
    queryFn: () => sslApi.getSettings(siteId),
    enabled: Boolean(siteId),
  })

  useEffect(() => {
    if (settingsQuery.data) setSettings(settingsQuery.data)
  }, [settingsQuery.data])

  const certs = useMemo(
    () =>
      [...(certsQuery.data ?? [])].sort((a, b) => {
        const da = daysUntilExpiry(a.expires_at, now) ?? Number.MAX_SAFE_INTEGER
        const db = daysUntilExpiry(b.expires_at, now) ?? Number.MAX_SAFE_INTEGER
        return da - db
      }),
    [certsQuery.data, now],
  )

  const invalidate = () => {
    void queryClient.invalidateQueries({ queryKey: sslKeys.all(siteId) })
  }

  const create = useMutation({
    mutationFn: (payload: CreateSslRequest) => sslApi.create(payload),
    onSuccess: (cert) => {
      toast.success(t('pages.ssl.certCreated'), cert.domain)
      closeDialog()
      invalidate()
    },
    onError: (e) => setError(e instanceof Error ? e.message : String(e)),
  })

  const renew = useMutation({
    mutationFn: (cert: SslCertificate) => sslApi.renew(siteId, cert.id),
    onSuccess: (cert) => {
      toast.success(t('pages.ssl.renewStarted'), cert.domain)
      invalidate()
    },
  })

  const remove = useMutation({
    mutationFn: (id: string) => sslApi.delete(siteId, id),
    onSuccess: (_d, id) => {
      toast.success(t('pages.ssl.certDeleted'), certs.find((c) => c.id === id)?.domain)
      setPendingDelete(null)
      invalidate()
    },
  })

  const saveSettings = useMutation({
    mutationFn: (payload: Partial<SslSettings>) => sslApi.updateSettings(siteId, payload),
    onSuccess: (saved) => {
      setSettings(saved)
      toast.success(t('pages.ssl.settingsSaved'))
      invalidate()
    },
  })

  const closeDialog = () => {
    setDialogOpen(false)
    setForm(emptyForm())
    setError(null)
  }

  const submit = () => {
    setError(null)
    const domain = form.domain.trim()
    if (!domain) {
      setError(t('pages.ssl.domainRequired'))
      return
    }
    const payload: CreateSslRequest = {
      site_id: siteId,
      domain,
      auto_renew: form.auto_renew,
    }
    if (form.mode === 'acme') {
      payload.acme_email = form.acme_email.trim() || null
      payload.acme_challenge_type = form.acme_challenge_type
      payload.acme_dns_provider =
        form.acme_challenge_type === 'dns-01' ? form.acme_dns_provider : null
    } else {
      if (!form.cert_pem.trim() || !form.key_pem.trim()) {
        setError(t('pages.ssl.pemRequired'))
        return
      }
      payload.cert_pem = form.cert_pem
      payload.key_pem = form.key_pem
    }
    create.mutate(payload)
  }

  const columns: Column<SslCertificate>[] = [
    {
      key: 'domain',
      header: t('pages.ssl.domain'),
      accessor: (c) => c.domain,
      sortable: true,
      cell: (c) => (
        <div className="flex items-center gap-2">
          <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-brand-soft text-brand">
            <Lock weight="duotone" className="h-4 w-4" />
          </span>
          <div className="min-w-0">
            <p className="pw-mono truncate text-[13px] font-medium text-fg-strong">{c.domain}</p>
            <p className="truncate text-xs text-fg-subtle">
              {c.issuer || t('pages.ssl.unknownIssuer')}
            </p>
          </div>
        </div>
      ),
    },
    {
      key: 'expires',
      header: t('pages.ssl.expires'),
      accessor: (c) => c.expires_at ?? '',
      width: '1%',
      cell: (c) => {
        const days = daysUntilExpiry(c.expires_at, now)
        const tone = expiryTone(days)
        return (
          <div className="flex flex-col items-start gap-1">
            <span className="text-[13px] text-fg">{formatDateTime(c.expires_at)}</span>
            {days !== null && (
              <Badge tone={tone} size="sm" dot>
                {days < 0
                  ? t('status.expired')
                  : t('pages.ssl.daysLeft', { days })}
              </Badge>
            )}
          </div>
        )
      },
    },
    {
      key: 'auto_renew',
      header: t('pages.ssl.autoRenew'),
      accessor: (c) => (c.auto_renew ? 1 : 0),
      width: '1%',
      cell: (c) =>
        c.auto_renew ? (
          <Badge tone="success" size="sm">
            {t('common.enabled')}
          </Badge>
        ) : (
          <Badge tone="neutral" size="sm">
            {t('common.disabled')}
          </Badge>
        ),
    },
    {
      key: 'row-actions',
      header: '',
      align: 'right',
      width: '1%',
      cell: (c) => (
        <div className="flex items-center justify-end gap-1">
          <Button
            size="icon"
            variant="ghost"
            aria-label={t('common.details')}
            onClick={() => setDetails(c)}
            icon={<Info weight="duotone" className="h-4 w-4" />}
          />
          <Button
            size="icon"
            variant="ghost"
            aria-label={t('pages.ssl.renew')}
            disabled={!canWrite || renew.isPending}
            onClick={() => renew.mutate(c)}
            icon={<ArrowsClockwise weight="duotone" className="h-4 w-4" />}
          />
          <Button
            size="icon"
            variant="ghost"
            className="hover:text-fg-danger"
            aria-label={t('common.delete')}
            disabled={!canWrite}
            onClick={() => setPendingDelete(c)}
            icon={<Trash weight="duotone" className="h-4 w-4" />}
          />
        </div>
      ),
    },
  ]

  const expiringSoon = certs.filter((c) => {
    const d = daysUntilExpiry(c.expires_at, now)
    return d !== null && d >= 0 && d < 30
  }).length

  return (
    <div className="animate-slide-up">
      <PageHeader
        title={t('pages.ssl.title')}
        description={t('pages.ssl.description')}
        actions={
          <div className="flex items-center gap-2">
            <Button
              variant="secondary"
              loading={certsQuery.isFetching}
              onClick={() => certsQuery.refetch()}
              icon={<ArrowClockwise weight="duotone" className="h-4 w-4" />}
            >
              {t('common.refresh')}
            </Button>
            {canWrite && (
              <Button
                variant="primary"
                icon={<Plus weight="bold" className="h-4 w-4" />}
                onClick={() => {
                  setForm(emptyForm())
                  setDialogOpen(true)
                }}
              >
                {t('pages.ssl.addCert')}
              </Button>
            )}
          </div>
        }
      />

      {expiringSoon > 0 && (
        <div className="mb-4 flex items-center gap-3 rounded-lg border border-warning/40 bg-warning/10 px-4 py-3">
          <ShieldCheck weight="duotone" className="h-5 w-5 shrink-0 text-warning" />
          <p className="text-sm text-fg">
            {t('pages.ssl.expiringWarning', { count: expiringSoon })}
          </p>
        </div>
      )}

      {certsQuery.isError && !certsQuery.data ? (
        <ErrorState
          error={certsQuery.error}
          onRetry={() => certsQuery.refetch()}
          retrying={certsQuery.isFetching}
        />
      ) : (
        <Card className="mb-6">
          <CardHeader
            title={t('pages.ssl.certificates')}
            description={t('pages.ssl.certificatesHint')}
          />
          <CardBody className="p-0">
            {certsQuery.isPending ? (
              <SkeletonRows rows={3} columns={4} />
            ) : certs.length === 0 ? (
              <EmptyState
                className="border-0 py-12"
                icon={<Certificate weight="duotone" className="h-8 w-8" />}
                title={t('pages.ssl.empty')}
                description={t('pages.ssl.emptyDescription')}
                action={
                  canWrite ? (
                    <Button
                      variant="primary"
                      icon={<Plus weight="bold" className="h-4 w-4" />}
                      onClick={() => setDialogOpen(true)}
                    >
                      {t('pages.ssl.addCert')}
                    </Button>
                  ) : undefined
                }
              />
            ) : (
              <Table
                columns={columns}
                data={certs}
                rowKey={(c) => c.id}
                dense
                onRowClick={(c) => setDetails(c)}
              />
            )}
          </CardBody>
        </Card>
      )}

      {/* SSL settings */}
      <Card>
        <CardHeader
          title={t('pages.ssl.settingsTitle')}
          description={t('pages.ssl.settingsHint')}
          action={
            canWrite && (
              <Button
                variant="primary"
                size="sm"
                loading={saveSettings.isPending}
                disabled={!settings}
                onClick={() =>
                  settings &&
                  saveSettings.mutate({
                    min_tls_version: settings.min_tls_version,
                    hsts_enabled: settings.hsts_enabled,
                    hsts_max_age: settings.hsts_max_age,
                    always_use_https: settings.always_use_https,
                  })
                }
              >
                {t('common.save')}
              </Button>
            )
          }
        />
        <CardBody>
          {settingsQuery.isPending && !settings ? (
            <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
              <SkeletonStat />
              <SkeletonStat />
            </div>
          ) : settings ? (
            <div className="flex flex-col gap-5">
              <Select
                label={t('pages.ssl.minVersion')}
                value={String(settings.min_tls_version)}
                disabled={!canWrite}
                containerClassName="max-w-xs"
                options={TLS_VERSIONS.map((v) => ({ value: v, label: `TLS ${v}` }))}
                onChange={(e) =>
                  setSettings((s) => (s ? { ...s, min_tls_version: e.target.value } : s))
                }
              />
              <Switch
                checked={settings.hsts_enabled}
                disabled={!canWrite}
                onCheckedChange={(hsts_enabled) =>
                  setSettings((s) => (s ? { ...s, hsts_enabled } : s))
                }
                label={t('pages.ssl.hsts')}
                description={t('pages.ssl.hstsHint')}
              />
              {settings.hsts_enabled && (
                <Input
                  type="number"
                  label={t('pages.ssl.hstsMaxAge')}
                  value={settings.hsts_max_age}
                  min={0}
                  disabled={!canWrite}
                  containerClassName="max-w-xs"
                  hint={t('pages.ssl.hstsMaxAgeHint')}
                  onChange={(e) =>
                    setSettings((s) =>
                      s ? { ...s, hsts_max_age: Number(e.target.value) } : s,
                    )
                  }
                />
              )}
              <Switch
                checked={settings.always_use_https}
                disabled={!canWrite}
                onCheckedChange={(always_use_https) =>
                  setSettings((s) => (s ? { ...s, always_use_https } : s))
                }
                label={t('pages.ssl.alwaysHttps')}
                description={t('pages.ssl.alwaysHttpsHint')}
              />
            </div>
          ) : (
            <ErrorState
              variant="inline"
              error={settingsQuery.error}
              onRetry={() => settingsQuery.refetch()}
              retrying={settingsQuery.isFetching}
            />
          )}
        </CardBody>
      </Card>

      {/* Add certificate */}
      <Dialog
        open={dialogOpen}
        onClose={create.isPending ? () => undefined : closeDialog}
        size="lg"
        title={t('pages.ssl.addCert')}
        description={t('pages.ssl.addCertDescription')}
        footer={
          <>
            <Button variant="ghost" onClick={closeDialog} disabled={create.isPending}>
              {t('common.cancel')}
            </Button>
            <Button variant="primary" onClick={submit} loading={create.isPending}>
              {t('common.create')}
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-4">
          <Input
            label={t('pages.ssl.domain')}
            value={form.domain}
            autoFocus
            placeholder="example.com"
            onChange={(e) => setForm((f) => ({ ...f, domain: e.target.value }))}
            required
          />

          <Tabs
            variant="pill"
            value={form.mode}
            onChange={(mode) => setForm((f) => ({ ...f, mode: mode as Mode }))}
            items={[
              { value: 'acme', label: t('pages.ssl.acme') },
              { value: 'manual', label: t('pages.ssl.manualUpload') },
            ]}
          />

          {form.mode === 'acme' ? (
            <div className="flex flex-col gap-4">
              <Input
                type="email"
                label={t('pages.ssl.acmeEmail')}
                value={form.acme_email}
                placeholder="admin@example.com"
                hint={t('pages.ssl.acmeEmailHint')}
                onChange={(e) => setForm((f) => ({ ...f, acme_email: e.target.value }))}
              />
              <Select
                label={t('pages.ssl.challengeType')}
                value={form.acme_challenge_type}
                options={ACME_CHALLENGE_TYPES.map((c) => ({
                  value: c,
                  label: c.toUpperCase(),
                }))}
                onChange={(e) =>
                  setForm((f) => ({ ...f, acme_challenge_type: e.target.value }))
                }
              />
              {form.acme_challenge_type === 'dns-01' && (
                <Select
                  label={t('pages.ssl.dnsProvider')}
                  value={form.acme_dns_provider}
                  options={ACME_DNS_PROVIDERS.map((p) => ({
                    value: p,
                    label: p === 'manual' ? t('pages.ssl.manualDns') : p,
                  }))}
                  onChange={(e) =>
                    setForm((f) => ({ ...f, acme_dns_provider: e.target.value }))
                  }
                />
              )}
              <Switch
                checked={form.auto_renew}
                onCheckedChange={(auto_renew) => setForm((f) => ({ ...f, auto_renew }))}
                label={t('pages.ssl.autoRenew')}
                description={t('pages.ssl.autoRenewHint')}
              />
            </div>
          ) : (
            <div className="flex flex-col gap-4">
              <Textarea
                label={t('pages.ssl.certPem')}
                mono
                rows={6}
                value={form.cert_pem}
                placeholder={'-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----'}
                onChange={(e) => setForm((f) => ({ ...f, cert_pem: e.target.value }))}
              />
              <Textarea
                label={t('pages.ssl.keyPem')}
                mono
                rows={6}
                value={form.key_pem}
                placeholder={'-----BEGIN PRIVATE KEY-----\n...\n-----END PRIVATE KEY-----'}
                onChange={(e) => setForm((f) => ({ ...f, key_pem: e.target.value }))}
              />
            </div>
          )}

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

      {/* Certificate details */}
      <Dialog
        open={details !== null}
        onClose={() => setDetails(null)}
        size="md"
        title={t('pages.ssl.detailsTitle')}
        footer={
          <Button variant="secondary" onClick={() => setDetails(null)}>
            {t('common.close')}
          </Button>
        }
      >
        {details && (
          <dl className="grid grid-cols-1 gap-x-6 gap-y-3 text-sm sm:grid-cols-2">
            <Detail label={t('pages.ssl.domain')} value={details.domain} mono />
            <Detail label={t('pages.ssl.issuer')} value={details.issuer || '—'} />
            <Detail
              label={t('pages.ssl.expires')}
              value={formatDateTime(details.expires_at)}
            />
            <Detail
              label={t('pages.ssl.autoRenew')}
              value={details.auto_renew ? t('common.yes') : t('common.no')}
            />
            <Detail
              label={t('pages.ssl.acmeEmail')}
              value={details.acme_email || '—'}
            />
            <Detail
              label={t('pages.ssl.challengeType')}
              value={(details.acme_challenge_type || '—').toUpperCase()}
            />
            <Detail
              label={t('pages.ssl.dnsProvider')}
              value={details.acme_dns_provider || '—'}
            />
            <Detail
              label={t('pages.ssl.created')}
              value={formatDateTime(details.created_at)}
            />
          </dl>
        )}
      </Dialog>

      <ConfirmDialog
        open={pendingDelete !== null}
        onClose={() => setPendingDelete(null)}
        onConfirm={() => pendingDelete && remove.mutate(pendingDelete.id)}
        title={t('pages.ssl.deleteTitle')}
        description={t('pages.ssl.deleteDescription')}
        confirmLabel={t('common.delete')}
        loading={remove.isPending}
      >
        {pendingDelete && (
          <div className="rounded-md border border-line bg-recessed px-3 py-2">
            <p className="pw-mono text-[13px] font-medium text-fg-strong">
              {pendingDelete.domain}
            </p>
          </div>
        )}
      </ConfirmDialog>
    </div>
  )
}

function Detail({
  label,
  value,
  mono,
}: {
  label: string
  value: string
  mono?: boolean
}) {
  return (
    <div className="min-w-0">
      <dt className="text-xs text-fg-subtle">{label}</dt>
      <dd
        className={
          mono
            ? 'pw-mono mt-0.5 truncate text-[13px] font-medium text-fg-strong'
            : 'mt-0.5 truncate text-[13px] font-medium text-fg-strong'
        }
      >
        {value}
      </dd>
    </div>
  )
}

export default SslPage
