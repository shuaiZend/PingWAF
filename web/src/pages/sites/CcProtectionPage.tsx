import { useState } from 'react'
import { useParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ArrowClockwise, Warning } from '@phosphor-icons/react'
import { PageHeader } from '@/components/PageHeader'
import { Card, CardBody, CardHeader } from '@/components/ui/Card'
import { Button } from '@/components/ui/Button'
import { Input } from '@/components/ui/Input'
import { Select } from '@/components/ui/Select'
import { Switch } from '@/components/ui/Switch'
import { TagInput } from '@/components/ui/MultiSelect'
import { useToast } from '@/components/ui/Toast'
import { ErrorState } from '@/components/ErrorState'
import { challengeApi, challengeKeys, defaultChallengeConfig } from '@/api/challenge'
import { useCanWrite } from '@/hooks'
import { CHALLENGE_LEVELS, type ChallengeConfig } from '@/api/types'

const MIN_CLEARANCE_SECS = 60
const MAX_CLEARANCE_SECS = 86_400

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

  // Seed the form from the query (render-phase reset on new data / first
  // failure), so nothing flashes stale values on the first paint.
  const serverConfig = configQuery.data
  const loadFailed = configQuery.isError && Boolean(siteId)
  // `null` until the first seed: cached query data present at mount must still
  // reach the form.
  const [sync, setSync] = useState<{ config: typeof serverConfig; failed: boolean } | null>(null)
  if (!sync || sync.config !== serverConfig || sync.failed !== loadFailed) {
    setSync({ config: serverConfig, failed: loadFailed })
    if (serverConfig) {
      setConfig(serverConfig)
      setDirty(false)
    } else if (loadFailed) {
      setConfig(defaultChallengeConfig())
    }
  }

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
        under_attack_mode: payload.under_attack_mode,
        default_level: payload.default_level,
        clearance_duration_secs: payload.clearance_duration_secs,
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
              onClick={() => configQuery.refetch()}
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

      {configQuery.isError && !config ? (
        <ErrorState
          error={configQuery.error}
          onRetry={() => configQuery.refetch()}
          retrying={configQuery.isFetching}
        />
      ) : config ? (
        <div className="flex flex-col gap-6">
          {config.under_attack_mode && (
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
                checked={config.under_attack_mode}
                disabled={!canWrite || !config.enabled}
                onCheckedChange={(under_attack_mode) => patch({ under_attack_mode })}
                label={t('pages.cc.underAttack')}
                description={t('pages.cc.underAttackDescription')}
              />
              <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
                <Select
                  label={t('pages.cc.challengeLevel')}
                  value={config.default_level}
                  disabled={!canWrite || !config.enabled}
                  options={CHALLENGE_LEVELS.map((l) => ({
                    value: l,
                    label: t(`challengeLevels.${l}`, l),
                  }))}
                  onChange={(e) => patch({ default_level: e.target.value })}
                />
                <Input
                  type="number"
                  label={t('pages.cc.clearanceDuration')}
                  value={Math.round(config.clearance_duration_secs / 60)}
                  min={MIN_CLEARANCE_SECS / 60}
                  max={MAX_CLEARANCE_SECS / 60}
                  disabled={!canWrite || !config.enabled}
                  hint={t('pages.cc.minutes')}
                  onChange={(e) =>
                    patch({
                      clearance_duration_secs: Math.min(
                        MAX_CLEARANCE_SECS,
                        Math.max(MIN_CLEARANCE_SECS, Number(e.target.value) * 60),
                      ),
                    })
                  }
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

export default CcProtectionPage
