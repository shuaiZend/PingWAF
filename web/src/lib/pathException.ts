/**
 * Path-exception helpers — pure, tested.
 *
 * Wildcard paths must end in `*` (the data-plane matcher only supports a
 * trailing glob); prefix/exact paths must be rooted and free of `?`/`#`.
 */

import { PATH_SKIP_LAYERS, type PathMatchType, type PathSkipLayer } from '@/api/types'

type PathErrorKey =
  | 'pages.pathExceptions.errorPathRequired'
  | 'pages.pathExceptions.errorPathRooted'
  | 'pages.pathExceptions.errorPathWildcardSuffix'
  | 'pages.pathExceptions.errorPathRegex'

/**
 * Validation for the (path, match_type) pair. Returns an i18n error key or
 * `null` when the input is acceptable.
 */
export function validatePathExceptionPath(
  path: string,
  matchType: PathMatchType | string,
): PathErrorKey | null {
  const trimmed = path.trim()
  if (trimmed.length === 0) return 'pages.pathExceptions.errorPathRequired'
  if (matchType === 'regex') {
    // Regex patterns are free-form: no rooting or glob rules, just basic
    // sanity (length, no whitespace/control chars) and a best-effort syntax
    // check — the server compiles it with the Rust regex crate and answers
    // 400 with a displayable message if it still fails.
    if (trimmed.length > 512) return 'pages.pathExceptions.errorPathRegex'
    // No whitespace or control characters: a request path never contains them,
    // and the server rejects them too. Walking the code points avoids a
    // control-character regex (which the linter forbids for good reason).
    const hasUnsafeChar = [...trimmed].some((ch) => {
      const code = ch.codePointAt(0) ?? 0
      return ch.trim() === '' || code < 0x20 || code === 0x7f
    })
    if (hasUnsafeChar) return 'pages.pathExceptions.errorPathRegex'
    try {
      new RegExp(trimmed)
    } catch {
      return 'pages.pathExceptions.errorPathRegex'
    }
    return null
  }
  if (!trimmed.startsWith('/')) return 'pages.pathExceptions.errorPathRooted'
  if (matchType === 'wildcard' && !trimmed.endsWith('*'))
    return 'pages.pathExceptions.errorPathWildcardSuffix'
  return null
}

/** Whether `layers` bypasses every protection layer. */
export function bypassesAllLayers(layers: string[]): boolean {
  return layers.includes('*')
}

/**
 * Adds or removes one layer from the skip set. When `*` is present and a
 * layer is switched off, the wildcard is expanded into every concrete layer
 * minus the removed one, so the intent ("bypass all except bot") is kept
 * exactly. Adding a layer while `*` is present is a no-op.
 */
export function toggleSkipLayer(
  layers: string[],
  layer: PathSkipLayer,
  on: boolean,
): string[] {
  if (on) {
    if (layers.includes('*') || layers.includes(layer)) return layers
    return [...layers, layer]
  }
  if (layers.includes('*')) {
    return PATH_SKIP_LAYERS.filter((l) => l !== '*' && l !== layer)
  }
  return layers.filter((l) => l !== layer)
}

/** Badge text source: `*` renders as "all" (the pages localize the label). */
export function isAllLayersValue(layer: string): boolean {
  return layer === '*'
}
