import { describe, expect, it } from 'vitest'
import { formatSize } from '../format'

describe('formatSize', () => {
  it('renders whole bytes below 1024', () => {
    expect(formatSize(0)).toBe('0 B')
    expect(formatSize(1.4)).toBe('1 B')
    // Rounding 1023.6 up must not print "1024 B" — it promotes to KB.
    expect(formatSize(1023.6)).toBe('1.0 KB')
  })

  it('keeps at most one decimal place above 1024', () => {
    expect(formatSize(1024)).toBe('1.0 KB')
    expect(formatSize(1536)).toBe('1.5 KB')
    expect(formatSize(10 * 1024 * 1024)).toBe('10 MB')
  })

  it('renders a negative reading as unknown', () => {
    // Some counters use -1 for "not sampled"; that is not "-1 B".
    expect(formatSize(-1)).toBe('—')
  })
})
