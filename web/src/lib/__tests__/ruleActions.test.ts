import { describe, expect, it } from 'vitest'
import { IP_RULE_ACTIONS, RULE_ACTIONS } from '@/api/types'

describe('challenge action options', () => {
  it('exposes one server-verified challenge option', () => {
    expect(RULE_ACTIONS).toContain('challenge')
    expect(RULE_ACTIONS).not.toContain('js_challenge')
    expect(IP_RULE_ACTIONS).toContain('challenge')
    expect(IP_RULE_ACTIONS).not.toContain('js_challenge')
  })
})
