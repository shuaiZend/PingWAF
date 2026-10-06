import { useMemo, useState } from 'react'
import { useParams, useSearchParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { useQuery } from '@tanstack/react-query'
import { Gauge, Info, Lightning, CaretDown, ShieldWarning } from '@phosphor-icons/react'
import { Card, CardBody, CardHeader } from '@/components/ui/Card'
import { Switch } from '@/components/ui/Switch'
import { Badge } from '@/components/ui/Badge'
import { Button } from '@/components/ui/Button'
import { SkeletonRows } from '@/components/ui/Skeleton'
import { ErrorState } from '@/components/ErrorState'
import { managedRulesApi, wafSettingsApi, wafSettingsKeys } from '@/api/wafSettings'
import { useCanWrite, useWafSettingsMutation } from '@/hooks'
import { cn } from '@/lib/utils'
import { applyCategoryLinkage } from '@/lib/wafLinkage'
import {
  WAF_CATEGORIES,
  WAF_STACKS,
  type ManagedRule,
} from '@/api/types'

/** Recon/access-policy rules; every other category-less rule lands in the protocol group. */
const RECON_RULE_IDS = [
  'PINGWAF-1001',
  'PINGWAF-1010',
  'PINGWAF-1011',
  'PINGWAF-1020',
  'PINGWAF-1021',
  'PINGWAF-1040',
]

const ACTION_TONE: Record<string, 'danger' | 'warning' | 'info' | 'success' | 'neutral'> = {
  block: 'danger',
  challenge: 'warning',
  js_challenge: 'warning',
  log: 'info',
  allow: 'success',
}

/**
 * The managed-rules module of the protection page: attack categories and the
 * built-in rule set with their linkage, deep inspection, and the backend
 * stack downgrades.
 *
 * The category switch and the per-rule switch write different backend lists —
 * the engine treats them as overlapping downgrade sources — so a category
 * toggle carries its family rules along (see `applyCategoryLinkage`), while a
 * per-rule toggle stays an independent override.
 */
export function ManagedRulesPanel() {
  const { t } = useTranslation()
  const canWrite = useCanWrite()
  const { siteId = '' } = useParams<{ siteId: string }>()
  const [searchParams] = useSearchParams()
  // Deep link from the log console (`?rule=PINGWAF-…`): highlight the row.
  const highlightRule = searchParams.get('rule')

  const settingsQuery = useQuery({
    queryKey: wafSettingsKeys.settings(siteId),
    queryFn: () => wafSettingsApi.get(siteId),
    enabled: Boolean(siteId),
  })
  const settings = settingsQuery.data

  const managedRulesQuery = useQuery({
    queryKey: wafSettingsKeys.managedRules(),
    queryFn: () => managedRulesApi.list(),
    staleTime: 5 * 60 * 1000,
  })
  const managedRules = useMemo(() => managedRulesQuery.data ?? [], [managedRulesQuery.data])

  const updateSettings = useWafSettingsMutation(siteId)

  if (settingsQuery.isError && !settings) {
    return (
      <ErrorState
        error={settingsQuery.error}
        onRetry={() => settingsQuery.refetch()}
        retrying={settingsQuery.isFetching}
      />
    )
  }
  if (managedRulesQuery.isError && !managedRulesQuery.data) {
    return (
      <ErrorState
        error={managedRulesQuery.error}
        onRetry={() => managedRulesQuery.refetch()}
        retrying={managedRulesQuery.isFetching}
      />
    )
  }
  if (!settings || managedRulesQuery.isPending) {
    return <SkeletonRows rows={6} columns={3} />
  }

  const busy = updateSettings.isPending
  const categoriesOn = (settings.monitor_categories ?? []) as string[]
  const stacksOn = (settings.monitor_stacks ?? []) as string[]
  const monitoredRules = (settings.monitor_managed_rules ?? []) as string[]

  /** Category → carry its family rules along in the same PUT. */
  const toggleCategory = (category: string, logOnly: boolean) => {
    updateSettings.mutate(applyCategoryLinkage(category, logOnly, settings, managedRules))
  }

  const toggleStack = (stack: string, on: boolean) => {
    updateSettings.mutate({
      monitor_stacks: on
        ? [...stacksOn, stack]
        : stacksOn.filter((s) => s !== stack),
    })
  }

  const toggleManagedMonitor = (ruleId: string, on: boolean) => {
    updateSettings.mutate({
      monitor_managed_rules: on
        ? [...monitoredRules, ruleId]
        : monitoredRules.filter((id) => id !== ruleId),
    })
  }

  /** Batch the whole built-in set in one PUT: union with (or minus) all ids. */
  const batchAll = (logOnly: boolean) => {
    const allIds = managedRules.map((r) => r.id)
    updateSettings.mutate({
      monitor_managed_rules: logOnly
        ? [...new Set([...monitoredRules, ...allIds])]
        : monitoredRules.filter((id) => !allIds.includes(id)),
    })
  }

  const familyGroups = WAF_CATEGORIES.map((category) => ({
    category,
    rules: managedRules.filter((r) => r.category === category),
  })).filter((g) => g.rules.length > 0)

  const policyRules = managedRules.filter((r) => r.category === null)
  const reconRules = policyRules.filter((r) => RECON_RULE_IDS.includes(r.id))
  const protocolRules = policyRules.filter((r) => !RECON_RULE_IDS.includes(r.id))

  return (
    <div className="flex flex-col gap-4">
      {/* ── Attack categories ─────────────────────────────────────────── */}
      <Card>
        <CardHeader
          title={t('pages.protection.categories.cardTitle')}
          description={t('pages.protection.categories.cardHint')}
        />
        <CardBody>
          <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2 lg:grid-cols-3">
            {WAF_CATEGORIES.map((category) => {
              const logOnly = categoriesOn.includes(category)
              const linked = managedRules.some((r) => r.category === category)
              return (
                <div
                  key={category}
                  className={cn(
                    'rounded-lg border px-3 py-2.5 transition-colors',
                    logOnly ? 'border-warning/40 bg-warning/8' : 'border-line bg-elevated',
                  )}
                >
                  <div className="flex items-center justify-between gap-2">
                    <span className="flex min-w-0 items-center gap-1.5">
                      <span
                        className={cn(
                          'truncate text-sm font-medium',
                          logOnly ? 'text-fg' : 'text-fg-subtle',
                        )}
                      >
                        {t(`pages.waf.${category}`)}
                      </span>
                      {linked && (
                        <span
                          title={t('pages.protection.categories.linkageHint')}
                          aria-label={t('pages.protection.categories.linkageHint')}
                        >
                          <Info
                            weight="duotone"
                            className="h-3.5 w-3.5 shrink-0 text-fg-subtle"
                          />
                        </span>
                      )}
                    </span>
                    <Switch
                      size="sm"
                      checked={logOnly}
                      disabled={!canWrite || busy}
                      aria-label={t(`pages.waf.${category}`)}
                      onCheckedChange={(on) => toggleCategory(category, on)}
                    />
                  </div>
                  <div className="mt-1.5">
                    <Badge tone={logOnly ? 'warning' : 'danger'} dot>
                      {t(logOnly ? 'pages.protection.state.log' : 'pages.protection.state.block')}
                    </Badge>
                  </div>
                </div>
              )
            })}
          </div>
          <p className="mt-3 flex items-start gap-2 border-t border-line pt-3 text-xs leading-relaxed text-fg-subtle">
            <ShieldWarning weight="duotone" className="mt-0.5 h-3.5 w-3.5 shrink-0" />
            {t('pages.protection.monitorExplainer')}
          </p>
        </CardBody>
      </Card>

      {/* ── Built-in managed rules ────────────────────────────────────── */}
      <Card>
        <CardHeader
          title={t('pages.waf.managedRules')}
          description={t('pages.protection.managedRules.cardHint')}
          action={
            canWrite ? (
              <div className="flex items-center gap-2">
                <Button
                  size="sm"
                  variant="secondary"
                  disabled={busy}
                  onClick={() => batchAll(true)}
                >
                  {t('pages.protection.managedRules.batchLogAll')}
                </Button>
                <Button
                  size="sm"
                  variant="secondary"
                  disabled={busy}
                  onClick={() => batchAll(false)}
                >
                  {t('pages.protection.managedRules.batchBlockAll')}
                </Button>
              </div>
            ) : undefined
          }
        />
        <CardBody className="flex flex-col gap-4">
          {familyGroups.map((group) => (
            <ManagedRuleSection
              key={group.category}
              title={t(`pages.waf.${group.category}`)}
              rules={group.rules}
              monitoredRules={monitoredRules}
              highlightId={highlightRule}
              canWrite={canWrite}
              busy={busy}
              onToggle={toggleManagedMonitor}
            />
          ))}
          <ManagedRuleSection
            title={t('pages.protection.managedRules.groupRecon')}
            rules={reconRules}
            monitoredRules={monitoredRules}
            highlightId={highlightRule}
            canWrite={canWrite}
            busy={busy}
            onToggle={toggleManagedMonitor}
          />
          <ManagedRuleSection
            title={t('pages.protection.managedRules.groupProtocol')}
            rules={protocolRules}
            monitoredRules={monitoredRules}
            highlightId={highlightRule}
            canWrite={canWrite}
            busy={busy}
            onToggle={toggleManagedMonitor}
          />
          <p className="border-t border-line pt-3 text-xs leading-relaxed text-fg-subtle">
            {t('pages.waf.managedExplainer')}
          </p>
        </CardBody>
      </Card>

      {/* ── Deep inspection ───────────────────────────────────────────── */}
      <Card className={cn(settings.advanced_mode && 'border-danger/35')}>
        <CardBody className="flex items-start justify-between gap-4">
          <span className="flex min-w-0 gap-3">
            <span
              className={cn(
                'flex h-10 w-10 shrink-0 items-center justify-center rounded-lg',
                settings.advanced_mode
                  ? 'bg-danger/15 text-fg-danger'
                  : 'bg-recessed text-fg-subtle',
              )}
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
            checked={settings.advanced_mode}
            disabled={!canWrite || busy}
            aria-label={t('pages.protection.deepInspection')}
            onCheckedChange={(advanced_mode) => updateSettings.mutate({ advanced_mode })}
          />
        </CardBody>
      </Card>

      {/* ── Backend stacks (collapsed by default) ─────────────────────── */}
      <StacksCard
        stacksOn={stacksOn}
        canWrite={canWrite}
        busy={busy}
        onToggle={toggleStack}
      />
    </div>
  )
}

/** One labelled group of managed-rule rows. */
function ManagedRuleSection({
  title,
  rules,
  monitoredRules,
  highlightId,
  canWrite,
  busy,
  onToggle,
}: {
  title: string
  rules: ManagedRule[]
  monitoredRules: string[]
  highlightId: string | null
  canWrite: boolean
  busy: boolean
  onToggle: (ruleId: string, on: boolean) => void
}) {
  const { t } = useTranslation()
  if (rules.length === 0) return null
  return (
    <section>
      <h3 className="mb-1.5 text-xs font-semibold uppercase tracking-wide text-fg-subtle">
        {title}
      </h3>
      <div className="grid grid-cols-1 gap-x-8 gap-y-0.5 lg:grid-cols-2">
        {rules.map((rule) => {
          const monitored = monitoredRules.includes(rule.id)
          return (
            <div
              key={rule.id}
              className={cn(
                'flex items-center justify-between gap-3 rounded-md px-1 py-1.5',
                highlightId === rule.id && 'bg-brand/10 ring-1 ring-brand/40',
              )}
            >
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-1.5">
                  <span
                    className={cn(
                      'truncate text-sm',
                      monitored ? 'text-fg' : 'text-fg-subtle',
                    )}
                  >
                    {rule.name}
                  </span>
                  <Badge tone={ACTION_TONE[rule.action] ?? 'neutral'}>
                    {t(`actions.${rule.action}`, rule.action)}
                  </Badge>
                  {rule.strict_only && (
                    <Badge tone="warning">{t('pages.waf.strictOnly')}</Badge>
                  )}
                </div>
                <div className="mt-0.5 flex items-center gap-2">
                  <span className="pw-mono text-xs text-fg-subtle">{rule.id}</span>
                  <span className="flex items-center gap-0.5" title={`${rule.severity}/5`}>
                    {[1, 2, 3, 4, 5].map((n) => (
                      <span
                        key={n}
                        className={cn(
                          'h-1 w-2.5 rounded-full',
                          n <= rule.severity
                            ? rule.severity >= 4
                              ? 'bg-danger'
                              : 'bg-brand'
                            : 'bg-fill',
                        )}
                      />
                    ))}
                  </span>
                  {rule.tags.slice(0, 2).map((tag) => (
                    <span
                      key={tag}
                      className="pw-mono rounded border border-line bg-recessed px-1.5 py-px text-[10px] text-fg-subtle"
                    >
                      {tag}
                    </span>
                  ))}
                </div>
              </div>
              <Switch
                size="sm"
                checked={monitored}
                disabled={!canWrite || busy}
                aria-label={`${t('pages.waf.managedMonitor')}: ${rule.name}`}
                onCheckedChange={(on) => onToggle(rule.id, on)}
              />
            </div>
          )
        })}
      </div>
    </section>
  )
}

function StacksCard({
  stacksOn,
  canWrite,
  busy,
  onToggle,
}: {
  stacksOn: string[]
  canWrite: boolean
  busy: boolean
  onToggle: (stack: string, on: boolean) => void
}) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  return (
    <Card>
      <CardHeader
        title={t('pages.protection.stacks.cardTitle')}
        description={t('pages.protection.stacks.cardHint')}
        action={
          <Button
            size="sm"
            variant="ghost"
            onClick={() => setOpen((v) => !v)}
            icon={
              <CaretDown
                weight="duotone"
                className={cn('h-3.5 w-3.5 transition-transform', open && 'rotate-180')}
              />
            }
          >
            {open ? t('common.hide') : t('common.show')}
          </Button>
        }
      />
      {open && (
        <CardBody className="flex flex-col gap-4">
          <div className="grid grid-cols-1 gap-x-8 gap-y-1 sm:grid-cols-2 lg:grid-cols-4">
            {WAF_STACKS.map((stack) => {
              const on = stacksOn.includes(stack)
              return (
                <div
                  key={stack}
                  className="flex items-center justify-between gap-3 rounded-md px-1 py-1.5"
                >
                  <span className="min-w-0">
                    <span
                      className={cn(
                        'block truncate text-sm',
                        on ? 'text-fg' : 'text-fg-subtle',
                      )}
                    >
                      {t(`pages.waf.stack_${stack}`)}
                    </span>
                  </span>
                  <Switch
                    size="sm"
                    checked={on}
                    disabled={!canWrite || busy}
                    aria-label={t(`pages.waf.stack_${stack}`)}
                    onCheckedChange={(enabled) => onToggle(stack, enabled)}
                  />
                </div>
              )
            })}
          </div>
          <p className="flex items-start gap-2 border-t border-line pt-3 text-xs leading-relaxed text-fg-subtle">
            <ShieldWarning weight="duotone" className="mt-0.5 h-3.5 w-3.5 shrink-0" />
            {t('pages.protection.monitorExplainer')}
          </p>
        </CardBody>
      )}
    </Card>
  )
}

export default ManagedRulesPanel
