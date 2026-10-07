import { useEffect, useState } from 'react'
import { Navigate, useLocation, useParams, useSearchParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Cloud, Gauge, Lightning, ShieldCheck, Sliders, Warning } from '@phosphor-icons/react'
import { PageHeader } from '@/components/PageHeader'
import { Tabs } from '@/components/ui/Tabs'
import { Switch } from '@/components/ui/Switch'
import { SkeletonCard } from '@/components/ui/Skeleton'
import { ConfirmDialog } from '@/components/ui/ConfirmDialog'
import { useToast } from '@/components/ui/Toast'
import { challengeApi, challengeKeys } from '@/api/challenge'
import { wafSettingsApi, wafSettingsKeys } from '@/api/wafSettings'
import { useCanWrite, useWafSettingsMutation } from '@/hooks'
import { ManagedRulesPanel } from './ManagedRulesPanel'
import { CustomRulesPanel } from './CustomRulesPanel'
import { MitigationPanel } from './MitigationPanel'

type ProtectionTab = 'managed' | 'custom' | 'cc'

const PROTECTION_TABS: ProtectionTab[] = ['managed', 'custom', 'cc']

/** Pre-0.22 tab values, still accepted in links and normalized on load. */
const LEGACY_TABS: Record<string, ProtectionTab> = {
  settings: 'managed',
  rules: 'custom',
}

function parseTab(value: string | null): ProtectionTab {
  if (value && LEGACY_TABS[value]) return LEGACY_TABS[value]
  return PROTECTION_TABS.includes(value as ProtectionTab)
    ? (value as ProtectionTab)
    : 'managed'
}

/**
 * Web protection for one site: the built-in managed rules and attack
 * categories, the user's own WAF rules, and rate-limiting/challenge
 * mitigation under one roof, fronted by the under-attack banner.
 */
