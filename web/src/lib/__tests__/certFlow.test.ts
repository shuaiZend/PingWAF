import { describe, expect, it } from 'vitest'
import {
  bindingKind,
  deleteBlockers,
  mergeBindResults,
  type CertUsage,
} from '../certFlow'

describe('bindingKind', () => {
  it('returns owner when usage.owner matches the site', () => {
    const usage: CertUsage = { owner: { id: 's1' }, linked_sites: [] }
    expect(bindingKind({ site_id: null }, usage, 's1')).toBe('owner')
  })

  it('returns owner when only cert.site_id matches (usage fallback)', () => {
    expect(bindingKind({ site_id: 's1' }, undefined, 's1')).toBe('owner')
  })

  it('owner takes priority over linked_sites containing the same site', () => {
    const usage: CertUsage = {
      owner: { id: 's1' },
      linked_sites: [{ id: 's1' }],
    }
    expect(
      bindingKind({ site_id: null }, usage, 's1', ['s1']),
    ).toBe('owner')
  })

  it('returns bound when usage.linked_sites contains the site', () => {
    const usage: CertUsage = { linked_sites: [{ id: 's2' }] }
    expect(bindingKind({ site_id: null }, usage, 's2')).toBe('bound')
  })

  it('returns bound from the inlined bound_sites fallback when usage is missing', () => {
    expect(
      bindingKind({ site_id: null }, undefined, 's3', ['s3', 's4']),
    ).toBe('bound')
  })

  it('does not treat the owner site as bound via the inlined fallback', () => {
    // usage 缺失：属主行不能被回退信号升级成「已绑定」（否则会渲染解绑按钮）。
    expect(
      bindingKind({ site_id: 's1' }, undefined, 's1', ['s1']),
    ).toBe('owner')
  })

  it('returns bindable when the site matches nothing', () => {
    const usage: CertUsage = { owner: { id: 's1' }, linked_sites: [{ id: 's2' }] }
    expect(bindingKind({ site_id: 's1' }, usage, 's9', ['s2'])).toBe('bindable')
  })
})

describe('mergeBindResults', () => {
  it('returns empty arrays for an empty input', () => {
    expect(mergeBindResults([])).toEqual({ ok: [], failed: [] })
  })

  it('collects all successes in order', () => {
    expect(
      mergeBindResults([
        { siteId: 'a', ok: true },
        { siteId: 'b', ok: true },
      ]),
    ).toEqual({ ok: ['a', 'b'], failed: [] })
  })

  it('keeps input order and pairs each failure with its reason', () => {
    const merged = mergeBindResults([
      { siteId: 'a', ok: false, error: 'conflict' },
      { siteId: 'b', ok: true },
      { siteId: 'c', ok: false, error: 'timeout' },
      { siteId: 'd', ok: true },
    ])
    expect(merged.ok).toEqual(['b', 'd'])
    expect(merged.failed).toEqual([
      { siteId: 'a', reason: 'conflict' },
      { siteId: 'c', reason: 'timeout' },
    ])
  })

  it('leaves the reason empty when a failure has no error text (page renders the i18n fallback)', () => {
    expect(mergeBindResults([{ siteId: 'x', ok: false }]).failed).toEqual([
      { siteId: 'x', reason: '' },
    ])
  })
})

describe('deleteBlockers', () => {
  it('blocks with serving sites taking priority over linked ones', () => {
    const usage: CertUsage = {
      serving_sites: [{ id: 'sv1', domain: 'a.com' }],
      linked_sites: [{ id: 'lk1', domain: 'b.com' }],
    }
    expect(
      deleteBlockers({ site_id: null }, usage),
    ).toEqual({
      blocked: true,
      sites: [{ id: 'sv1', domain: 'a.com' }],
      reason: 'serving',
    })
  })

  it('blocks with linked sites when nothing is serving', () => {
    const usage: CertUsage = { linked_sites: [{ id: 'lk1', domain: 'b.com' }] }
    expect(deleteBlockers({ site_id: null }, usage)).toEqual({
      blocked: true,
      sites: [{ id: 'lk1', domain: 'b.com' }],
      reason: 'linked',
    })
  })

  it('blocks with the inlined bound_sites when usage has no entries', () => {
    expect(
      deleteBlockers(
        { site_id: null, bound_sites: [{ id: 's1', domain: 'c.com' }] },
        { linked_sites: [], serving_sites: [] },
      ),
    ).toEqual({
      blocked: true,
      sites: [{ id: 's1', domain: 'c.com' }],
      reason: 'linked',
    })
  })

  it('is conservatively blocked (unknown) when usage is missing and no inline signal exists', () => {
    expect(deleteBlockers({ site_id: null }, undefined)).toEqual({
      blocked: true,
      sites: [],
      reason: 'unknown',
    })
  })

  it('does not block when usage resolved and nothing is bound or serving', () => {
    expect(
      deleteBlockers({ site_id: null, bound_sites: [] }, { linked_sites: [] }),
    ).toEqual({ blocked: false, sites: [], reason: null })
  })

  it('dedupes blocking sites by id and falls back to the id as domain', () => {
    const usage: CertUsage = {
      serving_sites: [
        { id: 'sv1', domain: 'a.com' },
        { id: 'sv1', domain: 'a.com' },
        { id: 'sv2' },
      ],
    }
    const result = deleteBlockers({ site_id: null }, usage)
    expect(result.sites).toEqual([
      { id: 'sv1', domain: 'a.com' },
      { id: 'sv2', domain: 'sv2' },
    ])
  })
})
