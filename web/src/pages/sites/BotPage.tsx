import { useEffect, useState, type ReactNode } from 'react'
import { useParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  Robot,
  ArrowClockwise,
  Plus,
  Trash,
  Bug,
  Fingerprint,
  Brain,
  UserFocus,
} from '@phosphor-icons/react'
import { PageHeader } from '@/components/PageHeader'
import { Card, CardBody, CardHeader } from '@/components/ui/Card'
import { Button } from '@/components/ui/Button'
import { Input } from '@/components/ui/Input'
import { Select } from '@/components/ui/Select'
import { Switch } from '@/components/ui/Switch'
import { Badge } from '@/components/ui/Badge'
import { Dialog } from '@/components/ui/Dialog'
import { Table, type Column } from '@/components/ui/Table'
import { ConfirmDialog } from '@/components/ui/ConfirmDialog'
import { SkeletonStat } from '@/components/ui/Skeleton'
import { useToast } from '@/components/ui/Toast'
import { ErrorState } from '@/components/ErrorState'
import { botApi, botKeys, defaultBotConfig, COMMON_KNOWN_BOTS } from '@/api/bot'
import { useCanWrite } from '@/hooks'
import { formatNumber, formatPercent } from '@/lib/format'
import { BOT_ACTIONS, type BotConfig, type KnownBot } from '@/api/types'

interface BotForm {
  name: string
  ua_pattern: string
  action: string
}

const emptyBotForm = (): BotForm => ({ name: '', ua_pattern: '', action: 'allow' })

