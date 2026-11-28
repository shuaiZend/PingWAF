import { describe, expect, it } from 'vitest'
import {
  parseResponseStatusCodes,
  formatResponseStatusCodes,
} from '../rateLimitStatus'

describe('parseResponseStatusCodes', () => {
  it('accepts individual status codes', () => {
    expect(parseResponseStatusCodes('404')).toEqual([404])
  })

  it('sorts and removes duplicates', () => {
    expect(
      parseResponseStatusCodes('404, 400, 403, 404'),
    ).toEqual([400, 403, 404])
  })

  it('expands ranges', () => {
    expect(
      parseResponseStatusCodes('400-403'),
    ).toEqual([400, 401, 402, 403])
  })

  it('expands status classes', () => {
    const codes = parseResponseStatusCodes('4xx')

    expect(codes).toHaveLength(100)
    expect(codes?.[0]).toBe(400)
    expect(codes?.[99]).toBe(499)
  })

  it('combines ranges, classes and individual codes', () => {
    expect(
      parseResponseStatusCodes('4XX, 404, 500-502'),
    ).toHaveLength(103)
  })

  it('accepts valid boundaries', () => {
    expect(
      parseResponseStatusCodes('200, 599'),
    ).toEqual([200, 599])
  })

  it('rejects invalid input', () => {
    for (const input of [
      '199',
      '600',
      'abc',
      '404.5',
      '500-400',
      '100-400',
      '500-999',
      '6xx',
      '404,',
    ]) {
      expect(parseResponseStatusCodes(input)).toBeNull()
    }
  })

  it('handles empty input', () => {
    expect(parseResponseStatusCodes('')).toEqual([])
  })
})

describe('formatResponseStatusCodes', () => {
  it('formats individual codes and consecutive ranges', () => {
    expect(
      formatResponseStatusCodes([404, 400, 401, 403, 404]),
    ).toBe('400-401, 403-404')
  })

  it('compresses a complete status class', () => {
    const codes = parseResponseStatusCodes('4xx') ?? []

    expect(formatResponseStatusCodes(codes)).toBe('4xx')
  })

  it('preserves values through a formatting round trip', () => {
    const codes =
      parseResponseStatusCodes('4xx, 500, 502-504') ?? []

    expect(
      parseResponseStatusCodes(
        formatResponseStatusCodes(codes),
      ),
    ).toEqual(codes)
  })
})
