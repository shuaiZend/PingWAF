import { describe, expect, it } from 'vitest'
import {
  UNMATCHED_HOST_STATUS_CODES,
  asErrorPageScope,
  defaultStatusCodeForScope,
  filterErrorPagesByScope,
  statusCodeAllowedForScope,
  statusCodesForScope,
} from './errorPageScope'

interface Row {
  id: string
  status_code: number
  scope?: string
}

const row = (id: string, status_code: number, scope?: string): Row => ({
  id,
  status_code,
  scope,
})

describe('asErrorPageScope', () => {
  it('defaults unknown/missing values to site', () => {
    expect(asErrorPageScope(undefined)).toBe('site')
    expect(asErrorPageScope('site')).toBe('site')
    expect(asErrorPageScope('bogus')).toBe('site')
  })

  it('accepts unmatched_host', () => {
    expect(asErrorPageScope('unmatched_host')).toBe('unmatched_host')
  })
})

describe('filterErrorPagesByScope', () => {
  const items = [
    row('a', 403, 'site'),
    row('b', 404, 'unmatched_host'),
    row('c', 503, 'unmatched_host'),
    row('d', 429), // legacy row without scope → site
  ]

  it('keeps only site rows (including legacy rows without scope)', () => {
    expect(filterErrorPagesByScope(items, 'site').map((r) => r.id)).toEqual(['a', 'd'])
  })

  it('keeps only unmatched_host rows', () => {
    expect(filterErrorPagesByScope(items, 'unmatched_host').map((r) => r.id)).toEqual([
      'b',
      'c',
    ])
  })
})

describe('unmatched_host status-code restriction', () => {
  it('allows only 404 / 421 / 503', () => {
    expect(UNMATCHED_HOST_STATUS_CODES).toEqual([404, 421, 503])
    for (const code of UNMATCHED_HOST_STATUS_CODES) {
      expect(statusCodeAllowedForScope('unmatched_host', code)).toBe(true)
    }
    for (const code of [403, 429, 502, 504]) {
      expect(statusCodeAllowedForScope('unmatched_host', code)).toBe(false)
    }
  })

  it('keeps the original range for site', () => {
    for (const code of [403, 429, 502, 503, 504]) {
      expect(statusCodeAllowedForScope('site', code)).toBe(true)
    }
  })

  it('offers a separate card grid per scope', () => {
    expect(statusCodesForScope('site')).toEqual([403, 429, 502, 503, 504])
    expect(statusCodesForScope('unmatched_host')).toEqual([404, 421, 503])
  })

  it('falls back to 404 when switching scope makes the current code invalid', () => {
    expect(defaultStatusCodeForScope('unmatched_host', 403)).toBe(404)
    expect(defaultStatusCodeForScope('unmatched_host', 421)).toBe(421)
    expect(defaultStatusCodeForScope('site', 503)).toBe(503)
  })
})
