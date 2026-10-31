import type { CacheRule, CacheTypePolicy } from '@/api/types'

/**
 * The five fixed preset families, in server evaluation order
 * (`CACHE_TYPE_POLICY_CATEGORIES`).
 */
export const PRESET_CATEGORY_ORDER = [
  'web_assets',
  'image',
  'media',
  'other',
  'html',
] as const satisfies readonly (keyof CacheTypePolicy)[]

/** Whether a row is an explicit cache rule or a category preset. */
export type CacheRowKind = 'rule' | 'preset'

/** A merged, order-numbered cache row shown in the unified list. */
export interface CacheRow {
  kind: CacheRowKind
  /** 1-based execution order across the merged list. */
  order: number
  /** Stable row key: the rule id, or the category name for presets. */
  key: string
  name: string
  /** Explicit rules carry their expression; presets are described by extensions. */
  matchExpression: string | null
  extensions: string[]
  edgeTtlSeconds: number
  browserTtlSeconds: number
  cacheEligible: boolean
  enabled: boolean
}

/**
 * Merges explicit cache rules (kept in backend order — they evaluate first)
 * with the category presets (fixed order, shown after) into one ordered list.
 */
export function mergeCacheRows(
  rules: CacheRule[],
  policy: CacheTypePolicy | null | undefined,
  categoryOrder: readonly (keyof CacheTypePolicy)[] = PRESET_CATEGORY_ORDER,
): CacheRow[] {
  const ruleRows: CacheRow[] = rules.map((rule, index) => ({
    kind: 'rule' as const,
    order: index + 1,
    key: rule.id,
    name: rule.name,
    matchExpression: rule.match_expression,
    extensions: [],
    edgeTtlSeconds: rule.edge_ttl_seconds,
    browserTtlSeconds: rule.browser_ttl_seconds,
    cacheEligible: rule.cache_eligible,
    enabled: rule.enabled,
  }))

  const presetRows: CacheRow[] = []
  let order = ruleRows.length
  for (const category of categoryOrder) {
    const family = policy?.[category]
    if (!family) continue
    presetRows.push({
      kind: 'preset',
      order: ++order,
      key: category,
      name: category,
      matchExpression: null,
      extensions: family.extensions,
      edgeTtlSeconds: family.edge_ttl_seconds,
      browserTtlSeconds: family.browser_ttl_seconds,
      cacheEligible: true,
      enabled: family.enabled,
    })
  }
  return [...ruleRows, ...presetRows]
}

/** Only these are accepted as a file extension (mirrors the editor validation). */
const EXTENSION_PATTERN = /^[a-z0-9]{1,16}$/

/**
 * Builds the explicit-rule expression matching a preset's extensions, e.g.
 * `http.request.uri.path matches "(?i)\.(?:png|jpg)$"`.
 * Extensions are trimmed, lower-cased, deduplicated and regex-escaped;
 * anything outside `EXTENSION_PATTERN` is dropped. Returns null when no
 * valid extension survives.
 */
export function presetToExpression(extensions: string[]): string | null {
  const seen = new Set<string>()
  for (const raw of extensions) {
    const extension = raw.trim().toLowerCase()
    if (!EXTENSION_PATTERN.test(extension)) continue
    seen.add(extension.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))
  }
  if (seen.size === 0) return null
  return `http.request.uri.path matches "(?i)\\.(?:${[...seen].join('|')})$"`
}
