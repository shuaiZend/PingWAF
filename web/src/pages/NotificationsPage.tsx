import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  Plus,
  Trash,
  Pencil,
  PaperPlaneTilt,
  ArrowClockwise,
  Bell,
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
import { SkeletonRows } from '@/components/ui/Skeleton'
import { useToast } from '@/components/ui/Toast'
import { ErrorState } from '@/components/ErrorState'
import {
  notificationsApi,
  notificationKeys,
  unmaskSecrets,
  NOTIFICATION_EVENT_TYPES,
  type NotificationChannel,
  type NotificationSettings,
} from '@/api/notifications'
import { useCanWrite } from '@/hooks'
import { formatDateTime, formatRelative } from '@/lib/format'

const KIND_TONE: Record<string, 'info' | 'success' | 'warning' | 'neutral'> = {
  email: 'info',
  wecom: 'success',
  dingtalk: 'info',
  webhook: 'neutral',
}

const SEVERITY_TONE: Record<string, 'info' | 'warning' | 'danger'> = {
  info: 'info',
  warning: 'warning',
  critical: 'danger',
}

export function NotificationsPage() {
  const { t } = useTranslation()
  const toast = useToast()
  const queryClient = useQueryClient()
  const canWrite = useCanWrite()

  const [editing, setEditing] = useState<NotificationChannel | null>(null)
  const [createOpen, setCreateOpen] = useState(false)
  const [pendingDelete, setPendingDelete] = useState<NotificationChannel | null>(null)
  const [eventTypeFilter, setEventTypeFilter] = useState('')

  const channelsQuery = useQuery({
    queryKey: notificationKeys.channels(),
    queryFn: () => notificationsApi.listChannels(),
  })

  const settingsQuery = useQuery({
    queryKey: notificationKeys.settings(),
    queryFn: () => notificationsApi.getSettings(),
  })

  const [eventsOffset, setEventsOffset] = useState(0)
  const eventsQuery = useQuery({
    queryKey: notificationKeys.events(eventsOffset),
    queryFn: () => notificationsApi.listEvents(50, eventsOffset, eventTypeFilter || undefined),
  })

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: notificationKeys.all })
  }

  const deleteMutation = useMutation({
    mutationFn: (id: string) => notificationsApi.deleteChannel(id),
    onSuccess: () => {
      toast.success(t('pages.notifications.channelDeleted'))
      invalidate()
    },
    onError: () => toast.error(t('pages.notifications.channelDeleteFailed')),
  })

  const testMutation = useMutation({
    mutationFn: (id: string) => notificationsApi.testChannel(id),
    onSuccess: (result) => {
      if (result.ok) {
        toast.success(t('pages.notifications.testSent'))
      } else {
        toast.error(`${t('pages.notifications.testFailed')}: ${result.error ?? ''}`)
      }
    },
    onError: (error) => toast.error(error.message || t('pages.notifications.testFailed')),
  })

  const channels = useMemo(() => channelsQuery.data ?? [], [channelsQuery.data])
  const events = useMemo(() => eventsQuery.data ?? [], [eventsQuery.data])

  const columns: Column<NotificationChannel>[] = useMemo(
    () => [
      {
        key: 'name',
        header: t('pages.notifications.colName'),
        accessor: (row) => row.name,
      },
      {
        key: 'kind',
        header: t('pages.notifications.colKind'),
        accessor: (row) => row.kind,
        cell: (row) => (
          <Badge tone={KIND_TONE[row.kind] ?? 'neutral'}>
            {t(`pages.notifications.kind_${row.kind}`)}
          </Badge>
        ),
      },
      {
        key: 'events',
        header: t('pages.notifications.colEvents'),
        accessor: (row) => row.events.length,
        cell: (row) =>
          row.events.length === 0 ? (
            <span className="text-fg-subtle">{t('pages.notifications.allEvents')}</span>
          ) : (
            <span className="text-xs">{row.events.map((e) => t(`pages.notifications.event_${e.replace('.', '_')}`)).join(', ')}</span>
          ),
      },
      {
        key: 'enabled',
        header: t('pages.notifications.colEnabled'),
        accessor: (row) => (row.enabled ? 1 : 0),
        cell: (row) => (
          <Badge tone={row.enabled ? 'success' : 'neutral'}>
            {row.enabled ? t('common.enabled') : t('common.disabled')}
          </Badge>
        ),
      },
      {
        key: 'actions',
        header: '',
        width: '1%',
        cell: (row) => (
          <div className="flex items-center justify-end gap-1">
            <Button
              size="sm"
              variant="ghost"
              onClick={() => testMutation.mutate(row.id)}
              title={t('pages.notifications.test')}
            >
              <PaperPlaneTilt className="h-4 w-4" />
            </Button>
            {canWrite && (
              <>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => setEditing(row)}
                  title={t('common.edit')}
                >
                  <Pencil className="h-4 w-4" />
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => setPendingDelete(row)}
                  title={t('common.delete')}
                >
                  <Trash className="h-4 w-4 text-danger" />
                </Button>
              </>
            )}
          </div>
        ),
      },
    ],
    [t, canWrite, testMutation],
  )

  return (
    <>
      <PageHeader
        title={t('pages.notifications.title')}
        description={t('pages.notifications.description')}
        actions={
          canWrite ? (
            <Button variant="primary" onClick={() => setCreateOpen(true)}>
              <Plus className="h-4 w-4" />
              {t('pages.notifications.createChannel')}
            </Button>
          ) : undefined
        }
      />

      <div className="space-y-4">
        {/* Channels */}
        <Card>
          <CardBody>
            {channelsQuery.isLoading ? (
              <SkeletonRows columns={5} rows={3} />
            ) : channelsQuery.isError ? (
              <ErrorState error={channelsQuery.error} onRetry={invalidate} />
            ) : channels.length === 0 ? (
              <EmptyState
                icon={<Bell className="h-10 w-10" />}
                title={t('pages.notifications.noChannels')}
                description={t('pages.notifications.noChannelsHint')}
                action={
                  canWrite ? (
                    <Button variant="primary" onClick={() => setCreateOpen(true)}>
                      <Plus className="h-4 w-4" />
                      {t('pages.notifications.createChannel')}
                    </Button>
                  ) : undefined
                }
              />
            ) : (
              <Table columns={columns} data={channels} rowKey={(row) => row.id} />
            )}
          </CardBody>
        </Card>

        {/* Threshold settings */}
        {settingsQuery.data && (
          <SettingsCard
            settings={settingsQuery.data}
            canWrite={canWrite}
            onSaved={invalidate}
          />
        )}

        {/* Alert history */}
        <Card>
          <CardBody>
            <div className="mb-4 flex flex-wrap items-center gap-3">
              <h3 className="text-base font-semibold text-fg">
                {t('pages.notifications.historyTitle')}
              </h3>
              <Select
                className="w-56"
                value={eventTypeFilter}
                onChange={(e) => {
                  setEventTypeFilter(e.target.value)
                  setEventsOffset(0)
                }}
                options={[
                  { value: '', label: t('pages.notifications.allEvents') },
                  ...NOTIFICATION_EVENT_TYPES.map((type) => ({
                    value: type,
                    label: t(`pages.notifications.event_${type.replace('.', '_')}`),
                  })),
                ]}
              />
              <Button
                size="sm"
                variant="ghost"
                className="ml-auto"
                onClick={() => eventsQuery.refetch()}
              >
                <ArrowClockwise className="h-4 w-4" />
              </Button>
            </div>

            {eventsQuery.isLoading ? (
              <SkeletonRows columns={4} rows={5} />
            ) : eventsQuery.isError ? (
              <ErrorState error={eventsQuery.error} onRetry={() => eventsQuery.refetch()} />
            ) : events.length === 0 ? (
              <EmptyState
                title={t('pages.notifications.noEvents')}
                description={t('pages.notifications.noEventsHint')}
              />
            ) : (
              <>
                <div className="divide-y divide-line">
                  {events.map((event) => (
                    <div key={event.id} className="flex items-start gap-3 py-2.5">
                      <Badge tone={SEVERITY_TONE[event.severity] ?? 'neutral'}>
                        {t(`pages.notifications.severity_${event.severity}`)}
                      </Badge>
                      <div className="min-w-0 flex-1">
                        <p className="truncate text-sm font-medium text-fg" title={event.title}>
                          {event.title}
                        </p>
                        <p className="truncate text-xs text-fg-subtle" title={event.message}>
                          {event.message}
                        </p>
                      </div>
                      <span className="shrink-0 text-xs text-fg-subtle" title={formatDateTime(event.created_at)}>
                        {formatRelative(event.created_at)}
                      </span>
                    </div>
                  ))}
                </div>
                <div className="mt-3 flex justify-end gap-2">
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={eventsOffset === 0}
                    onClick={() => setEventsOffset((o) => Math.max(0, o - 50))}
                  >
                    {t('pagination.prev')}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={events.length < 50}
                    onClick={() => setEventsOffset((o) => o + 50)}
                  >
                    {t('pagination.next')}
                  </Button>
                </div>
              </>
            )}
          </CardBody>
        </Card>
      </div>

      {createOpen && (
        <ChannelDialog
          onClose={() => setCreateOpen(false)}
          onSaved={() => {
            setCreateOpen(false)
            invalidate()
          }}
        />
      )}

      {editing && (
        <ChannelDialog
          initial={editing}
          onClose={() => setEditing(null)}
          onSaved={() => {
            setEditing(null)
            invalidate()
          }}
        />
      )}

      <ConfirmDialog
        open={Boolean(pendingDelete)}
        title={t('pages.notifications.confirmDeleteTitle')}
        description={t('pages.notifications.confirmDeleteDesc', {
          name: pendingDelete?.name,
        })}
        confirmLabel={t('common.delete')}
        tone="danger"
        onConfirm={() => {
          if (pendingDelete) deleteMutation.mutate(pendingDelete.id)
          setPendingDelete(null)
        }}
        onClose={() => setPendingDelete(null)}
      />
    </>
  )
}

