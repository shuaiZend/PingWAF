import { describe, expect, it } from 'vitest'
import { defaultErrorPageTemplate, ERROR_PAGE_MESSAGES, renderErrorPagePreview } from './errorPages'

describe('error page editor preview', () => {
  it.each([403, 404, 421, 429, 502, 503, 504])('renders the selected %i status instead of the sample 403', (status) => {
    const html = renderErrorPagePreview(defaultErrorPageTemplate(status, 'text/html'), status)
    expect(html).toContain(String(status))
    expect(html).toContain(ERROR_PAGE_MESSAGES[status])
    if (status !== 403) expect(html).not.toContain('403')
  })

  it('updates custom templates when the selected status changes, retaining other sample variables', () => {
    const template = '{{ error_code }} {{error_message}} {{host}} {{unknown}}'
    expect(renderErrorPagePreview(template, 404)).toBe('404 Not Found 203.0.113.10 {{unknown}}')
    expect(renderErrorPagePreview(template, 503)).toBe('503 Service Unavailable 203.0.113.10 {{unknown}}')
  })
})
