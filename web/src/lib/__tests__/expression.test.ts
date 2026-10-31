import { describe, expect, it } from 'vitest'

import {
  EXPRESSION_FIELDS,
  REQUEST_FIELDS,
  RESPONSE_FIELDS,
  buildExpression,
} from '../expression'

describe('expression field sets', () => {
  it('splits fields by direction', () => {
    expect(REQUEST_FIELDS.map((f) => f.value)).toContain('http.request.uri.path')
    expect(REQUEST_FIELDS.map((f) => f.value)).not.toContain('http.response.status')
    expect(RESPONSE_FIELDS.map((f) => f.value)).toEqual(['http.response.status'])
  })

  it('keeps EXPRESSION_FIELDS as the merged superset', () => {
    expect(EXPRESSION_FIELDS).toEqual([...REQUEST_FIELDS, ...RESPONSE_FIELDS])
  })

  it('builds a response-status condition', () => {
    expect(buildExpression([{ field: 'http.response.status', operator: 'eq', value: '404' }], 'and')).toBe(
      'http.response.status eq "404"',
    )
  })
})
