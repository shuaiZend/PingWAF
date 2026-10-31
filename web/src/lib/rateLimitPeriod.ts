/**
 * Rate-limit time-window formatting — mirrors the server's
 * `period_seconds` bounds (1–86400, default 60).
 *
 * Pure and locale-agnostic: the unit words are parameters so tests can pin
 * the segmentation logic and the pages can localize them.
 */

export const RATE_LIMIT_MIN_PERIOD_SECONDS = 1
export const RATE_LIMIT_MAX_PERIOD_SECONDS = 86_400

/** Default unit labels (Chinese) — the pages pass i18n-provided words. */
export interface PeriodUnits {
  second: string
  minute: string
  hour: string
}

export const DEFAULT_PERIOD_UNITS: PeriodUnits = {
  second: '秒',
  minute: '分',
  hour: '小时',
}

/** True only for integers inside the server's accepted window. */
export function isValidPeriodSeconds(seconds: unknown): boolean {
  return (
    typeof seconds === 'number' &&
    Number.isInteger(seconds) &&
    seconds >= RATE_LIMIT_MIN_PERIOD_SECONDS &&
    seconds <= RATE_LIMIT_MAX_PERIOD_SECONDS
  )
}

/**
 * Human-readable window: 45 → "45 秒", 90 → "1 分 30 秒",
 * 3600 → "1 小时", 86400 → "24 小时". Returns `null` for values the
 * server would reject (non-integers, <1, >86400) so callers can fall
 * back to a validation message instead of showing nonsense.
 */
export function formatPeriodSeconds(
  seconds: number,
  units: PeriodUnits = DEFAULT_PERIOD_UNITS,
): string | null {
  if (!isValidPeriodSeconds(seconds)) return null
  const hours = Math.floor(seconds / 3600)
  const minutes = Math.floor((seconds % 3600) / 60)
  const secs = seconds % 60
  const parts: string[] = []
  if (hours > 0) parts.push(`${hours} ${units.hour}`)
  if (minutes > 0) parts.push(`${minutes} ${units.minute}`)
  if (secs > 0) parts.push(`${secs} ${units.second}`)
  return parts.join(' ')
}