/* ── Threshold settings ─────────────────────────────────────────── */

function SettingsCard({
  settings,
  canWrite,
  onSaved,
}: {
  settings: NotificationSettings
  canWrite: boolean
  onSaved: () => void
}) {
  const { t } = useTranslation()
  const toast = useToast()

  const [cpu, setCpu] = useState(String(settings.cpu_percent))
  const [memory, setMemory] = useState(String(settings.memory_percent))
  const [disk, setDisk] = useState(String(settings.disk_percent))
  const [dedup, setDedup] = useState(String(settings.dedup_window_secs))

  const saveMutation = useMutation({
    mutationFn: () =>
      notificationsApi.saveSettings({
        cpu_percent: Number(cpu),
        memory_percent: Number(memory),
        disk_percent: Number(disk),
        dedup_window_secs: Number(dedup),
      }),
    onSuccess: () => {
      toast.success(t('pages.notifications.settingsSaved'))
      onSaved()
    },
    onError: () => toast.error(t('pages.notifications.settingsSaveFailed')),
  })

  return (
    <Card>
      <CardBody>
        <h3 className="mb-4 text-base font-semibold text-fg">
          {t('pages.notifications.settingsTitle')}
        </h3>
        <div className="grid grid-cols-2 gap-4 md:grid-cols-4">
          <Input
            type="number"
            label={t('pages.notifications.cpuThreshold')}
            value={cpu}
            onChange={(e) => setCpu(e.target.value)}
          />
          <Input
            type="number"
            label={t('pages.notifications.memoryThreshold')}
            value={memory}
            onChange={(e) => setMemory(e.target.value)}
          />
          <Input
            type="number"
            label={t('pages.notifications.diskThreshold')}
            value={disk}
            onChange={(e) => setDisk(e.target.value)}
          />
          <Input
            type="number"
            label={t('pages.notifications.dedupWindow')}
            value={dedup}
            onChange={(e) => setDedup(e.target.value)}
          />
        </div>
        <p className="mt-2 text-xs text-fg-subtle">
          {t('pages.notifications.settingsHint')}
        </p>
        {canWrite && (
          <div className="mt-4 flex justify-end">
            <Button
              variant="primary"
              disabled={saveMutation.isPending}
              onClick={() => saveMutation.mutate()}
            >
              {t('common.save')}
            </Button>
          </div>
        )}
      </CardBody>
    </Card>
  )
}

