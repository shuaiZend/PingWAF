export type ProtectionTab = 'managed' | 'custom' | 'cc' | 'paths'

/**
 * Tabs accepted from `?tab=`. `paths` must stay in this list: `parseTab` is
 * called on every render, so a missing entry makes the URL write and the next
 * read disagree — the tab writes `paths`, reads it back as `managed`, and the
 * panel never opens.
 */
export const PROTECTION_TABS: ProtectionTab[] = ['managed', 'custom', 'cc', 'paths']

/** Pre-0.22 tab values, still accepted in links and normalized on load. */
export const LEGACY_TABS: Record<string, ProtectionTab> = {
  settings: 'managed',
  rules: 'custom',
}

export function parseTab(value: string | null): ProtectionTab {
  if (value && LEGACY_TABS[value]) return LEGACY_TABS[value]
  return PROTECTION_TABS.includes(value as ProtectionTab)
    ? (value as ProtectionTab)
    : 'managed'
}
