import { describe, expect, it } from 'vitest'
import {
  ALLOW_FALLBACK_MODE,
  isAllowBlockConflict,
  isModeAllowedForAction,
} from '../ruleActionMode'
import { groupCertsByRelation } from '../certRelation'

describe('isAllowBlockConflict', () => {
  it('flags allow + block pairs the server rejects', () => {
    expect(isAllowBlockConflict('allow', 'block')).toBe(true)
    expect(isAllowBlockConflict('allow', 'monitor')).toBe(false)
    expect(isAllowBlockConflict('block', 'block')).toBe(false)
    expect(isAllowBlockConflict('log', 'block')).toBe(false)
  })
})

describe('isModeAllowedForAction', () => {
  it('disables only block for allow rules', () => {
    expect(isModeAllowedForAction('allow', 'block')).toBe(false)
    expect(isModeAllowedForAction('allow', 'monitor')).toBe(true)
    expect(isModeAllowedForAction('allow', 'off')).toBe(true)
    expect(isModeAllowedForAction('challenge', 'block')).toBe(true)
  })
})

describe('ALLOW_FALLBACK_MODE', () => {
  it('falls back to monitor, not off', () => {
    expect(ALLOW_FALLBACK_MODE).toBe('monitor')
    expect(isAllowBlockConflict('allow', ALLOW_FALLBACK_MODE)).toBe(false)
  })
})

describe('groupCertsByRelation', () => {
  const cert = (
    id: string,
    relation?: 'owner' | 'linked' | 'available',
  ) => ({
    id,
    site_id: 's1',
    domain: `${id}.example.com`,
    issuer: null,
    not_before: null,
    expires_at: null,
    auto_renew: false,
    acme_email: null,
    acme_challenge_type: 'http-01',
    acme_dns_provider: null,
    acme_dns_config: null,
    status: 'active',
    failure_reason: null,
    has_certificate: true,
    has_private_key: true,
    created_at: '',
    updated_at: '',
    ...(relation ? { relation } : {}),
  })

  it('splits owner and linked rows by relation', () => {
    const grouped = groupCertsByRelation([
      cert('a', 'owner'),
      cert('b', 'linked'),
      cert('c', 'owner'),
    ])
    expect(grouped.owner.map((c) => c.id)).toEqual(['a', 'c'])
    expect(grouped.linked.map((c) => c.id)).toEqual(['b'])
  })

  it('treats rows without the field (old responses) as owner', () => {
    const grouped = groupCertsByRelation([cert('legacy')])
    expect(grouped.owner.map((c) => c.id)).toEqual(['legacy'])
    expect(grouped.linked).toEqual([])
  })

  it('returns two empty groups for an empty list', () => {
    expect(groupCertsByRelation([])).toEqual({ owner: [], linked: [] })
  })
})