/* ── Create / Edit channel dialog ───────────────────────────────── */

interface ChannelDialogProps {
  initial?: NotificationChannel
  onClose: () => void
  onSaved: () => void
}

function ChannelDialog({ initial, onClose, onSaved }: ChannelDialogProps) {
  const { t } = useTranslation()
  const toast = useToast()
  const isEdit = Boolean(initial)

  const [name, setName] = useState(initial?.name ?? '')
  const [kind, setKind] = useState<NotificationChannel['kind']>(initial?.kind ?? 'email')
  const [config, setConfig] = useState<Record<string, unknown>>(initial?.config ?? {})
  const [events, setEvents] = useState<Set<string>>(() => new Set(initial?.events ?? []))
  const [enabled, setEnabled] = useState(initial?.enabled ?? true)

  const setField = (key: string, value: unknown) =>
    setConfig((c) => ({ ...c, [key]: value }))
  const fieldValue = (key: string) => String(config[key] ?? '')

  const toList = (value: string) =>
    value
      .split(/[\n,;]/)
      .map((item) => item.trim())
      .filter(Boolean)

  const saveMutation = useMutation({
    mutationFn: async () => {
      const payload = {
        name: name.trim(),
        kind,
        config: unmaskSecrets(config),
        events: Array.from(events),
        enabled,
      }
      if (isEdit && initial) {
        await notificationsApi.updateChannel(initial.id, payload)
      } else {
        await notificationsApi.createChannel(payload)
      }
    },
    onSuccess: () => {
      toast.success(
        isEdit
          ? t('pages.notifications.channelSaved')
          : t('pages.notifications.channelCreated'),
      )
      onSaved()
    },
    onError: (error) =>
      toast.error(error.message || t('pages.notifications.channelSaveFailed')),
  })

  const toggleEvent = (type: string) =>
    setEvents((prev) => {
      const next = new Set(prev)
      if (next.has(type)) next.delete(type)
      else next.add(type)
      return next
    })

  const canSave = name.trim().length > 0 && !saveMutation.isPending

  return (
    <Dialog
      open
      onClose={onClose}
      title={
        isEdit
          ? t('pages.notifications.editChannelTitle')
          : t('pages.notifications.createChannelTitle')
      }
      className="max-w-2xl"
    >
      <div className="space-y-4">
        <div className="grid grid-cols-2 gap-4">
          <Input
            label={t('pages.notifications.fieldName')}
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder={t('pages.notifications.fieldNamePh')}
          />
          <Select
            label={t('pages.notifications.colKind')}
            value={kind}
            onChange={(e) => setKind(e.target.value as NotificationChannel['kind'])}
            options={[
              { value: 'email', label: t('pages.notifications.kind_email') },
              { value: 'wecom', label: t('pages.notifications.kind_wecom') },
              { value: 'dingtalk', label: t('pages.notifications.kind_dingtalk') },
              { value: 'webhook', label: t('pages.notifications.kind_webhook') },
            ]}
          />
        </div>

        {kind === 'email' && (
          <>
            <div className="grid grid-cols-3 gap-4">
              <Input
                className="col-span-2"
                label={t('pages.notifications.smtpHost')}
                value={fieldValue('smtp_host')}
                onChange={(e) => setField('smtp_host', e.target.value)}
                placeholder="smtp.example.com"
              />
              <Input
                type="number"
                label={t('pages.notifications.smtpPort')}
                value={fieldValue('smtp_port') || '465'}
                onChange={(e) => setField('smtp_port', Number(e.target.value))}
              />
            </div>
            <div className="grid grid-cols-2 gap-4">
              <Input
                label={t('pages.notifications.smtpUser')}
                value={fieldValue('smtp_user')}
                onChange={(e) => setField('smtp_user', e.target.value)}
              />
              <Input
                type="password"
                label={t('pages.notifications.smtpPass')}
                value={fieldValue('smtp_pass')}
                onChange={(e) => setField('smtp_pass', e.target.value)}
              />
            </div>
            <div className="grid grid-cols-2 gap-4">
              <Input
                label={t('pages.notifications.fromAddr')}
                value={fieldValue('from')}
                onChange={(e) => setField('from', e.target.value)}
                placeholder="pingwaf@example.com"
              />
              <Select
                label={t('pages.notifications.tlsMode')}
                value={fieldValue('tls') || 'ssl'}
                onChange={(e) => setField('tls', e.target.value)}
                options={[
                  { value: 'ssl', label: 'SSL/TLS (465)' },
                  { value: 'starttls', label: 'STARTTLS (587)' },
                  { value: 'none', label: t('pages.notifications.tlsNone') },
                ]}
              />
            </div>
            <div>
              <label className="mb-1 block text-sm font-medium text-fg">
                {t('pages.notifications.toAddrs')}
              </label>
              <textarea
                className="w-full rounded-md border border-border bg-surface px-3 py-2 text-sm text-fg outline-none focus:border-brand focus:ring-1 focus:ring-brand"
                rows={3}
                value={Array.isArray(config.to) ? (config.to as string[]).join('\n') : ''}
                onChange={(e) => setField('to', toList(e.target.value))}
                placeholder={'ops@example.com\nsec@example.com'}
              />
            </div>
          </>
        )}

        {(kind === 'wecom' || kind === 'dingtalk' || kind === 'webhook') && (
          <Input
            label={t('pages.notifications.webhookUrl')}
            value={fieldValue('url')}
            onChange={(e) => setField('url', e.target.value)}
            placeholder={
              kind === 'wecom'
                ? 'https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=…'
                : kind === 'dingtalk'
                  ? 'https://oapi.dingtalk.com/robot/send?access_token=…'
                  : 'https://example.com/hooks/pingwaf'
            }
          />
        )}
        {kind === 'dingtalk' && (
          <Input
            label={t('pages.notifications.dingtalkSecret')}
            value={fieldValue('secret')}
            onChange={(e) => setField('secret', e.target.value)}
            placeholder="SEC…"
            hint={t('pages.notifications.dingtalkSecretHint')}
          />
        )}
        {kind === 'webhook' && (
          <Input
            label={t('pages.notifications.webhookToken')}
            value={fieldValue('secret_token')}
            onChange={(e) => setField('secret_token', e.target.value)}
            hint={t('pages.notifications.webhookTokenHint')}
          />
        )}

        <div>
          <label className="mb-1 block text-sm font-medium text-fg">
            {t('pages.notifications.fieldEvents')}
          </label>
          <p className="mb-2 text-xs text-fg-subtle">
            {t('pages.notifications.fieldEventsHint')}
          </p>
          <div className="grid max-h-40 grid-cols-2 gap-1 overflow-y-auto rounded-md border border-border p-2">
            {NOTIFICATION_EVENT_TYPES.map((type) => (
              <label key={type} className="flex items-center gap-2 text-sm">
                <input
                  type="checkbox"
                  checked={events.has(type)}
                  onChange={() => toggleEvent(type)}
                  className="rounded border-border"
                />
                {t(`pages.notifications.event_${type.replace('.', '_')}`)}
              </label>
            ))}
          </div>
        </div>

        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={enabled}
            onChange={(e) => setEnabled(e.target.checked)}
            className="rounded border-border"
          />
          {t('common.enabled')}
        </label>

        <div className="flex justify-end gap-2 pt-2">
          <Button variant="ghost" onClick={onClose}>
            {t('common.cancel')}
          </Button>
          <Button
            variant="primary"
            disabled={!canSave}
            onClick={() => saveMutation.mutate()}
          >
            {isEdit ? t('common.save') : t('common.create')}
          </Button>
        </div>
      </div>
    </Dialog>
  )
}
