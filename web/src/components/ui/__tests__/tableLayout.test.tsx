import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'
import { Table, type Column } from '../Table'

// The table only needs `t` for its empty/loading copy; a stub keeps this a
// pure layout test (no i18n instance, no DOM).
vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

interface Row {
  id: string
  label: string
  count: number
}

const columns: Column<Row>[] = [
  { key: 'label', header: '规则名称', cell: (r) => r.label },
  {
    key: 'count',
    header: '攻击次数',
    width: '1%',
    nowrap: true,
    cell: (r) => r.count,
  },
]

function render(): string {
  return renderToStaticMarkup(
    <Table
      columns={columns}
      data={[{ id: 'a', label: 'rule', count: 3 }]}
      rowKey={(r) => r.id}
    />,
  )
}

function tags(html: string, name: 'th' | 'td'): string[] {
  // `(?=[\s>])` keeps `<th>` from matching `<thead>`.
  return [...html.matchAll(new RegExp(`<${name}(?=[\\s>])[^>]*>`, 'g'))].map(
    (m) => m[0],
  )
}

describe('Table column layout', () => {
  it('marks every header with whitespace-nowrap', () => {
    // Otherwise a Chinese header can break between every character, letting
    // the column shrink to a single glyph.
    for (const header of tags(render(), 'th')) {
      expect(header).toContain('whitespace-nowrap')
    }
  })

  it('keeps only the columns marked nowrap on one line', () => {
    const cells = tags(render(), 'td')
    expect(cells).toHaveLength(2)
    expect(cells[0]).not.toContain('whitespace-nowrap')
    expect(cells[1]).toContain('whitespace-nowrap')
  })

  it('leaves the loading placeholder row free to wrap', () => {
    const html = renderToStaticMarkup(
      <Table columns={columns} data={[]} rowKey={(r) => r.id} loading />,
    )
    const cells = tags(html, 'td')
    expect(cells).toHaveLength(1)
    expect(cells[0]).not.toContain('whitespace-nowrap')
  })
})
