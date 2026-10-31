/**
 * Certificate relation grouping — pure, tested.
 *
 * Lives outside `api/ssl.ts` so tests can import it without pulling the
 * API client (and its localStorage-backed auth store) into the test env.
 */

import type { SslCertificate } from '@/api/types'

export interface GroupedCertificates {
  owner: SslCertificate[]
  linked: SslCertificate[]
}

/**
 * Splits a site certificate list into owned and shared-in rows, keyed by the
 * server's `relation` field. Rows without the field (old responses) count as
 * owned, which matches the pre-sharing behaviour.
 */
export function groupCertsByRelation(certs: SslCertificate[]): GroupedCertificates {
  const owner: SslCertificate[] = []
  const linked: SslCertificate[] = []
  for (const cert of certs) {
    if (cert.relation === 'linked') linked.push(cert)
    else owner.push(cert)
  }
  return { owner, linked }
}
