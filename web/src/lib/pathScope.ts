/**
 * Path-scope helpers — pure, tested.
 *
 * IP rules, geo restrictions, basic auth and OIDC all carry the same
 * (path, match_type) pair the path-exception module uses. An empty path
 * means "every path of the site"; the server then stores `prefix`.
 */

import type { PathMatchType } from '@/api/types'
import { validatePathExceptionPath } from './pathException'

/** Wire value for an empty scope: the server normalizes to `prefix`. */
export const ALL_PATHS_MATCH_TYPE: PathMatchType = 'prefix'

/** One (path, match_type) pair as it is sent to (or read from) the API. */
export interface PathScope {
  path: string
  match_type: PathMatchType | string
}

/**
 * Normalizes form input into a wire payload: a blank path collapses to
 * `{ path: '', match_type: 'prefix' }` (all paths), a non-empty path is
 * trimmed and keeps the chosen match type (default `prefix`).
 */
export function normalizePathScope(
  path: string,
  matchType: PathMatchType | string,
): PathScope {
  const trimmed = path.trim()
  if (trimmed === '') return { path: '', match_type: ALL_PATHS_MATCH_TYPE }
  return { path: trimmed, match_type: matchType || ALL_PATHS_MATCH_TYPE }
}

/**
 * Reads scope fields from an API response that may predate them: a missing
 * `path` means all paths, and a match type only matters with a path.
 */
export function pathScopeFrom(
  path: string | null | undefined,
  matchType: string | null | undefined,
): PathScope {
  return normalizePathScope(path ?? '', matchType ?? ALL_PATHS_MATCH_TYPE)
}

/**
 * Validation for the scope pair. An empty path is valid (all paths); a
 * filled path follows the same rules as a path exception. Returns an i18n
 * error key or `null`.
 */
export function validatePathScope(
  path: string,
  matchType: PathMatchType | string,
): string | null {
  if (path.trim() === '') return null
  return validatePathExceptionPath(path, matchType)
}
