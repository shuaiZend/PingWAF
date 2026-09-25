import { useEffect, useState, type ReactNode } from 'react'
import { useParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  Cloud,
  ArrowClockwise,
  Warning,
  ShieldWarning,
  Funnel,
  Prohibit,
} from '@phosphor-icons/react'
import { PageHeader } from '@/components/PageHeader'
import { Card, CardBody, CardHeader } from '@/components/ui/Card'
import { Button } from '@/components/ui/Button'
import { Input } from '@/components/ui/Input'
import { Select } from '@/components/ui/Select'
import { Switch } from '@/components/ui/Switch'
import { TagInput } from '@/components/ui/MultiSelect'
import { SkeletonStat } from '@/components/ui/Skeleton'
import { useToast } from '@/components/ui/Toast'
import { ErrorState } from '@/components/ErrorState'
import { challengeApi, challengeKeys, defaultChallengeConfig } from '@/api/challenge'
import { useCanWrite } from '@/hooks'
import { formatNumber, formatPercent } from '@/lib/format'
import { CHALLENGE_LEVELS, type ChallengeConfig } from '@/api/types'

export function CcProtectionPage() {
  const { t } = useTranslation()
  const toast = useToast()
  const queryClient = useQueryClient()
  const canWrite = useCanWrite()
  const { siteId = '' } = useParams<{ siteId: string }>()

  const [config, setConfig] = useState<ChallengeConfig | null>(null)
  const [dirty, setDirty] = useState(false)

  const configQuery = useQuery({
    queryKey: challengeKeys.config(siteId),
    queryFn: () => challengeApi.get(siteId),
    enabled: Boolean(siteId),
  })

  const statsQuery = useQuery({
    queryKey: challengeKeys.stats(siteId),
    queryFn: () => challengeApi.stats(siteId),
    enabled: Boolean(siteId),
  })

  useEffect(() => {
    if (configQuery.data) {
      setConfig(configQuery.data)
      setDirty(false)
    } else if (configQuery.isError && siteId) {
      setConfig(defaultChallengeConfig(siteId))
    }
  }, [configQuery.data, configQuery.isError, siteId])

  const patch = (part: Partial<ChallengeConfig>) => {
    setConfig((c) => (c ? { ...c, ...part } : c))
    setDirty(true)
  }

  const invalidate = () => {
    void queryClient.invalidateQueries({ queryKey: challengeKeys.all(siteId) })
  }

  const save = useMutation({
    mutationFn: (payload: ChallengeConfig) =>
      challengeApi.update(siteId, {
        enabled: payload.enabled,
        under_attack: payload.under_attack,
        challenge_level: payload.challenge_level,
        clearance_duration: payload.clearance_duration,
        rate_threshold: payload.rate_threshold,
        exempt_paths: payload.exempt_paths,
        browser_integrity_check: payload.browser_integrity_check,
        tls_fingerprint_check: payload.tls_fingerprint_check,
      }),
    onSuccess: (saved) => {
      setConfig(saved)
      setDirty(false)
      toast.success(t('pages.cc.saved'))
      invalidate()
    },
  })

  const stats = statsQuery.data

  return (
    <div className="animate-slide-up">
      <PageHeader
        title={t('pages.cc.title')}
        description={t('pages.cc.description')}
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
              icon={<Funnel weight="duotone" className="h-4 w-4" />}
              label={t('pages.cc.challengesServed')}
              value={formatNumber(stats?.challenges_served ?? 0)}
            />
            <StatTile
              icon={<ShieldWarning weight="duotone" className="h-4 w-4" />}
              label={t('pages.cc.passRate')}
              value={formatPercent(stats?.pass_rate ?? 0)}
              tone="success"
            />
            <StatTile
              icon={<Prohibit weight="duotone" className="h-4 w-4" />}
              label={t('pages.cc.blockRate')}
              value={formatPercent(stats?.block_rate ?? 0)}
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
          {config.under_attack && (
            <div className="flex items-start gap-3 rounded-lg border border-danger/40 bg-danger/10 px-4 py-3">
              <Warning weight="fill" className="mt-0.5 h-5 w-5 shrink-0 text-fg-danger" />
              <div>
                <p className="text-sm font-medium text-fg-strong">
                  {t('pages.cc.underAttackTitle')}
                </p>
                <p className="mt-0.5 text-[13px] text-fg-subtle">
                  {t('pages.cc.underAttackHint')}
                </p>
              </div>
            </div>
          )}

          <Card>
            <CardHeader title={t('pages.cc.general')} />
            <CardBody className="flex flex-col gap-5">
              <Switch
                checked={config.enabled}
                disabled={!canWrite}
                onCheckedChange={(enabled) => patch({ enabled })}
                label={t('pages.cc.enable')}
                description={t('pages.cc.enableHint')}
              />
              <Switch
                checked={config.under_attack}
                disabled={!canWrite || !config.enabled}
                onCheckedChange={(under_attack) => patch({ under_attack })}
                label={t('pages.cc.underAttack')}
                description={t('pages.cc.underAttackDescription')}
              />
              <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
                <Select
                  label={t('pages.cc.challengeLevel')}
                  value={config.challenge_level}
                  disabled={!canWrite || !config.enabled}
                  options={CHALLENGE_LEVELS.map((l) => ({
                    value: l,
                    label: t(`challengeLevels.${l}`, l),
                  }))}
                  onChange={(e) => patch({ challenge_level: e.target.value })}
                />
                <Input
                  type="number"
                  label={t('pages.cc.clearanceDuration')}
                  value={config.clearance_duration}
                  min={1}
                  disabled={!canWrite || !config.enabled}
                  hint={t('pages.cc.minutes')}
                  onChange={(e) => patch({ clearance_duration: Number(e.target.value) })}
                />
                <Input
                  type="number"
                  label={t('pages.cc.rateThreshold')}
                  value={config.rate_threshold}
                  min={1}
                  disabled={!canWrite || !config.enabled}
                  hint={t('pages.cc.rateThresholdHint')}
                  onChange={(e) => patch({ rate_threshold: Number(e.target.value) })}
                />
              </div>
            </CardBody>
          </Card>

          <Card>
            <CardHeader
              title={t('pages.cc.checks')}
              description={t('pages.cc.checksHint')}
            />
            <CardBody className="flex flex-col gap-5">
              <Switch
                checked={config.browser_integrity_check}
                disabled={!canWrite || !config.enabled}
                onCheckedChange={(browser_integrity_check) => patch({ browser_integrity_check })}
                label={t('pages.cc.browserIntegrity')}
                description={t('pages.cc.browserIntegrityHint')}
              />
              <Switch
                checked={config.tls_fingerprint_check}
                disabled={!canWrite || !config.enabled}
                onCheckedChange={(tls_fingerprint_check) => patch({ tls_fingerprint_check })}
                label={t('pages.cc.tlsFingerprint')}
                description={t('pages.cc.tlsFingerprintHint')}
              />
            </CardBody>
          </Card>

          <Card>
            <CardHeader
              title={t('pages.cc.exemptPaths')}
              description={t('pages.cc.exemptPathsHint')}
              action={
                <span className="flex items-center gap-1.5 text-xs text-fg-subtle">
                  <Cloud weight="duotone" className="h-4 w-4" />
                  {config.exempt_paths.length}
                </span>
              }
            />
            <CardBody>
              <TagInput
                value={config.exempt_paths}
                disabled={!canWrite || !config.enabled}
                placeholder="/health"
                suggestions={['/health', '/status', '/api/heartbeat']}
                onChange={(exempt_paths) => patch({ exempt_paths })}
              />
            </CardBody>
          </Card>
        </div>
      ) : null}
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

export default CcProtectionPage
