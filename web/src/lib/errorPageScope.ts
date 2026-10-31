/**
 * Custom error page scoping — mirrors the server-side rules for
 * `scope=site|unmatched_host`:
 *
 * - `site` pages keep the original status-code range;
 * - `unmatched_host` pages (served when the Host header matches no site, or
 *   when a client hits the gateway by bare IP) only allow 404 / 421 / 503;
 * - upsert is keyed on `(scope, status_code)`, so duplicates are per-scope.
 */

export type ErrorPageScope = 'site' | 'unmatched_host'

/** Status codes the control plane accepts for `unmatched_host` pages. */
export const UNMATCHED_HOST_STATUS_CODES: readonly number[] = [404, 421, 503]

/** Narrow an unknown string to a scope, defaulting to `site`. */
export function asErrorPageScope(value: unknown): ErrorPageScope {
  return value === 'unmatched_host' ? 'unmatched_host' : 'site'
}

/** Whether `statusCode` may be used for a page of the given scope. */
export function statusCodeAllowedForScope(
  scope: ErrorPageScope,
  statusCode: number,
): boolean {
  return scope === 'unmatched_host'
    ? UNMATCHED_HOST_STATUS_CODES.includes(statusCode)
    : true
}

/** The status codes the console offers cards for, per scope. */
export function statusCodesForScope(scope: ErrorPageScope): readonly number[] {
  return scope === 'unmatched_host'
    ? UNMATCHED_HOST_STATUS_CODES
    : [403, 429, 502, 503, 504]
}

/** Filter a page list down to one scope (defensive: rows without `scope`
 * are legacy `site` rows). */
export function filterErrorPagesByScope<T extends { scope?: unknown }>(
  items: T[],
  scope: ErrorPageScope,
): T[] {
  return items.filter((item) => asErrorPageScope(item.scope) === scope)
}

/** Status code a dialog should default to when switching to a scope. */
export function defaultStatusCodeForScope(
  scope: ErrorPageScope,
  current: number,
): number {
  return statusCodeAllowedForScope(scope, current)
    ? current
    : scope === 'unmatched_host'
      ? UNMATCHED_HOST_STATUS_CODES[0]
      : current
}