export function BotPage() {
  const { t } = useTranslation()
  const toast = useToast()
  const queryClient = useQueryClient()
  const canWrite = useCanWrite()
  const { siteId = '' } = useParams<{ siteId: string }>()

  const [config, setConfig] = useState<BotConfig | null>(null)
  const [dirty, setDirty] = useState(false)
  const [dialogOpen, setDialogOpen] = useState(false)
  const [botForm, setBotForm] = useState<BotForm>(emptyBotForm())
  const [pendingDelete, setPendingDelete] = useState<KnownBot | null>(null)

  const configQuery = useQuery({
    queryKey: botKeys.config(siteId),
    queryFn: () => botApi.get(siteId),
    enabled: Boolean(siteId),
  })

  const statsQuery = useQuery({
    queryKey: botKeys.stats(siteId),
    queryFn: () => botApi.stats(siteId),
    enabled: Boolean(siteId),
  })

  useEffect(() => {
    if (configQuery.data) {
      setConfig(configQuery.data)
      setDirty(false)
    } else if (configQuery.isError && siteId) {
      setConfig(defaultBotConfig(siteId))
    }
  }, [configQuery.data, configQuery.isError, siteId])

  const patch = (part: Partial<BotConfig>) => {
    setConfig((c) => (c ? { ...c, ...part } : c))
    setDirty(true)
  }

  const invalidate = () => {
    void queryClient.invalidateQueries({ queryKey: botKeys.all(siteId) })
  }

  const save = useMutation({
    mutationFn: (payload: BotConfig) =>
      botApi.update(siteId, {
        enabled: payload.enabled,
        user_agent_analysis: payload.user_agent_analysis,
        js_detection: payload.js_detection,
        tls_fingerprinting: payload.tls_fingerprinting,
        behavioral_analysis: payload.behavioral_analysis,
        action: payload.action,
      }),
    onSuccess: (saved) => {
      setConfig(saved)
      setDirty(false)
      toast.success(t('pages.bot.saved'))
      invalidate()
    },
  })

  const addBot = useMutation({
    mutationFn: (bot: Omit<KnownBot, 'id'>) => botApi.addKnownBot(siteId, bot),
    onSuccess: () => {
      toast.success(t('pages.bot.botAdded'))
      setDialogOpen(false)
      setBotForm(emptyBotForm())
      invalidate()
    },
  })

  const removeBot = useMutation({
    mutationFn: (id: string) => botApi.removeKnownBot(siteId, id),
    onSuccess: () => {
      toast.success(t('pages.bot.botRemoved'))
      setPendingDelete(null)
      invalidate()
    },
  })

  const addCommon = (bot: Omit<KnownBot, 'id'>) => addBot.mutate(bot)

  const stats = statsQuery.data

  const columns: Column<KnownBot>[] = [
    {
      key: 'name',
      header: t('common.name'),
      accessor: (b) => b.name,
      cell: (b) => (
        <span className="text-[13px] font-medium text-fg-strong">{b.name}</span>
      ),
    },
    {
      key: 'ua_pattern',
      header: t('pages.bot.uaPattern'),
      accessor: (b) => b.ua_pattern,
      cell: (b) => (
        <span className="pw-mono text-xs text-fg-subtle">{b.ua_pattern}</span>
      ),
    },
    {
      key: 'action',
      header: t('pages.waf.action'),
      accessor: (b) => b.action,
      width: '1%',
      cell: (b) => (
        <Badge
          tone={b.action === 'block' ? 'danger' : b.action === 'challenge' ? 'warning' : 'success'}
          size="sm"
        >
          {t(`actions.${b.action}`, b.action)}
        </Badge>
      ),
    },
    {
      key: 'row-actions',
      header: '',
      align: 'right',
      width: '1%',
      cell: (b) => (
        <Button
          size="icon"
          variant="ghost"
          className="hover:text-fg-danger"
          aria-label={t('common.delete')}
          disabled={!canWrite}
          onClick={() => setPendingDelete(b)}
          icon={<Trash weight="duotone" className="h-4 w-4" />}
        />
      ),
    },
  ]

  const knownBots = config?.known_bots ?? []

  return (
    <div className="animate-slide-up">
      <PageHeader
        title={t('pages.bot.title')}
        description={t('pages.bot.description')}
        actions={
          <div className="flex items-center gap-2">
            <Button
              variant="secondary"
              loading={configQuery.isFetching}
              onClick={() => {
                configQuery.refetch()
                statsQuery.refetch()
              }}
              icon={<ArrowClockwise weight="duotone" className="h-4 w-4" />}
            >
              {t('common.refresh')}
            </Button>
            {canWrite && (
              <Button
                variant="primary"
                disabled={!dirty || save.isPending}
                loading={save.isPending}
                onClick={() => config && save.mutate(config)}
              >
                {t('common.save')}
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
            <StatTile
              icon={<Robot weight="duotone" className="h-4 w-4" />}
              label={t('pages.bot.botRequestPct')}
              value={formatPercent(stats?.bot_request_pct ?? 0)}
            />
            <StatTile
              icon={<Bug weight="duotone" className="h-4 w-4" />}
              label={t('pages.bot.verifiedBots')}
              value={formatNumber(stats?.verified_bots ?? 0)}
              tone="success"
            />
            <StatTile
              icon={<UserFocus weight="duotone" className="h-4 w-4" />}
              label={t('pages.bot.likelyBots')}
              value={formatNumber(stats?.likely_bots ?? 0)}
              tone="danger"
            />
          </>
        )}
      </div>

      {configQuery.isError && !config ? (
        <ErrorState
          error={configQuery.error}
          onRetry={() => configQuery.refetch()}
          retrying={configQuery.isFetching}
        />
      ) : config ? (
        <div className="flex flex-col gap-6">
          <Card>
            <CardHeader title={t('pages.bot.detection')} description={t('pages.bot.detectionHint')} />
            <CardBody className="flex flex-col gap-5">
              <Switch
                checked={config.enabled}
                disabled={!canWrite}
                onCheckedChange={(enabled) => patch({ enabled })}
                label={t('pages.bot.enable')}
                description={t('pages.bot.enableHint')}
              />
              <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
                <MethodToggle
                  icon={<Bug weight="duotone" className="h-4 w-4" />}
                  checked={config.user_agent_analysis}
                  disabled={!canWrite || !config.enabled}
                  onChange={(user_agent_analysis) => patch({ user_agent_analysis })}
                  label={t('pages.bot.userAgentAnalysis')}
                  description={t('pages.bot.userAgentAnalysisHint')}
                />
                <MethodToggle
                  icon={<Fingerprint weight="duotone" className="h-4 w-4" />}
                  checked={config.js_detection}
                  disabled={!canWrite || !config.enabled}
                  onChange={(js_detection) => patch({ js_detection })}
                  label={t('pages.bot.jsDetection')}
                  description={t('pages.bot.jsDetectionHint')}
                />
                <MethodToggle
                  icon={<Brain weight="duotone" className="h-4 w-4" />}
                  checked={config.tls_fingerprinting}
                  disabled={!canWrite || !config.enabled}
                  onChange={(tls_fingerprinting) => patch({ tls_fingerprinting })}
                  label={t('pages.bot.tlsFingerprinting')}
                  description={t('pages.bot.tlsFingerprintingHint')}
                />
                <MethodToggle
                  icon={<UserFocus weight="duotone" className="h-4 w-4" />}
                  checked={config.behavioral_analysis}
                  disabled={!canWrite || !config.enabled}
                  onChange={(behavioral_analysis) => patch({ behavioral_analysis })}
                  label={t('pages.bot.behavioralAnalysis')}
                  description={t('pages.bot.behavioralAnalysisHint')}
                />
              </div>
              <Select
                label={t('pages.bot.action')}
                value={config.action}
                disabled={!canWrite || !config.enabled}
                containerClassName="max-w-xs"
                hint={t('pages.bot.actionHint')}
                options={BOT_ACTIONS.map((a) => ({ value: a, label: t(`actions.${a}`, a) }))}
                onChange={(e) => patch({ action: e.target.value })}
              />
            </CardBody>
          </Card>

          <Card>
            <CardHeader
              title={t('pages.bot.knownBots')}
              description={t('pages.bot.knownBotsHint')}
              action={
                canWrite && (
                  <Button
                    size="sm"
                    variant="secondary"
                    icon={<Plus weight="bold" className="h-4 w-4" />}
                    onClick={() => {
                      setBotForm(emptyBotForm())
                      setDialogOpen(true)
                    }}
                  >
                    {t('pages.bot.addBot')}
                  </Button>
                )
              }
            />
            <CardBody className="p-0">
              <Table
                columns={columns}
                data={knownBots}
                rowKey={(b) => b.id}
                dense
                empty={
                  <div className="py-10 text-center text-sm text-fg-subtle">
                    {t('pages.bot.noKnownBots')}
                  </div>
                }
              />
            </CardBody>
            {canWrite && (
              <div className="flex flex-wrap items-center gap-2 border-t border-line px-5 py-3">
                <span className="text-xs text-fg-subtle">{t('pages.bot.quickAdd')}</span>
                {COMMON_KNOWN_BOTS.filter(
                  (c) => !knownBots.some((b) => b.ua_pattern === c.ua_pattern),
                )
                  .slice(0, 6)
                  .map((c) => (
                    <button
                      key={c.name}
                      type="button"
                      disabled={addBot.isPending}
                      onClick={() => addCommon(c)}
                      className="rounded-full border border-dashed border-line px-2.5 py-0.5 text-xs text-fg-subtle transition-colors hover:border-brand hover:text-brand"
                    >
                      + {c.name}
                    </button>
                  ))}
              </div>
            )}
          </Card>
        </div>
      ) : null}

      {/* Add known bot */}
      <Dialog
        open={dialogOpen}
        onClose={addBot.isPending ? () => undefined : () => setDialogOpen(false)}
        size="md"
        title={t('pages.bot.addBot')}
        description={t('pages.bot.addBotDescription')}
        footer={
          <>
            <Button variant="ghost" onClick={() => setDialogOpen(false)} disabled={addBot.isPending}>
              {t('common.cancel')}
            </Button>
            <Button
              variant="primary"
              loading={addBot.isPending}
              onClick={() =>
                addBot.mutate({
                  name: botForm.name.trim(),
                  ua_pattern: botForm.ua_pattern.trim(),
                  action: botForm.action,
                })
              }
            >
              {t('common.create')}
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-4">
          <Input
            label={t('common.name')}
            value={botForm.name}
            autoFocus
            placeholder="Googlebot"
            onChange={(e) => setBotForm((f) => ({ ...f, name: e.target.value }))}
            required
          />
          <Input
            label={t('pages.bot.uaPattern')}
            value={botForm.ua_pattern}
            className="pw-mono text-[13px]"
            placeholder="Googlebot"
            hint={t('pages.bot.uaPatternHint')}
            onChange={(e) => setBotForm((f) => ({ ...f, ua_pattern: e.target.value }))}
          />
          <Select
            label={t('pages.waf.action')}
            value={botForm.action}
            options={BOT_ACTIONS.map((a) => ({ value: a, label: t(`actions.${a}`, a) }))}
            onChange={(e) => setBotForm((f) => ({ ...f, action: e.target.value }))}
          />
        </div>
      </Dialog>

      <ConfirmDialog
        open={pendingDelete !== null}
        onClose={() => setPendingDelete(null)}
        onConfirm={() => pendingDelete && removeBot.mutate(pendingDelete.id)}
        title={t('pages.bot.deleteBotTitle')}
        description={t('pages.bot.deleteBotDescription')}
        confirmLabel={t('common.delete')}
        loading={removeBot.isPending}
      >
        {pendingDelete && (
          <div className="rounded-md border border-line bg-recessed px-3 py-2">
            <p className="text-[13px] font-medium text-fg-strong">{pendingDelete.name}</p>
            <p className="pw-mono mt-0.5 text-xs text-fg-subtle">{pendingDelete.ua_pattern}</p>
          </div>
        )}
      </ConfirmDialog>
    </div>
  )
}

function MethodToggle({
  icon,
  checked,
  disabled,
  onChange,
  label,
  description,
}: {
  icon: ReactNode
  checked: boolean
  disabled?: boolean
  onChange: (v: boolean) => void
  label: string
  description: string
}) {
  return (
    <div className="flex items-start gap-3 rounded-lg border border-line bg-recessed/40 p-3">
      <span className="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-brand-soft text-brand">
        {icon}
      </span>
      <Switch
        checked={checked}
        disabled={disabled}
        onCheckedChange={onChange}
        label={label}
        description={description}
      />
    </div>
  )
}

function StatTile({
  icon,
  label,
  value,
  tone = 'brand',
}: {
  icon: ReactNode
  label: string
  value: string
  tone?: 'brand' | 'success' | 'danger'
}) {
  const toneClass = {
    brand: 'bg-brand-soft text-brand',
    success: 'bg-success/12 text-fg-success',
    danger: 'bg-danger/12 text-fg-danger',
  }[tone]
  return (
    <Card padded>
      <div className="flex items-center justify-between gap-3">
        <div className="min-w-0">
          <p className="text-xs font-medium uppercase tracking-wide text-fg-subtle">{label}</p>
          <p className="mt-1.5 text-2xl font-semibold tabular-nums text-fg-strong">{value}</p>
        </div>
        <span className={`flex h-9 w-9 shrink-0 items-center justify-center rounded-lg ${toneClass}`}>
          {icon}
        </span>
      </div>
    </Card>
  )
}

export default BotPage
