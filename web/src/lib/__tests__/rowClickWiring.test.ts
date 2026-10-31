import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * Regression guard for the "delete button opens the edit page" defect class.
 *
 * The unit tests in rowClick.test.ts cover the guard's semantics; this file
 * pins the wiring: the shared `<Table>` must actually consult the guard, and
 * every clickable container that is NOT a guarded `<Table>` row must isolate
 * its row controls with `stopPropagation`. If someone refactors either side
 * away, this fails before the bug ships again.
 */

const src = (rel: string) => readFileSync(join(__dirname, '..', '..', rel), 'utf8')

describe('row-click wiring', () => {
  it('Table rows consult shouldIgnoreRowClick before navigating', () => {
    const table = src('components/ui/Table.tsx')
    expect(table).toContain("from '@/lib/rowClick'")
    expect(table).toContain('shouldIgnoreRowClick(event.target)')
  })

  it('non-Table clickable containers isolate their controls with stopPropagation', () => {
    // SettingsPage error-page cards: the whole card navigates to the editor,
    // the enabled Switch inside must not trigger it.
    expect(src('pages/SettingsPage.tsx')).toMatch(
      /onClick=\{\(e\) => e\.stopPropagation\(\)\}>\s*\n\s*<Switch/,
    )
    // SitesListPage / AgentsPage action cells live in guarded Tables, but they
    // also stop propagation explicitly; keep both lines of defence pinned.
    for (const page of ['pages/SitesListPage.tsx', 'pages/AgentsPage.tsx']) {
      expect(src(page)).toContain('onClick={(e) => e.stopPropagation()}')
    }
    // IpQuickActions inside the clickable dashboard/traffic ranking rows.
    expect(src('components/waf/TopRankingTables.tsx')).toContain(
      '<div onClick={(e) => e.stopPropagation()}>',
    )
  })
})
