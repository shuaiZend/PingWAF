import { describe, expect, it } from 'vitest'
import type { CacheRule, CacheTypePolicy } from '@/api/types'
import { mergeCacheRows, presetToExpression } from '@/lib/cachePolicy'

const rule = (overrides: Partial<CacheRule>): CacheRule => ({
  id: 'r1',
  site_id: 's1',
  name: 'rule',
  match_expression: '',
  edge_ttl_seconds: 3600,
  browser_ttl_seconds: 300,
  cache_eligible: true,
  respect_origin: true,
  enabled: true,
  created_at: '2024-01-01T00:00:00Z',
  ...overrides,
})

const family = (enabled: boolean, extensions: string[]) => ({
  enabled,
  edge_ttl_seconds: 600,
  browser_ttl_seconds: 60,
  extensions,
})

const policy: CacheTypePolicy = {
  web_assets: family(true, ['css', 'js']),
  image: family(false, ['png']),
  media: family(true, ['mp4']),
  other: family(false, []),
  html: family(true, ['html']),
}

describe('mergeCacheRows', () => {
  it('puts explicit rules first, in backend order, then presets', () => {
    const rows = mergeCacheRows(
      [rule({ id: 'b', name: 'zulu' }), rule({ id: 'a', name: 'alpha' })],
      policy,
    )
    expect(rows.map((r) => [r.kind, r.key])).toEqual([
      ['rule', 'b'],
      ['rule', 'a'],
      ['preset', 'web_assets'],
      ['preset', 'image'],
      ['preset', 'media'],
      ['preset', 'other'],
      ['preset', 'html'],
    ])
    expect(rows.map((r) => r.order)).toEqual([1, 2, 3, 4, 5, 6, 7])
  })

  it('keeps disabled presets in the list', () => {
    const rows = mergeCacheRows([], policy)
    const image = rows.find((r) => r.key === 'image')
    expect(image?.kind).toBe('preset')
    expect(image?.enabled).toBe(false)
  })

  it('shows presets with no extensions as disabled', () => {
    const rows = mergeCacheRows([], policy)
    const other = rows.find((r) => r.key === 'other')
    expect(other?.enabled).toBe(false)
    expect(other?.extensions).toEqual([])
  })

  it('carries rule fields through', () => {
    const rows = mergeCacheRows(
      [rule({ enabled: false, edge_ttl_seconds: 10, browser_ttl_seconds: 5 })],
      policy,
    )
    expect(rows[0]).toMatchObject({
      kind: 'rule',
      enabled: false,
      edgeTtlSeconds: 10,
      browserTtlSeconds: 5,
    })
  })

  it('skips preset families missing from the policy', () => {
    const rows = mergeCacheRows([], null)
    expect(rows).toEqual([])
  })
})

describe('presetToExpression', () => {
  it('builds a case-insensitive suffix match over the extensions', () => {
    expect(presetToExpression(['png', 'jpg'])).toBe(
      'http.request.uri.path matches "(?i)\\.(?:png|jpg)$"',
    )
  })

  it('filters out invalid extensions and dedupes', () => {
    expect(presetToExpression(['PNG', ' png ', 'a.b', '', 'x!', 'png'])).toBe(
      'http.request.uri.path matches "(?i)\\.(?:png)$"',
    )
  })

  it('rejects an extension that is too long', () => {
    expect(presetToExpression(['a'.repeat(17)])).toBeNull()
  })

  it('returns null for empty or all-invalid input', () => {
    expect(presetToExpression([])).toBeNull()
    expect(presetToExpression(['', '!!'])).toBeNull()
  })
})
