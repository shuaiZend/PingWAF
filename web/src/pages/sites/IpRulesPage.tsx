import { useEffect, useMemo, useState } from 'react'
import { useParams, useSearchParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  IdentificationCard,
  Plus,
  PencilSimple,
  Trash,
  ArrowClockwise,
  UploadSimple,
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
import { SkeletonRows } from '@/components/ui/Skeleton'
import { useToast } from '@/components/ui/Toast'
import { ErrorState } from '@/components/ErrorState'
import { ipRulesApi, ipRuleKeys, validateIpCidr, parseIpList } from '@/api/ipRules'
import { useCanWrite } from '@/hooks'
import { formatDateTime } from '@/lib/format'
import { IP_RULE_ACTIONS, type CreateIpRuleRequest, type IpRule } from '@/api/types'

const ACTION_TONE: Record<string, 'danger' | 'warning' | 'success' | 'neutral'> = {
  block: 'danger',
  challenge: 'warning',
  allow: 'success',
}

interface FormState {
  ip_cidr: string
  action: string
  note: string
  enabled: boolean
}

const emptyForm = (): FormState => ({
  ip_cidr: '',
  action: 'block',
  note: '',
  enabled: true,
})

function formFromRule(rule: IpRule): FormState {
  return {
    ip_cidr: rule.ip_cidr,
    action: rule.action,
    note: rule.note ?? '',
    enabled: rule.enabled,
  }
}

export function IpRulesPage() {
  const { t } = useTranslation()
  const toast = useToast()
  const queryClient = useQueryClient()
  const canWrite = useCanWrite()
  const { siteId = '' } = useParams<{ siteId: string }>()
  const [searchParams, setSearchParams] = useSearchParams()

  const [dialogOpen, setDialogOpen] = useState(false)
  const [tab, setTab] = useState<'single' | 'bulk'>('single')
  const [editing, setEditing] = useState<IpRule | null>(null)
  const [form, setForm] = useState<FormState>(emptyForm())
  const [bulk, setBulk] = useState('')
  const [bulkAction, setBulkAction] = useState('block')
  const [error, setError] = useState<string | null>(null)
  const [pendingDelete, setPendingDelete] = useState<IpRule | null>(null)

  // Quick-block deep link: /security/ip-rules?block=1.2.3.4
  const prefillBlock = searchParams.get('block')

  const rulesQuery = useQuery({
    queryKey: ipRuleKeys.list(siteId),
    queryFn: () => ipRulesApi.list(siteId),
    enabled: Boolean(siteId),
    select: (page) => page.items,
  })

  const rules = useMemo(
    () =>
      [...(rulesQuery.data ?? [])].sort(
        (a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime(),
      ),
    [rulesQuery.data],
  )

  useEffect(() => {
    if (prefillBlock) {
      setEditing(null)
      setTab('single')
      setForm({ ...emptyForm(), ip_cidr: prefillBlock, action: 'block' })
      setDialogOpen(true)
      searchParams.delete('block')
      setSearchParams(searchParams, { replace: true })
    }
  }, [prefillBlock, searchParams, setSearchParams])

  useEffect(() => {
    if (!dialogOpen) return
    setError(null)
    if (editing) setForm(formFromRule(editing))
    else if (!prefillBlock) setForm(emptyForm())
  }, [dialogOpen, editing, prefillBlock])

  const invalidate = () => {
    void queryClient.invalidateQueries({ queryKey: ipRuleKeys.all(siteId) })
  }

  const save = useMutation({
    mutationFn: (payload: CreateIpRuleRequest) =>
      editing
        ? ipRulesApi.update(siteId, editing.id, payload)
        : ipRulesApi.create(siteId, payload),
    onSuccess: (rule) => {
      toast.success(
        editing ? t('pages.ipRules.ruleUpdated') : t('pages.ipRules.ruleCreated'),
        rule.ip_cidr,
      )
      closeDialog()
      invalidate()
    },
    onError: (e) => setError(e instanceof Error ? e.message : String(e)),
  })

  const bulkSave = useMutation({
    mutationFn: (entries: string[]) =>
      Promise.all(
        entries.map((ip) =>
          ipRulesApi.create(siteId, { ip_cidr: ip, action: bulkAction, enabled: true }),
        ),
      ),
    onSuccess: (created) => {
      toast.success(t('pages.ipRules.bulkCreated', { count: created.length }))
      closeDialog()
      invalidate()
    },
    onError: (e) => setError(e instanceof Error ? e.message : String(e)),
  })

  const toggle = useMutation({
    mutationFn: ({ rule, enabled }: { rule: IpRule; enabled: boolean }) =>
      ipRulesApi.toggleEnabled(siteId, rule.id, enabled),
    onSuccess: () => invalidate(),
  })

  const remove = useMutation({
    mutationFn: (id: string) => ipRulesApi.delete(siteId, id),
    onSuccess: (_d, id) => {
      toast.success(t('pages.ipRules.ruleDeleted'), rules.find((r) => r.id === id)?.ip_cidr)
      setPendingDelete(null)
      invalidate()
    },
  })

  const closeDialog = () => {
    setDialogOpen(false)
    setEditing(null)
    setForm(emptyForm())
    setBulk('')
    setError(null)
  }

  const submitSingle = () => {
    setError(null)
    const reason = validateIpCidr(form.ip_cidr)
    if (reason) {
      setError(t(`pages.ipRules.invalid.${reason}`, t('pages.ipRules.invalidGeneric')))
      return
    }
    save.mutate({
      ip_cidr: form.ip_cidr.trim(),
      action: form.action,
      note: form.note.trim() || null,
      enabled: form.enabled,
    })
  }

  const submitBulk = () => {
    setError(null)
    const { valid, invalid } = parseIpList(bulk)
    if (valid.length === 0) {
      setError(t('pages.ipRules.bulkNone'))
      return
    }
    if (invalid.length > 0) {
      setError(t('pages.ipRules.bulkInvalid', { count: invalid.length, list: invalid.slice(0, 5).join(', ') }))
      return
    }
    bulkSave.mutate(valid)
  }

  const columns: Column<IpRule>[] = [
    {
      key: 'ip_cidr',
      header: t('pages.ipRules.ipCidr'),
      accessor: (r) => r.ip_cidr,
      sortable: true,
      cell: (r) => (
        <span className="pw-mono text-[13px] font-medium text-fg-strong">{r.ip_cidr}</span>
      ),
    },
    {
      key: 'action',
      header: t('pages.waf.action'),
      accessor: (r) => r.action,
      width: '1%',
      cell: (r) => (
        <Badge tone={ACTION_TONE[r.action] ?? 'neutral'}>
          {t(`actions.${r.action}`, r.action)}
        </Badge>
      ),
    },
    {
      key: 'note',
      header: t('pages.ipRules.note'),
      accessor: (r) => r.note ?? '',
      cell: (r) => (
        <span className="line-clamp-1 text-[13px] text-fg-subtle">
          {r.note || <span className="text-fg-subtle/50">—</span>}
        </span>
      ),
    },
    {
      key: 'created_at',
      header: t('pages.ipRules.created'),
      accessor: (r) => r.created_at,
      sortable: true,
      width: '1%',
      cell: (r) => (
        <span className="text-[13px] text-fg-subtle">{formatDateTime(r.created_at)}</span>
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
          aria-label={`${t('common.enabled')}: ${r.ip_cidr}`}
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
              setTab('single')
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

  const blockedCount = rules.filter((r) => r.action === 'block' && r.enabled).length

  return (
    <div className="animate-slide-up">
      <PageHeader
        title={t('pages.ipRules.title')}
        description={t('pages.ipRules.description')}
        actions={
          <div className="flex items-center gap-2">
            <Button
              variant="secondary"
              loading={rulesQuery.isFetching}
              onClick={() => rulesQuery.refetch()}
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
                  setTab('single')
                  setDialogOpen(true)
                }}
              >
                {t('pages.ipRules.addRule')}
              </Button>
            )}
          </div>
        }
      />

      {rules.length > 0 && (
        <div className="mb-4 flex flex-wrap items-center gap-4 rounded-lg border border-line bg-elevated px-4 py-3">
          <span className="flex h-9 w-9 items-center justify-center rounded-lg bg-brand-soft text-brand">
            <IdentificationCard weight="duotone" className="h-4 w-4" />
          </span>
          <div className="min-w-0 flex-1">
            <p className="text-sm font-medium text-fg-strong">
              {t('pages.ipRules.summary', { total: rules.length, blocked: blockedCount })}
            </p>
            <p className="text-xs text-fg-subtle">{t('pages.ipRules.summaryHint')}</p>
          </div>
        </div>
      )}

      {rulesQuery.isError && !rulesQuery.data ? (
        <ErrorState
          error={rulesQuery.error}
          onRetry={() => rulesQuery.refetch()}
          retrying={rulesQuery.isFetching}
        />
      ) : (
        <Card>
          <CardHeader title={t('pages.ipRules.rulesTitle')} />
          <CardBody className="p-0">
            {rulesQuery.isPending ? (
              <SkeletonRows rows={5} columns={6} />
            ) : rules.length === 0 ? (
              <EmptyState
                className="border-0 py-12"
                icon={<IdentificationCard weight="duotone" className="h-8 w-8" />}
                title={t('pages.ipRules.empty')}
                description={t('pages.ipRules.emptyDescription')}
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
                      {t('pages.ipRules.addRule')}
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
                        setTab('single')
                        setDialogOpen(true)
                      }
                    : undefined
                }
              />
            )}
          </CardBody>
        </Card>
      )}

      <Dialog
        open={dialogOpen}
        onClose={save.isPending || bulkSave.isPending ? () => undefined : closeDialog}
        size="lg"
        title={editing ? t('pages.ipRules.editRule') : t('pages.ipRules.addRule')}
        description={t('pages.ipRules.dialogDescription')}
        footer={
          <>
            <Button variant="ghost" onClick={closeDialog} disabled={save.isPending || bulkSave.isPending}>
              {t('common.cancel')}
            </Button>
            {tab === 'single' ? (
              <Button variant="primary" onClick={submitSingle} loading={save.isPending}>
                {editing ? t('common.save') : t('common.create')}
              </Button>
            ) : (
              <Button variant="primary" onClick={submitBulk} loading={bulkSave.isPending}>
                {t('pages.ipRules.importCount', { count: parseIpList(bulk).valid.length })}
              </Button>
            )}
          </>
        }
      >
        {!editing && (
          <Tabs
            variant="pill"
            className="mb-4"
            value={tab}
            onChange={(v) => setTab(v as 'single' | 'bulk')}
            items={[
              { value: 'single', label: t('pages.ipRules.single') },
              { value: 'bulk', label: t('pages.ipRules.bulk'), icon: <UploadSimple weight="duotone" className="h-4 w-4" /> },
            ]}
          />
        )}

        {tab === 'single' || editing ? (
          <div className="flex flex-col gap-4">
            <Input
              label={t('pages.ipRules.ipCidr')}
              value={form.ip_cidr}
              autoFocus
              className="pw-mono"
              placeholder="203.0.113.44 or 10.0.0.0/24"
              hint={t('pages.ipRules.ipHint')}
              onChange={(e) => setForm((f) => ({ ...f, ip_cidr: e.target.value }))}
              required
            />
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <Select
                label={t('pages.waf.action')}
                value={form.action}
                options={IP_RULE_ACTIONS.map((a) => ({
                  value: a,
                  label: t(`actions.${a}`, a),
                }))}
                onChange={(e) => setForm((f) => ({ ...f, action: e.target.value }))}
              />
              <Input
                label={t('pages.ipRules.note')}
                value={form.note}
                placeholder={t('pages.ipRules.notePlaceholder')}
                onChange={(e) => setForm((f) => ({ ...f, note: e.target.value }))}
              />
            </div>
            <Switch
              checked={form.enabled}
              onCheckedChange={(enabled) => setForm((f) => ({ ...f, enabled }))}
              label={t('common.enabled')}
            />
          </div>
        ) : (
          <div className="flex flex-col gap-4">
            <Textarea
              label={t('pages.ipRules.bulkLabel')}
              mono
              rows={8}
              value={bulk}
              placeholder={'203.0.113.44\n10.0.0.0/24\n198.51.100.7'}
              hint={t('pages.ipRules.bulkHint')}
              onChange={(e) => setBulk(e.target.value)}
            />
            <Select
              label={t('pages.waf.action')}
              value={bulkAction}
              options={IP_RULE_ACTIONS.map((a) => ({
                value: a,
                label: t(`actions.${a}`, a),
              }))}
              onChange={(e) => setBulkAction(e.target.value)}
            />
            {bulk.trim() && (
              <p className="text-xs text-fg-subtle">
                {t('pages.ipRules.bulkPreview', {
                  valid: parseIpList(bulk).valid.length,
                  invalid: parseIpList(bulk).invalid.length,
                })}
              </p>
            )}
          </div>
        )}

        {error && (
          <p
            role="alert"
            className="mt-4 rounded-md border border-danger/40 bg-danger/8 px-3 py-2 text-[13px] text-fg-danger"
          >
            {error}
          </p>
        )}
      </Dialog>

      <ConfirmDialog
        open={pendingDelete !== null}
        onClose={() => setPendingDelete(null)}
        onConfirm={() => pendingDelete && remove.mutate(pendingDelete.id)}
        title={t('pages.ipRules.deleteTitle')}
        description={t('pages.ipRules.deleteDescription')}
        confirmLabel={t('common.delete')}
        loading={remove.isPending}
      >
        {pendingDelete && (
          <div className="rounded-md border border-line bg-recessed px-3 py-2">
            <p className="pw-mono text-[13px] font-medium text-fg-strong">
              {pendingDelete.ip_cidr}
            </p>
            {pendingDelete.note && (
              <p className="mt-0.5 text-xs text-fg-subtle">{pendingDelete.note}</p>
            )}
          </div>
        )}
      </ConfirmDialog>
    </div>
  )
}

export default IpRulesPage
