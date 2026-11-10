import { describe, expect, it } from 'vitest'
import { applyRuleToPage, buildRulePayload } from '../ruleCache'
import type { RuleFormState } from '../ruleCache'
import type { Page, Rule } from '@/api/types'

function makePage(items: Rule[]): Page<Rule> {
  return { items, total: items.length, page: 1, page_size: 200 }
}

function makeRule(overrides: Partial<Rule> = {}): Rule {
  return {
    id: 'r1',
    managed_rule_id: null,
    group_id: null,
    site_id: 's1',
    name: 'Rule 1',
    description: null,
    expression: 'ip.src eq 1.2.3.4',
    action: 'block',
    severity: 3,
    tags: ['bot'],
    enabled: false,
    mode: 'monitor',
    priority: 100,
    created_at: '',
    updated_at: '',
    ...overrides,
  }
}

function makeForm(overrides: Partial<RuleFormState> = {}): RuleFormState {
  return {
    name: 'Renamed',
    description: 'desc',
    groupId: 'g1',
    action: 'block',
    severity: 5,
    mode: 'block',
    priority: 7,
    tags: ['xss'],
    enabled: true,
    ...overrides,
  }
}

describe('applyRuleToPage', () => {
  it('replaces the matching row and keeps pagination fields intact', () => {
    const a = makeRule({ id: 'a' })
    const b = makeRule({ id: 'b', enabled: true })
    const saved = { ...b, enabled: false }
    const page = makePage([a, b])
    const next = applyRuleToPage(page, saved)
    expect(next?.items).toEqual([a, saved])
    expect(next?.items).toHaveLength(2)
    expect(next?.total).toBe(2)
    expect(next?.page).toBe(1)
    expect(next?.page_size).toBe(200)
  })

  it('returns the page untouched when the rule is not in it', () => {
    const page = makePage([makeRule({ id: 'a' })])
    const saved = makeRule({ id: 'zzz', enabled: true })
    const next = applyRuleToPage(page, saved)
    expect(next).toEqual(page)
  })

  it('returns undefined for a missing cache entry', () => {
    expect(applyRuleToPage(undefined, makeRule())).toBeUndefined()
  })
})

describe('buildRulePayload', () => {
  it('sends only enabled/mode for a managed-rule override row', () => {
    const rule = makeRule({ managed_rule_id: 'builtin-owasp-01' })
    const payload = buildRulePayload(rule, makeForm(), 'ip.src eq 1.2.3.4')
    expect(Object.keys(payload).sort()).toEqual(['enabled', 'mode'])
    expect(payload).toEqual({ enabled: true, mode: 'block' })
  })

  it('still sends the full field set for custom rules (regression)', () => {
    const form = makeForm()
    const payload = buildRulePayload(null, form, 'ip.src eq 1.2.3.4')
    expect(payload).toEqual({
      name: 'Renamed',
      group_id: 'g1',
      description: 'desc',
      expression: 'ip.src eq 1.2.3.4',
      action: 'block',
      severity: 5,
      tags: ['xss'],
      enabled: true,
      mode: 'block',
      priority: 7,
    })
  })
})
