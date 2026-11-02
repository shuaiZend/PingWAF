/**
 * Pure decision helpers for the certificate bind / unbind / delete flow.
 * Extracted from GlobalSslPage so the priority rules can be unit tested
 * without a DOM (no jsdom / testing-library in this project).
 */

export type SiteBindingKind = 'owner' | 'bound' | 'bindable'

export interface UsageSite {
  id: string
  domain?: string
  selected?: boolean
}

export interface CertUsage {
  owner?: { id: string; domain?: string } | null
  linked_sites?: UsageSite[]
  serving_sites?: UsageSite[]
}

export interface CertLike {
  site_id: string | null
}

/**
 * 站点相对该证书的状态：属主优先于已绑定，已绑定优先于可绑定。
 * `usage` 缺失时只依据 `cert.site_id` 与 `inlinedBoundSiteIds` 回退，
 * 且属主站点不能被同时算作已绑定（否则会出现解绑自己的属主行）。
 */
export function bindingKind(
  cert: CertLike,
  usage: CertUsage | undefined,
  siteId: string,
  inlinedBoundSiteIds?: string[],
): SiteBindingKind {
  if (usage?.owner?.id === siteId || cert.site_id === siteId) return 'owner'
  if (
    (usage?.linked_sites ?? []).some((site) => site.id === siteId) ||
    (inlinedBoundSiteIds ?? []).includes(siteId)
  ) {
    return 'bound'
  }
  return 'bindable'
}

export interface BindAttemptLike {
  siteId: string
  ok: boolean
  error?: string
}

export interface MergedBindResults {
  ok: string[]
  failed: { siteId: string; reason: string }[]
}

/** 归并逐站提交结果：ok 保留顺序，failed 带可读原因。 */
export function mergeBindResults(results: BindAttemptLike[]): MergedBindResults {
  const ok: string[] = []
  const failed: { siteId: string; reason: string }[] = []
  for (const r of results) {
    if (r.ok) ok.push(r.siteId)
    else failed.push({ siteId: r.siteId, reason: r.error ?? '' })
  }
  return { ok, failed }
}

export interface DeleteBlockersResult {
  blocked: boolean
  sites: { id: string; domain: string }[]
  reason: 'serving' | 'linked' | 'unknown' | null
}

/**
 * 删除是否被阻止，以及阻止它的站点（serving 优先，其次 linked）。
 * `usage` 缺失且行上也没有 `bound_sites` 信号时保守阻塞（unknown），
 * 避免在信息缺失时放开删除、放任后端 409。
 */
export function deleteBlockers(
  cert: CertLike & { bound_sites?: UsageSite[] },
  usage: CertUsage | undefined,
): DeleteBlockersResult {
  const serving = usage?.serving_sites ?? []
  const linked = usage?.linked_sites ?? []
  const inline = cert.bound_sites ?? []

  if (serving.length > 0) {
    return { blocked: true, sites: dedupeSites(serving), reason: 'serving' }
  }
  if (linked.length > 0) {
    return { blocked: true, sites: dedupeSites(linked), reason: 'linked' }
  }
  if (inline.length > 0) {
    return { blocked: true, sites: dedupeSites(inline), reason: 'linked' }
  }
  if (usage === undefined) {
    return { blocked: true, sites: [], reason: 'unknown' }
  }
  return { blocked: false, sites: [], reason: null }
}

function dedupeSites(sites: UsageSite[]): { id: string; domain: string }[] {
  const seen = new Set<string>()
  const out: { id: string; domain: string }[] = []
  for (const site of sites) {
    if (seen.has(site.id)) continue
    seen.add(site.id)
    out.push({ id: site.id, domain: site.domain ?? site.id })
  }
  return out
}
