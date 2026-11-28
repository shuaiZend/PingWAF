/**
 * Parse HTTP status codes, comma-separated ranges and status classes.
 *
 * Examples:
 *   404
 *   400, 403, 404, 405
 *   400-499
 *   4xx, 500, 503
 *
 * Invalid input returns null. Empty input returns an empty array.
 */
export function parseResponseStatusCodes(
  input: string,
): number[] | null {
  const text = input.trim()

  if (!text) return []

  const codes = new Set<number>()

  for (const raw of text.split(',')) {
    const token = raw.trim().toLowerCase()

    const group = /^([2-5])xx$/.exec(token)

    if (group) {
      const start = Number(group[1]) * 100

      for (let code = start; code < start + 100; code++) {
        codes.add(code)
      }

      continue
    }

    const range = /^(\d{3})\s*-\s*(\d{3})$/.exec(token)

    if (range) {
      const start = Number(range[1])
      const end = Number(range[2])

      if (start < 200 || end > 599 || start > end) {
        return null
      }

      for (let code = start; code <= end; code++) {
        codes.add(code)
      }

      continue
    }

    if (!/^\d{3}$/.test(token)) return null

    const code = Number(token)

    if (code < 200 || code > 599) return null

    codes.add(code)
  }

  return [...codes].sort((a, b) => a - b)
}

/**
 * Compress consecutive HTTP status codes for display and editing.
 */
export function formatResponseStatusCodes(
  statuses: number[],
): string {
  const codes = [...new Set(statuses)]
    .sort((a, b) => a - b)

  const parts: string[] = []

  for (let i = 0; i < codes.length; i++) {
    const start = codes[i]
    let end = start

    while (
      i + 1 < codes.length &&
      codes[i + 1] === end + 1
    ) {
      i++
      end = codes[i]
    }

    if (start === end) {
      parts.push(String(start))
    } else if (
      start % 100 === 0 &&
      end === start + 99
    ) {
      parts.push(`${start / 100}xx`)
    } else {
      parts.push(`${start}-${end}`)
    }
  }

  return parts.join(', ')
}
