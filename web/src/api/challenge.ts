import { apiClient } from './client'
import type {
  ChallengeConfig,
  ChallengeStats,
  UpdateChallengeRequest,
} from './types'

/**
 * CC protection / challenge — `/api/v1/sites/{siteId}/challenge`.
 *
 * A single per-site record: the master toggle, under-attack mode, the default
 * challenge level, clearance duration, the per-IP rate threshold and the path
 * exemptions. `GET` returns the stored config plus live counters.
 */
export const challengeApi = {
  get: (siteId: string) =>
    apiClient.get<ChallengeConfig>(`/sites/${siteId}/challenge`),

  update: (siteId: string, data: UpdateChallengeRequest) =>
    apiClient.put<ChallengeConfig>(`/sites/${siteId}/challenge`, data),

  stats: (siteId: string) =>
    apiClient.get<ChallengeStats>(`/sites/${siteId}/challenge/stats`),
}

/** Sensible defaults matching the server-side `ChallengeConfig::default()`. */
export function defaultChallengeConfig(siteId: string): ChallengeConfig {
  return {
    site_id: siteId,
    enabled: false,
    under_attack: false,
    challenge_level: 'js_challenge',
    clearance_duration: 30,
    rate_threshold: 100,
    exempt_paths: [],
    browser_integrity_check: true,
    tls_fingerprint_check: false,
  }
}

export const challengeKeys = {
  all: (siteId: string) => ['sites', siteId, 'challenge'] as const,
  config: (siteId: string) => ['sites', siteId, 'challenge', 'config'] as const,
  stats: (siteId: string) => ['sites', siteId, 'challenge', 'stats'] as const,
}

export default challengeApi