export function ProtectionPage() {
  const { t } = useTranslation()
  const toast = useToast()
  const queryClient = useQueryClient()
  const canWrite = useCanWrite()
  const { siteId = '' } = useParams<{ siteId: string }>()
  const [searchParams, setSearchParams] = useSearchParams()
  const tab = parseTab(searchParams.get('tab'))

  const setTab = (next: string) => {
    const params = new URLSearchParams(searchParams)
    params.set('tab', next)
    setSearchParams(params, { replace: true })
  }

  // Rewrite a legacy `?tab=settings|rules` link in place so the address bar
  // always shows the current vocabulary — every other param (`?rule=`) stays.
  useEffect(() => {
    const raw = searchParams.get('tab')
    if (raw !== 'settings' && raw !== 'rules') return
    const params = new URLSearchParams(searchParams)
    params.set('tab', parseTab(raw))
    setSearchParams(params, { replace: true })
  }, [searchParams, setSearchParams])

  const challengeQuery = useQuery({
    queryKey: challengeKeys.config(siteId),
    queryFn: () => challengeApi.get(siteId),
    enabled: Boolean(siteId),
  })
  const settingsQuery = useQuery({
    queryKey: wafSettingsKeys.settings(siteId),
    queryFn: () => wafSettingsApi.get(siteId),
    enabled: Boolean(siteId),
  })
  const settings = settingsQuery.data
  const updateSettings = useWafSettingsMutation(siteId)

  const underAttack = challengeQuery.data?.under_attack_mode ?? false

  const setUnderAttack = useMutation({
    mutationFn: (enabled: boolean) =>
      challengeApi.update(siteId, {
        // The challenge gate must be on for the banner to mean anything; the
        // server forces the same pairing.
        under_attack_mode: enabled,
        ...(enabled ? { enabled: true } : {}),
      }),
    onSuccess: (saved) => {
      queryClient.setQueryData(challengeKeys.config(siteId), saved)
      toast.success(
        saved.under_attack_mode
          ? t('pages.protection.underAttackOn')
          : t('pages.protection.underAttackOff'),
      )
      void queryClient.invalidateQueries({ queryKey: challengeKeys.all(siteId) })
    },
    onError: (error) => {
      toast.error(
        t('pages.protection.updateFailed'),
        error instanceof Error ? error.message : undefined,
      )
    },
  })

  const [confirmUnderAttack, setConfirmUnderAttack] = useState(false)

  return (
    <div className="animate-slide-up">
      <PageHeader
        title={t('pages.protection.title')}
        description={t('pages.protection.description')}
      />

      {/* ── Mitigation modes: under attack + deep inspection, side by side ── */}
      <div className="mb-5 grid gap-4 lg:grid-cols-2">
      {challengeQuery.isPending ? (
        <SkeletonCard className="h-16" />
      ) : (
        <div
          className={`flex flex-wrap items-center justify-between gap-4 rounded-lg border px-4 py-3 ${
            underAttack
              ? 'border-danger/45 bg-danger/10'
              : 'border-line bg-elevated'
          }`}
        >
          <span className="flex min-w-0 items-center gap-3">
            <span
              className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-lg ${
                underAttack
                  ? 'bg-danger/15 text-fg-danger'
                  : 'bg-recessed text-fg-subtle'
              }`}
            >
              {underAttack ? (
                <Warning weight="fill" className="h-5 w-5" />
              ) : (
                <ShieldCheck weight="duotone" className="h-5 w-5" />
              )}
            </span>
            <span className="min-w-0">
              <span className="block text-sm font-semibold text-fg-strong">
                {underAttack
                  ? t('pages.protection.underAttackActive')
                  : t('pages.protection.underAttackTitle')}
              </span>
              <span className="mt-0.5 block text-[13px] text-fg-subtle">
                {underAttack
                  ? t('pages.protection.underAttackActiveHint')
                  : t('pages.protection.underAttackHint')}
              </span>
            </span>
          </span>
          {canWrite && (
            <Switch
              size="md"
              checked={underAttack}
              disabled={!canWrite || setUnderAttack.isPending}
              aria-label={t('pages.protection.underAttackTitle')}
              onCheckedChange={(enabled) => {
                if (enabled) {
                  setConfirmUnderAttack(true)
                } else {
                  setUnderAttack.mutate(false)
                }
              }}
            />
          )}
        </div>
      )}
      {settingsQuery.isPending ? (
        <SkeletonCard className="h-16" />
      ) : (
        <div
          className={`flex flex-wrap items-center justify-between gap-4 rounded-lg border px-4 py-3 ${
            settings?.advanced_mode
              ? 'border-danger/45 bg-danger/10'
              : 'border-line bg-elevated'
          }`}
        >
          <span className="flex min-w-0 items-center gap-3">
            <span
              className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-lg ${
                settings?.advanced_mode
                  ? 'bg-danger/15 text-fg-danger'
                  : 'bg-recessed text-fg-subtle'
              }`}
            >
              <Lightning weight="duotone" className="h-5 w-5" />
            </span>
            <span className="min-w-0">
              <span className="block text-sm font-semibold text-fg-strong">
                {t('pages.protection.deepInspection')}
              </span>
              <span className="mt-0.5 block text-[13px] text-fg-subtle">
                {t('pages.protection.deepInspectionHint')}
              </span>
              <span className="mt-1.5 flex items-center gap-1.5 text-xs text-fg-warning">
                <Gauge weight="duotone" className="h-3.5 w-3.5 shrink-0" />
                {t('pages.protection.performanceHint')}
              </span>
            </span>
          </span>
          <Switch
            size="md"
            checked={settings?.advanced_mode ?? false}
            disabled={!canWrite || updateSettings.isPending}
            aria-label={t('pages.protection.deepInspection')}
            onCheckedChange={(advanced_mode) => updateSettings.mutate({ advanced_mode })}
          />
        </div>
      )}
      </div>

      <Tabs
        variant="pill"
        className="mb-5"
        value={tab}
        onChange={setTab}
        items={[
          {
            value: 'managed',
            label: t('pages.protection.tabManaged'),
            icon: <Sliders weight="duotone" className="h-4 w-4" />,
          },
          {
            value: 'custom',
            label: t('pages.protection.tabCustom'),
            icon: <ShieldCheck weight="duotone" className="h-4 w-4" />,
          },
          {
            value: 'cc',
            label: t('pages.protection.tabCc'),
            icon: <Cloud weight="duotone" className="h-4 w-4" />,
          },
        ]}
      />

      {tab === 'managed' && <ManagedRulesPanel />}
      {tab === 'custom' && <CustomRulesPanel />}
      {tab === 'cc' && <MitigationPanel />}

      <ConfirmDialog
        open={confirmUnderAttack}
        onClose={() => setConfirmUnderAttack(false)}
        onConfirm={() => {
          setConfirmUnderAttack(false)
          setUnderAttack.mutate(true)
        }}
        tone="danger"
        title={t('pages.protection.underAttackConfirmTitle')}
        description={t('pages.protection.underAttackConfirmDescription')}
        confirmLabel={t('pages.protection.underAttackCta')}
        loading={setUnderAttack.isPending}
      />
    </div>
  )
}

/** Forwards a pre-merge route (`/security/waf`, `/security/cc`) here, query intact. */
export function LegacyProtectionRedirect({ tab }: { tab: ProtectionTab }) {
  const { siteId = '' } = useParams<{ siteId: string }>()
  const location = useLocation()
  const params = new URLSearchParams(location.search)
  params.set('tab', tab)
  return (
    <Navigate to={`/sites/${siteId}/security/protection?${params.toString()}`} replace />
  )
}

export default ProtectionPage
