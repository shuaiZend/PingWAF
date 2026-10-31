import { useMemo, useState } from 'react'
import { useParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  Eye,
  Path as PathIcon,
  PencilSimple,
  Plus,
  Trash,
  ArrowClockwise,
} from '@phosphor-icons/react'
import { Card, CardBody, CardHeader } from '@/components/ui/Card'
import { Badge } from '@/components/ui/Badge'
import { Button } from '@/components/ui/Button'
import { Input } from '@/components/ui/Input'
import { Switch } from '@/components/ui/Switch'
import { Dialog } from '@/components/ui/Dialog'
import { Table, type Column } from '@/components/ui/Table'
import { ConfirmDialog } from '@/components/ui/ConfirmDialog'
import { EmptyState } from '@/components/ui/EmptyState'
import { SkeletonRows } from '@/components/ui/Skeleton'
import { PillMultiSelect } from '@/components/ui/MultiSelect'
import { useToast } from '@/components/ui/Toast'
import { errorMessage } from '@/api/errors'
import { pathExceptionsApi, pathExceptionKeys } from '@/api/pathExceptions'
import { wafSettingsApi, wafSettingsKeys } from '@/api/wafSettings'
import { useCanWrite } from '@/hooks'
import { cn } from '@/lib/utils'
import { formatDateTime } from '@/lib/format'
import { bypassesAllLayers, toggleSkipLayer, validatePathExceptionPath } from '@/lib/pathException'
import {
  PATH_ADVANCED_LAYERS,
  PATH_PRIMARY_LAYERS,
  type CreatePathExceptionRequest,
  type PathException,
  type PathMatchType,
} from '@/api/types'

const MATCH_TYPES: PathMatchType[] = ['prefix', 'exact', 'wildcard']

const MATCH_TONE: Record<string, 'info' | 'brand' | 'warning'> = {
  prefix: 'info',
  exact: 'brand',
  wildcard: 'warning',
}

interface FormState {
  path: string
  match_type: PathMatchType
  skip_layers: string[]
  note: string
  log_hits: boolean
  enabled: boolean
}

const emptyForm = (): FormState => ({
  path: '',
  match_type: 'prefix',
  skip_layers: ['waf', 'bot'],
  note: '',
  log_hits: false,
  enabled: true,
})

function formFromException(e: PathException): FormState {
  return {
    path: e.path,
    match_type: (e.match_type as PathMatchType) ?? 'prefix',
    skip_layers: [...(e.skip_layers ?? [])],
    note: e.note ?? '',
    log_hits: e.log_hits,
    enabled: e.enabled,
  }
}

/**
 * Per-site path exceptions: request paths that bypass named protection
 * layers. Writes are only accepted while the site's WAF mode is `monitor`,
 * so the panel fronts an observation-mode gate.
 */
