import { apiClient } from './client'
import type { BotConfig, BotStats, KnownBot, UpdateBotRequest } from './types'

/**
 * Bot protection — `/api/v1/sites/{siteId}/bot-protection`.
 *
 * A single per-site record holding the detection-method toggles, the default
 * action for classified bots, and a whitelist of known-good crawlers. Stats are
 * served from a sibling endpoint so the config save stays cheap.
 */
export const botApi = {
  get: (siteId: string) => apiClient.get<BotConfig>(`/sites/${siteId}/bot-protection`),

  update: (siteId: string, data: UpdateBotRequest) =>
    apiClient.put<BotConfig>(`/sites/${siteId}/bot-protection`, data),

  stats: (siteId: string) => apiClient.get<BotStats>(`/sites/${siteId}/bot-protection/stats`),

  /* ── Known-bot whitelist ─────────────────────────────────────────── */
  addKnownBot: (siteId: string, bot: Omit<KnownBot, 'id'>) =>
    apiClient.post<BotConfig>(`/sites/${siteId}/bot-protection/known-bots`, bot),

  updateKnownBot: (siteId: string, id: string, bot: Partial<Omit<KnownBot, 'id'>>) =>
    apiClient.put<BotConfig>(`/sites/${siteId}/bot-protection/known-bots/${id}`, bot),

  removeKnownBot: (siteId: string, id: string) =>
    apiClient.delete<BotConfig>(`/sites/${siteId}/bot-protection/known-bots/${id}`),
}

/** Defaults matching the server-side `BotConfig::default()`. */
export function defaultBotConfig(siteId: string): BotConfig {
  return {
    site_id: siteId,
    enabled: false,
    user_agent_analysis: true,
    js_detection: true,
    tls_fingerprinting: false,
    behavioral_analysis: false,
    action: 'challenge',
    known_bots: [],
  }
}

/** Well-known good crawlers offered as one-click whitelist entries. */
export const COMMON_KNOWN_BOTS: Omit<KnownBot, 'id'>[] = [
  { name: 'Googlebot', ua_pattern: 'Googlebot', action: 'allow' },
  { name: 'Bingbot', ua_pattern: 'bingbot', action: 'allow' },
  { name: 'DuckDuckBot', ua_pattern: 'DuckDuckBot', action: 'allow' },
  { name: 'Slackbot', ua_pattern: 'Slackbot', action: 'allow' },
  { name: 'Twitterbot', ua_pattern: 'Twitterbot', action: 'allow' },
  { name: 'facebookexternalhit', ua_pattern: 'facebookexternalhit', action: 'allow' },
]

export const botKeys = {
  all: (siteId: string) => ['sites', siteId, 'bot-protection'] as const,
  config: (siteId: string) => ['sites', siteId, 'bot-protection', 'config'] as const,
  stats: (siteId: string) => ['sites', siteId, 'bot-protection', 'stats'] as const,
}

export default botApi
