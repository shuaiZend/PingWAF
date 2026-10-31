import { describe, expect, it } from 'vitest'
import {
  bypassesAllLayers,
  toggleSkipLayer,
  validatePathExceptionPath,
} from '../pathException'

describe('validatePathExceptionPath', () => {
  it('rejects empty paths', () => {
    expect(validatePathExceptionPath('', 'prefix')).toBe(
      'pages.pathExceptions.errorPathRequired',
    )
    expect(validatePathExceptionPath('   ', 'exact')).toBe(
      'pages.pathExceptions.errorPathRequired',
    )
  })

  it('requires rooted paths', () => {
    expect(validatePathExceptionPath('api/health', 'prefix')).toBe(
      'pages.pathExceptions.errorPathRooted',
    )
  })

  it('requires a trailing * for wildcard matching', () => {
    expect(validatePathExceptionPath('/assets', 'wildcard')).toBe(
      'pages.pathExceptions.errorPathWildcardSuffix',
    )
    expect(validatePathExceptionPath('/assets/*', 'wildcard')).toBeNull()
  })

  it('accepts rooted prefix and exact paths', () => {
    expect(validatePathExceptionPath('/api', 'prefix')).toBeNull()
    expect(validatePathExceptionPath('/healthz', 'exact')).toBeNull()
  })
})

describe('toggleSkipLayer', () => {
  it('adds a layer without duplicates', () => {
    expect(toggleSkipLayer(['waf'], 'bot', true)).toEqual(['waf', 'bot'])
    expect(toggleSkipLayer(['waf', 'bot'], 'bot', true)).toEqual(['waf', 'bot'])
  })

  it('removes a layer', () => {
    expect(toggleSkipLayer(['waf', 'bot'], 'bot', false)).toEqual(['waf'])
  })

  it('expands * into all concrete layers minus the removed one', () => {
    const result = toggleSkipLayer(['*'], 'bot', false)
    expect(result).not.toContain('bot')
    expect(result).not.toContain('*')
    expect(result).toContain('waf')
    expect(result).toContain('challenge')
  })

  it('adding a layer while * is present is a no-op', () => {
    expect(toggleSkipLayer(['*'], 'bot', true)).toEqual(['*'])
  })
})

describe('bypassesAllLayers', () => {
  it('detects the * wildcard', () => {
    expect(bypassesAllLayers(['*'])).toBe(true)
    expect(bypassesAllLayers(['waf'])).toBe(false)
    expect(bypassesAllLayers([])).toBe(false)
  })
})
