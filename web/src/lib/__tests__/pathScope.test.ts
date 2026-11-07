import { describe, expect, it } from 'vitest'
import {
  normalizePathScope,
  pathScopeFrom,
  validatePathScope,
} from '../pathScope'

describe('normalizePathScope', () => {
  it('collapses an empty path to all-paths with the server default match type', () => {
    expect(normalizePathScope('', 'regex')).toEqual({ path: '', match_type: 'prefix' })
    expect(normalizePathScope('   ', 'wildcard')).toEqual({
      path: '',
      match_type: 'prefix',
    })
  })

  it('trims a filled path and keeps the chosen match type', () => {
    expect(normalizePathScope(' /api/v1 ', 'exact')).toEqual({
      path: '/api/v1',
      match_type: 'exact',
    })
  })

  it('falls back to prefix when the match type is empty', () => {
    expect(normalizePathScope('/api', '')).toEqual({ path: '/api', match_type: 'prefix' })
  })
})

describe('pathScopeFrom', () => {
  it('treats missing response fields as all paths', () => {
    expect(pathScopeFrom(undefined, undefined)).toEqual({
      path: '',
      match_type: 'prefix',
    })
    expect(pathScopeFrom(null, null)).toEqual({ path: '', match_type: 'prefix' })
  })

  it('round-trips a stored scope', () => {
    expect(pathScopeFrom('/assets/*', 'wildcard')).toEqual({
      path: '/assets/*',
      match_type: 'wildcard',
    })
  })
})

describe('validatePathScope', () => {
  it('accepts an empty path as all paths', () => {
    expect(validatePathScope('', 'regex')).toBeNull()
    expect(validatePathScope('  ', 'prefix')).toBeNull()
  })

  it('applies the path-exception rules to a filled path', () => {
    expect(validatePathScope('/api', 'prefix')).toBeNull()
    expect(validatePathScope('api', 'prefix')).toBe(
      'pages.pathExceptions.errorPathRooted',
    )
    expect(validatePathScope('/assets', 'wildcard')).toBe(
      'pages.pathExceptions.errorPathWildcardSuffix',
    )
  })
})
