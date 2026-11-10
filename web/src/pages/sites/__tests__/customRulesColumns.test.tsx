import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'
import type { TFunction } from 'i18next'
import { Table } from '@/components/ui/Table'
import { buildRuleColumns } from '../ruleColumns'
import type { Rule } from '@/api/types'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

const t = ((key: string) => key) as unknown as TFunction

const rule: Rule = {
  id: 'r1',
  managed_rule_id: null,
  group_id: null,
  site_id: 's1',
  name: 'Rule 1',
  description: null,
  // Long enough that an unbounded `white-space: nowrap` column would blow up
  // the table width (bug report measured 2274px).
  expression:
    'http.request.uri.path starts_with "/very/long/path/prefix" and ip.src in { "203.0.113.0/24" }',
  action: 'block',
  severity: 3,
  tags: ['sqli', 'xss', 'rce', 'bot'],
  enabled: true,
  mode: 'block',
  priority: 10,
  created_at: '',
  updated_at: '',
}

const columns = buildRuleColumns({
  t,
  groups: [],
  canWrite: true,
  toggleBusy: false,
  onToggle: () => undefined,
  onEdit: () => undefined,
  onDelete: () => undefined,
})

function render(): string {
  return renderToStaticMarkup(
    <Table columns={columns} data={[rule]} rowKey={(r) => r.id} />,
  )
}

function cells(html: string): string[] {
  return [...html.matchAll(/<td(?=[\s>])[^>]*>/g)].map((m) => m[0])
}

describe('CustomRulesPanel rule table layout', () => {
  it('makes the name column the flexible one (w-full max-w-0) so truncate works', () => {
    const tds = cells(render())
    expect(tds).toHaveLength(columns.length)
    // name is the second column (after priority)
    expect(tds[1]).toContain('max-w-0')
    expect(tds[1]).toContain('w-full')
    // and its cell exposes the full expression via the title attribute
    expect(render()).toContain(`title="${rule.expression.replace(/"/g, '&quot;')}"`)
  })

  it('keeps the other columns width-bound and nowrap-free', () => {
    const tds = cells(render())
    tds.forEach((td, i) => {
      if (i === 1) return
      expect(td).not.toContain('max-w-0')
    })
    // The expression paragraph itself stays truncatable, not table-bloating.
    expect(tds[1]).toContain('max-w-0')
  })
})
