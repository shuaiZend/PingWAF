import type {
  CreateRuleRequest,
  Page,
  Rule,
  UpdateRuleRequest,
} from '@/api/types'

/**
 * The editable half of a rule, as held by the rule dialog's form state. It
 * lives here (not in the dialog) so these helpers stay a leaf module: a
 * component may import `lib/`, never the other way round.
 */
export interface RuleFormState {
  name: string
  description: string
  groupId: string
  action: string
  severity: number
  mode: string
  priority: number
  tags: string[]
  enabled: boolean
}

/**
 * D1: the rules query caches a `Page<Rule>` (the raw `queryFn` result), while
 * the component consumes `select: (page) => page.items`. Optimistic cache
 * writes must therefore go through this helper — writing a bare `Rule[]`
 * would make `select` see `page.items === undefined`.
 *
 * Replaces the matching rule in place; pagination fields are untouched.
 * Returns `undefined` when the cache entry is missing so React Query keeps
 * treating it as "no data" rather than materialising an empty page.
 */
export function applyRuleToPage(
  page: Page<Rule> | undefined,
  saved: Rule,
): Page<Rule> | undefined {
  if (!page) return undefined
  return {
    ...page,
    items: page.items.map((r) => (r.id === saved.id ? saved : r)),
  }
}

/**
 * D2: rows that override a built-in managed rule (`managed_rule_id` set) only
 * accept `enabled`/`mode` changes on the server; sending any other field is a
 * 400. Build the smallest payload the dialog may submit.
 */
export function buildRulePayload(
  rule: Rule | null,
  form: RuleFormState,
  expression: string,
): CreateRuleRequest | UpdateRuleRequest {
  if (rule?.managed_rule_id) {
    return { enabled: form.enabled, mode: form.mode }
  }
  return {
    name: form.name.trim(),
    group_id: form.groupId || null,
    description: form.description.trim() || null,
    expression,
    action: form.action,
    severity: form.severity,
    tags: form.tags,
    enabled: form.enabled,
    mode: form.mode,
    priority: Number.isFinite(form.priority) ? form.priority : 0,
  }
}