export function PathExceptionsPanel() {
  const { t } = useTranslation()
  const toast = useToast()
  const queryClient = useQueryClient()
  const canWrite = useCanWrite()
  const { siteId = '' } = useParams<{ siteId: string }>()

  const [dialogOpen, setDialogOpen] = useState(false)
  const [editing, setEditing] = useState<PathException | null>(null)
  const [form, setForm] = useState<FormState>(emptyForm())
  const [formError, setFormError] = useState<string | null>(null)
  const [pendingDelete, setPendingDelete] = useState<PathException | null>(null)

  const exceptionsQuery = useQuery({
    queryKey: pathExceptionKeys.list(siteId),
    queryFn: () => pathExceptionsApi.list(siteId),
    enabled: Boolean(siteId),
  })

  const settingsQuery = useQuery({
    queryKey: wafSettingsKeys.settings(siteId),
    queryFn: () => wafSettingsApi.get(siteId),
    enabled: Boolean(siteId),
  })
  const wafMode = settingsQuery.data?.mode ?? 'off'
  const observations = wafMode !== 'monitor'
  // Site is in observation mode (monitor) — required for writes.

  // Re-seed the editor only when it opens or the target changes (render-phase
  // reset), so a background refetch never wipes in-progress edits.
  const seedKey = dialogOpen ? (editing?.id ?? 'new') : ''
  const [lastSeedKey, setLastSeedKey] = useState<string | null>(null)
  if (seedKey !== lastSeedKey) {
    setLastSeedKey(seedKey)
    if (dialogOpen) {
      setFormError(null)
      setForm(editing ? formFromException(editing) : emptyForm())
    }
  }

  const invalidate = () => {
    void queryClient.invalidateQueries({ queryKey: pathExceptionKeys.all(siteId) })
  }

  const switchToMonitor = useMutation({
    mutationFn: () => wafSettingsApi.update(siteId, { mode: 'monitor' }),
    onSuccess: () => {
      toast.success(t('pages.pathExceptions.monitorApplied'))
      void queryClient.invalidateQueries({ queryKey: wafSettingsKeys.settings(siteId) })
      void queryClient.invalidateQueries({ queryKey: wafSettingsKeys.posture(siteId) })
    },
    onError: (e) => toast.error(t('pages.protection.updateFailed'), errorMessage(e)),
  })

  const save = useMutation({
    mutationFn: (payload: CreatePathExceptionRequest) =>
      editing
        ? pathExceptionsApi.update(siteId, editing.id, payload)
        : pathExceptionsApi.create(siteId, payload),
    onSuccess: (saved) => {
      toast.success(
        editing ? t('pages.pathExceptions.updated') : t('pages.pathExceptions.created'),
        saved.path,
      )
      setDialogOpen(false)
      invalidate()
    },
    onError: (e) => {
      // The server answers 409 (observation mode) / 400 (validation) with a
      // human-readable message — surface it instead of a generic failure.
      setFormError(errorMessage(e))
    },
  })

  const remove = useMutation({
    mutationFn: (id: string) => pathExceptionsApi.remove(siteId, id),
    onSuccess: (_data, id) => {
      toast.success(
        t('pages.pathExceptions.deleted'),
        (exceptionsQuery.data ?? []).find((p) => p.id === id)?.path,
      )
      setPendingDelete(null)
      invalidate()
    },
    onError: (e) => toast.error(t('pages.pathExceptions.deleteFailed'), errorMessage(e)),
  })

  const submit = () => {
    const validation = validatePathExceptionPath(form.path, form.match_type)
    if (validation) {
      setFormError(t(validation))
      return
    }
    if (form.skip_layers.length === 0) {
      setFormError(t('pages.pathExceptions.errorLayersRequired'))
      return
    }
    setFormError(null)
    save.mutate({
      path: form.path.trim(),
      match_type: form.match_type,
      skip_layers: form.skip_layers,
      note: form.note.trim() || null,
      log_hits: form.log_hits,
      enabled: form.enabled,
    })
  }

  const layerBadge = (layer: string) =>
    layer === '*' ? (
      <Badge key={layer} tone="warning" size="sm">
        {t('pages.pathExceptions.layerAll')}
      </Badge>
    ) : (
      <Badge key={layer} tone="neutral" size="sm">
        {t(`pages.pathExceptions.layer_${layer}`, layer)}
      </Badge>
    )

  const columns: Column<PathException>[] = [
    {
      key: 'path',
      header: t('pages.pathExceptions.path'),
      accessor: (p) => p.path,
      sortable: true,
      cell: (p) => (
        <div className="min-w-0">
          <p className="pw-mono truncate text-[13px] font-medium text-fg-strong">{p.path}</p>
          {p.note && <p className="truncate text-xs text-fg-subtle">{p.note}</p>}
        </div>
      ),
    },
    {
      key: 'match_type',
      header: t('pages.pathExceptions.matchType'),
      accessor: (p) => p.match_type,
      width: '1%',
      cell: (p) => (
        <Badge tone={MATCH_TONE[p.match_type] ?? 'neutral'} size="sm">
          {t(`pages.pathExceptions.match_${p.match_type}`, p.match_type)}
        </Badge>
      ),
    },
    {
      key: 'skip_layers',
      header: t('pages.pathExceptions.skipLayers'),
      width: '1%',
      cell: (p) => (
        <div className="flex max-w-[280px] flex-wrap gap-1">
          {(p.skip_layers ?? []).map(layerBadge)}
        </div>
      ),
    },
    {
      key: 'log_hits',
      header: t('pages.pathExceptions.logHits'),
      width: '1%',
      cell: (p) => (
        <span className="inline-flex items-center gap-1.5 text-[13px] text-fg-subtle">
          <Eye weight="duotone" className={cn('h-4 w-4', p.log_hits ? 'text-fg' : 'opacity-40')} />
          {p.log_hits ? t('common.yes') : t('common.no')}
        </span>
      ),
    },
    {
      key: 'enabled',
      header: t('common.enabled'),
      accessor: (p) => (p.enabled ? 1 : 0),
      width: '1%',
      cell: (p) =>
        p.enabled ? (
          <Badge tone="success" size="sm" dot>
            {t('common.enabled')}
          </Badge>
        ) : (
          <Badge tone="neutral" size="sm" dot>
            {t('common.disabled')}
          </Badge>
        ),
    },
    {
      key: 'updated',
      header: t('common.updated'),
      accessor: (p) => p.updated_at,
      width: '1%',
      cell: (p) => (
        <span className="whitespace-nowrap text-xs text-fg-subtle">
          {formatDateTime(p.updated_at)}
        </span>
      ),
    },
    {
      key: 'row-actions',
      header: '',
      align: 'right',
      width: '1%',
      cell: (p) => (
        <div className="flex items-center justify-end gap-1">
          <Button
            size="icon"
            variant="ghost"
            aria-label={t('common.edit')}
            disabled={!canWrite || observations}
            onClick={() => {
              setEditing(p)
              setDialogOpen(true)
            }}
            icon={<PencilSimple weight="duotone" className="h-4 w-4" />}
          />
          <Button
            size="icon"
            variant="ghost"
            className="hover:text-fg-danger"
            aria-label={t('common.delete')}
            disabled={!canWrite || observations}
            onClick={() => setPendingDelete(p)}
            icon={<Trash weight="duotone" className="h-4 w-4" />}
          />
        </div>
      ),
    },
  ]

  const exceptions = useMemo(() => exceptionsQuery.data ?? [], [exceptionsQuery.data])
  const editingDisabled = !canWrite || observations

  return (
    <Card>
      <CardHeader
        title={t('pages.pathExceptions.title')}
        description={t('pages.pathExceptions.description')}
        action={
          <div className="flex items-center gap-2">
            <Button
              variant="secondary"
              size="sm"
              loading={exceptionsQuery.isFetching}
              onClick={() => exceptionsQuery.refetch()}
              icon={<ArrowClockwise weight="duotone" className="h-3.5 w-3.5" />}
            >
              {t('common.refresh')}
            </Button>
            {canWrite && (
              <Button
                variant="primary"
                size="sm"
                disabled={observations}
                icon={<Plus weight="bold" className="h-4 w-4" />}
                onClick={() => {
                  setEditing(null)
                  setDialogOpen(true)
                }}
              >
                {t('pages.pathExceptions.add')}
              </Button>
            )}
          </div>
        }
      />

      {/* Observation-mode gate: writes require the site to run in monitor. */}
      {observations && (
        <div className="mx-5 mt-4 flex flex-wrap items-center justify-between gap-3 rounded-md border border-warning/40 bg-warning/10 px-3 py-2.5">
          <p className="text-[13px] text-fg">{t('pages.pathExceptions.observationGate')}</p>
          {canWrite && (
            <Button
              size="sm"
              variant="secondary"
              loading={switchToMonitor.isPending}
              onClick={() => switchToMonitor.mutate()}
            >
              {t('pages.pathExceptions.switchToMonitor')}
            </Button>
          )}
        </div>
      )}

      <CardBody className="p-0">
        {exceptionsQuery.isPending ? (
          <SkeletonRows rows={3} columns={6} />
        ) : exceptions.length === 0 ? (
          <EmptyState
            className="border-0 py-12"
            icon={<PathIcon weight="duotone" className="h-8 w-8" />}
            title={t('pages.pathExceptions.empty')}
            description={t('pages.pathExceptions.emptyDescription')}
            action={
              canWrite ? (
                <Button
                  variant="primary"
                  icon={<Plus weight="bold" className="h-4 w-4" />}
                  disabled={observations}
                  onClick={() => {
                    setEditing(null)
                    setDialogOpen(true)
                  }}
                >
                  {t('pages.pathExceptions.add')}
                </Button>
              ) : undefined
            }
          />
        ) : (
          <Table columns={columns} data={exceptions} rowKey={(p) => p.id} dense />
        )}
      </CardBody>

      {/* Create / edit */}
      <Dialog
        open={dialogOpen}
        onClose={save.isPending ? () => undefined : () => setDialogOpen(false)}
        size="md"
        title={
          editing ? t('pages.pathExceptions.edit') : t('pages.pathExceptions.add')
        }
        description={t('pages.pathExceptions.dialogDescription')}
        footer={
          <>
            <Button
              variant="ghost"
              onClick={() => setDialogOpen(false)}
              disabled={save.isPending}
            >
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
            label={t('pages.pathExceptions.path')}
            value={form.path}
            autoFocus
            placeholder="/api/*"
            hint={t('pages.pathExceptions.pathHint')}
            className="pw-mono"
            onChange={(e) => setForm((f) => ({ ...f, path: e.target.value }))}
            required
          />

          <div>
            <p className="mb-1.5 text-[13px] font-medium text-fg-subtle">
              {t('pages.pathExceptions.matchType')}
            </p>
            <div className="flex items-center gap-1.5">
              {MATCH_TYPES.map((mt) => (
                <button
                  key={mt}
                  type="button"
                  aria-pressed={form.match_type === mt}
                  onClick={() => setForm((f) => ({ ...f, match_type: mt }))}
                  className={cn(
                    'rounded-md border px-3 py-1.5 text-[13px] font-medium transition-colors',
                    form.match_type === mt
                      ? 'border-brand bg-brand-soft text-brand'
                      : 'border-line text-fg-subtle hover:border-fill hover:text-fg',
                  )}
                >
                  {t(`pages.pathExceptions.match_${mt}`, mt)}
                </button>
              ))}
            </div>
            {form.match_type === 'wildcard' && (
              <p className="mt-1.5 text-xs text-fg-subtle">
                {t('pages.pathExceptions.wildcardHint')}
              </p>
            )}
          </div>

          <div className="flex flex-col gap-2 rounded-md border border-line bg-recessed/60 p-3">
            <p className="text-[13px] font-medium text-fg">
              {t('pages.pathExceptions.bypassLayers')}
            </p>
            {bypassesAllLayers(form.skip_layers) && (
              <p className="text-xs font-medium text-fg-warning">
                {t('pages.pathExceptions.bypassAllNote')}
              </p>
            )}
            {PATH_PRIMARY_LAYERS.map((layer) => {
              const on =
                form.skip_layers.includes(layer) || bypassesAllLayers(form.skip_layers)
              return (
                <Switch
                  key={layer}
                  size="sm"
                  checked={on}
                  disabled={editingDisabled}
                  label={t(`pages.pathExceptions.layer_${layer}`, layer)}
                  description={t(`pages.pathExceptions.layerHint_${layer}`)}
                  onCheckedChange={(next) =>
                    setForm((f) => ({ ...f, skip_layers: toggleSkipLayer(f.skip_layers, layer, next) }))
                  }
                />
              )
            })}
            <PillMultiSelect
              className="mt-1"
              label={t('pages.pathExceptions.advancedLayers')}
              hint={t('pages.pathExceptions.advancedLayersHint')}
              value={form.skip_layers}
              onChange={(skip_layers) => setForm((f) => ({ ...f, skip_layers }))}
              options={[
                {
                  value: '*',
                  label: t('pages.pathExceptions.layerAll'),
                  hint: t('pages.pathExceptions.layerAllHint'),
                },
                ...PATH_ADVANCED_LAYERS.map((layer) => ({
                  value: layer,
                  label: t(`pages.pathExceptions.layer_${layer}`, layer),
                })),
                ...PATH_PRIMARY_LAYERS.map((layer) => ({
                  value: layer,
                  label: t(`pages.pathExceptions.layer_${layer}`, layer),
                })),
              ]}
            />
          </div>

          <Switch
            checked={form.log_hits}
            onCheckedChange={(log_hits) => setForm((f) => ({ ...f, log_hits }))}
            label={t('pages.pathExceptions.logHits')}
            description={t('pages.pathExceptions.logHitsHint')}
          />
          <Switch
            checked={form.enabled}
            onCheckedChange={(enabled) => setForm((f) => ({ ...f, enabled }))}
            label={t('common.enabled')}
          />
          <Input
            label={t('pages.pathExceptions.note')}
            value={form.note}
            placeholder={t('pages.pathExceptions.notePlaceholder')}
            onChange={(e) => setForm((f) => ({ ...f, note: e.target.value }))}
          />

          {formError && (
            <p
              role="alert"
              className="rounded-md border border-danger/40 bg-danger/8 px-3 py-2 text-[13px] text-fg-danger"
            >
              {formError}
            </p>
          )}
        </div>
      </Dialog>

      <ConfirmDialog
        open={pendingDelete !== null}
        onClose={() => setPendingDelete(null)}
        onConfirm={() => pendingDelete && remove.mutate(pendingDelete.id)}
        title={t('pages.pathExceptions.deleteTitle')}
        description={t('pages.pathExceptions.deleteDescription')}
        confirmLabel={t('common.delete')}
        loading={remove.isPending}
      >
        {pendingDelete && (
          <div className="rounded-md border border-line bg-recessed px-3 py-2">
            <p className="pw-mono text-[13px] font-medium text-fg-strong">
              {pendingDelete.path}
            </p>
          </div>
        )}
      </ConfirmDialog>
    </Card>
  )
}

export default PathExceptionsPanel
