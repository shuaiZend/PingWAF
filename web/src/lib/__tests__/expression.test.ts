import { describe, expect, it } from 'vitest'

import {
  EXPRESSION_FIELDS,
  REQUEST_FIELDS,
  RESPONSE_FIELDS,
  buildExpression,
  parseExpression,
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

  it('indexes a header field and round-trips it', () => {
    const built = buildExpression(
      [
        {
          field: 'http.request.headers',
          headerName: 'x-api-key',
          operator: 'contains',
          value: 'secret',
        },
      ],
      'and',
    )
    expect(built).toBe('http.request.headers["x-api-key"] contains "secret"')
    expect(parseExpression(built)?.conditions[0]).toEqual({
      field: 'http.request.headers',
      headerName: 'x-api-key',
      operator: 'contains',
      value: 'secret',
    })
  })

  it('drops a header condition without a header name instead of emitting a dead expression', () => {
    expect(
      buildExpression(
        [
          {
            field: 'http.request.headers',
            operator: 'contains',
            value: 'secret',
          },
        ],
        'and',
      ),
    ).toBe('')
  })
})
