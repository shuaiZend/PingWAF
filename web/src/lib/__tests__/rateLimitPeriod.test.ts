import { describe, expect, it } from 'vitest'
import { formatPeriodSeconds, isValidPeriodSeconds, DEFAULT_PERIOD_UNITS } from '../rateLimitPeriod'

describe('isValidPeriodSeconds', () => {
  it('accepts integers inside 1..86400', () => {
    expect(isValidPeriodSeconds(1)).toBe(true)
    expect(isValidPeriodSeconds(60)).toBe(true)
    expect(isValidPeriodSeconds(86400)).toBe(true)
  })

  it('rejects out-of-range, fractional and non-number values', () => {
    expect(isValidPeriodSeconds(0)).toBe(false)
    expect(isValidPeriodSeconds(-1)).toBe(false)
    expect(isValidPeriodSeconds(86401)).toBe(false)
    expect(isValidPeriodSeconds(1.5)).toBe(false)
    expect(isValidPeriodSeconds(NaN)).toBe(false)
    expect(isValidPeriodSeconds('60')).toBe(false)
    expect(isValidPeriodSeconds(null)).toBe(false)
  })
})

describe('formatPeriodSeconds', () => {
  it('formats seconds only', () => {
    expect(formatPeriodSeconds(45)).toBe('45 秒')
    expect(formatPeriodSeconds(1)).toBe('1 秒')
  })

  it('formats mixed minutes and seconds', () => {
    expect(formatPeriodSeconds(90)).toBe('1 分 30 秒')
    expect(formatPeriodSeconds(60)).toBe('1 分')
  })

  it('formats hours, folding leftover minutes and seconds', () => {
    expect(formatPeriodSeconds(3600)).toBe('1 小时')
    expect(formatPeriodSeconds(86400)).toBe('24 小时')
    expect(formatPeriodSeconds(3661)).toBe('1 小时 1 分 1 秒')
  })

  it('supports custom unit labels', () => {
    expect(
      formatPeriodSeconds(90, { second: 's', minute: 'm', hour: 'h' }),
    ).toBe('1 m 30 s')
  })

  it('returns null for invalid values', () => {
    expect(formatPeriodSeconds(0)).toBeNull()
    expect(formatPeriodSeconds(86401)).toBeNull()
    expect(formatPeriodSeconds(NaN)).toBeNull()
    expect(formatPeriodSeconds(1.5)).toBeNull()
  })

  it('default units stay Chinese', () => {
    expect(DEFAULT_PERIOD_UNITS.second).toBe('秒')
  })
})
