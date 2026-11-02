import { describe, expect, it } from 'vitest'

import { parseTab } from '../protectionTab'

describe('parseTab', () => {
  it('keeps the path-exceptions tab', () => {
    expect(parseTab('paths')).toBe('paths')
  })

  it('keeps the current tabs', () => {
    expect(parseTab('managed')).toBe('managed')
    expect(parseTab('custom')).toBe('custom')
    expect(parseTab('cc')).toBe('cc')
  })

  it('normalizes legacy tab names', () => {
    expect(parseTab('settings')).toBe('managed')
    expect(parseTab('rules')).toBe('custom')
  })

  it('falls back to managed for unknown or missing values', () => {
    expect(parseTab('nope')).toBe('managed')
    expect(parseTab(null)).toBe('managed')
  })
})
