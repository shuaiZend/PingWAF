import type { TFunction } from 'i18next'
import { PencilSimple, Trash } from '@phosphor-icons/react'
import { Badge } from '@/components/ui/Badge'
import { Button } from '@/components/ui/Button'
import { Switch } from '@/components/ui/Switch'
import { type Column } from '@/components/ui/Table'
import { isAllowBlockConflict } from '@/lib/ruleActionMode'
import { cn } from '@/lib/utils'
import { type Rule, type RuleGroup } from '@/api/types'

const ACTION_TONE: Record<string, 'danger' | 'warning' | 'info' | 'success' | 'neutral'> = {
  block: 'danger',
  challenge: 'warning',
  js_challenge: 'warning',
  log: 'info',
  allow: 'success',
}

const MODE_TONE: Record<string, 'danger' | 'warning' | 'neutral'> = {
  block: 'danger',
  monitor: 'warning',
  off: 'neutral',
}

interface RuleColumnsDeps {
  t: TFunction
  groups: RuleGroup[]
  canWrite: boolean
  toggleBusy: boolean
  onToggle: (rule: Rule, enabled: boolean) => void
  onEdit: (rule: Rule) => void
  onDelete: (rule: Rule) => void
}

/**
 * Column definitions for the protection page's rules table. Kept as a pure
 * factory so the layout contract (see `__tests__/customRulesColumns.test.tsx`)
 * can be asserted without mounting the panel.
 */
export function buildRuleColumns(deps: RuleColumnsDeps): Column<Rule>[] {
  const { t, groups, canWrite, toggleBusy, onToggle, onEdit, onDelete } = deps
  return [
    {
      key: 'priority',
      header: t('common.priority'),
      accessor: (r) => r.priority,
      sortable: true,
      align: 'right',
      width: '1%',
      cell: (r) => (
        <span className="pw-mono tabular-nums text-xs text-fg-subtle">{r.priority}</span>
      ),
    },
    {
      key: 'name',
      header: t('common.name'),
      accessor: (r) => r.name,
      sortable: true,
      // `max-w-0` + `w-full` lets the name column absorb the remaining width
      // so the truncated expression cannot push the table past the viewport.
      className: 'w-full max-w-0',
      cell: (r) => (
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <p className="truncate text-[13px] font-medium text-fg-strong">{r.name}</p>
            {isAllowBlockConflict(r.action, r.mode) && (
              <Badge tone="warning" size="sm" dot>
                {t('pages.waf.allowBlockConflictBadge')}
              </Badge>
            )}
            {r.group_id && (
              <span className="shrink-0 rounded border border-line px-1.5 py-px text-[10px] text-fg-subtle">
                {groups.find((g) => g.id === r.group_id)?.name ?? t('pages.waf.noGroup')}
              </span>
            )}
          </div>
          <p className="pw-mono truncate text-xs text-fg-subtle" title={r.expression}>
            {r.expression}
          </p>
        </div>
      ),
    },
    {
      key: 'action',
      header: t('pages.waf.action'),
      accessor: (r) => r.action,
      cell: (r) => (
        <Badge tone={ACTION_TONE[r.action] ?? 'neutral'}>
          {t(`actions.${r.action}`, r.action)}
        </Badge>
      ),
    },
    {
      key: 'mode',
      header: t('pages.waf.mode'),
      accessor: (r) => r.mode,
      cell: (r) => (
        <Badge
          tone={MODE_TONE[r.mode] ?? 'neutral'}
          dot
          title={
            r.mode === 'monitor' && r.action === 'block'
              ? t('pages.waf.modeMonitorHint')
              : undefined
          }
        >
          {t(`pages.waf.mode_${r.mode}`, r.mode)}
        </Badge>
      ),
    },
    {
      key: 'severity',
      header: t('pages.waf.severity'),
      accessor: (r) => r.severity,
      sortable: true,
      align: 'center',
      width: '1%',
      cell: (r) => (
        <div className="flex items-center justify-center gap-0.5" title={`${r.severity}/5`}>
          {[1, 2, 3, 4, 5].map((n) => (
            <span
              key={n}
              className={cn(
                'h-1.5 w-3 rounded-full',
                n <= r.severity
                  ? r.severity >= 4
                    ? 'bg-danger'
                    : 'bg-brand'
                  : 'bg-fill',
              )}
            />
          ))}
        </div>
      ),
    },
    {
      key: 'tags',
      header: t('pages.waf.tags'),
      width: '1%',
      cell: (r) => (
        <div className="flex max-w-[180px] flex-wrap gap-1">
          {(r.tags ?? []).slice(0, 3).map((tag) => (
            <span
              key={tag}
              className="pw-mono rounded border border-line bg-recessed px-1.5 py-px text-[10px] text-fg-subtle"
            >
              {tag}
            </span>
          ))}
          {(r.tags ?? []).length > 3 && (
            <span className="text-[10px] text-fg-subtle">+{r.tags.length - 3}</span>
          )}
        </div>
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
          disabled={!canWrite || toggleBusy}
          aria-label={`${t('common.enabled')}: ${r.name}`}
          onCheckedChange={(enabled) => onToggle(r, enabled)}
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
            onClick={() => onEdit(r)}
            icon={<PencilSimple weight="duotone" className="h-4 w-4" />}
          />
          <Button
            size="icon"
            variant="ghost"
            className="hover:text-fg-danger"
            aria-label={t('common.delete')}
            disabled={!canWrite}
            onClick={() => onDelete(r)}
            icon={<Trash weight="duotone" className="h-4 w-4" />}
          />
        </div>
      ),
    },
  ]
}
