import { apiClient } from './client'
import type {
  CreateSslRequest,
  SslCertificate,
  SslSettings,
  UpdateSslRequest,
  UpdateSslSettingsRequest,
} from './types'

/**
 * SSL / TLS — `/api/v1/ssl`.
 *
 * Certificates are site-scoped and addressed with `?site_id=`. The private key
 * is write-only: it can be uploaded but never read back. Site-wide TLS posture
 * (minimum version, HSTS, always-HTTPS) lives on a sibling `/ssl/settings`
 * record so it survives certificate churn.
 */
export const sslApi = {
  list: (siteId: string) =>
    apiClient.get<SslCertificate[]>('/ssl', { query: { site_id: siteId } }),

  get: (siteId: string, id: string) =>
    apiClient.get<SslCertificate>(`/ssl/${id}`, { query: { site_id: siteId } }),

  create: (data: CreateSslRequest) => apiClient.post<SslCertificate>('/ssl', data),

  update: (siteId: string, id: string, data: UpdateSslRequest) =>
    apiClient.put<SslCertificate>(`/ssl/${id}`, data, { query: { site_id: siteId } }),

  delete: (siteId: string, id: string) =>
    apiClient.delete<void>(`/ssl/${id}`, { query: { site_id: siteId } }),

  /** Triggers an out-of-band ACME renewal for one certificate. */
  renew: (siteId: string, id: string) =>
    apiClient.post<SslCertificate>(`/ssl/${id}/renew`, undefined, {
      query: { site_id: siteId },
    }),

  /* ── Site-wide TLS posture ───────────────────────────────────────── */
  getSettings: (siteId: string) =>
    apiClient.get<SslSettings>('/ssl/settings', { query: { site_id: siteId } }),

  updateSettings: (siteId: string, data: UpdateSslSettingsRequest) =>
    apiClient.put<SslSettings>('/ssl/settings', data, { query: { site_id: siteId } }),
}

/**
 * Days until a certificate expires, or `null` when no expiry is recorded.
 * Negative values mean the certificate has already lapsed.
 */
export function daysUntilExpiry(expiresAt: string | null, now = Date.now()): number | null {
  if (!expiresAt) return null
  const ts = new Date(expiresAt).getTime()
  if (Number.isNaN(ts)) return null
  return Math.floor((ts - now) / 86_400_000)
}

/** Tone for an expiry badge: red < 7d, amber < 30d, green otherwise. */
export function expiryTone(
  days: number | null,
): 'danger' | 'warning' | 'success' | 'neutral' {
  if (days === null) return 'neutral'
  if (days < 7) return 'danger'
  if (days < 30) return 'warning'
  return 'success'
}

export const sslKeys = {
  all: (siteId: string) => ['ssl', siteId] as const,
  list: (siteId: string) => ['ssl', siteId, 'list'] as const,
  detail: (siteId: string, id: string) => ['ssl', siteId, 'detail', id] as const,
  settings: (siteId: string) => ['ssl', siteId, 'settings'] as const,
}

export default sslApi
