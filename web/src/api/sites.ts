import { apiClient } from './client'
import type {
  CreateSiteRequest,
  CreateUpstreamRequest,
  Page,
  Site,
  SiteDetail,
  SiteListQuery,
  SslConfig,
  UpdateSiteRequest,
  Upstream,
  UpsertSslRequest,
} from './types'

/**
 * Sites CRUD — `/api/v1/sites`.
 *
 * `GET /sites` is paginated (`Page<Site>`); administrators see every site while
 * other roles only see their own. A site the caller may not see answers `404`
 * rather than `403` so existence is never leaked.
 */
export const sitesApi = {
  list: (query: SiteListQuery = {}) =>
    apiClient.get<Page<Site>>('/sites', { query: { page_size: 200, ...query } }),

  get: (id: string) => apiClient.get<SiteDetail>(`/sites/${id}`),

  create: (data: CreateSiteRequest) => apiClient.post<Site>('/sites', data),

  update: (id: string, data: UpdateSiteRequest) =>
    apiClient.put<Site>(`/sites/${id}`, data),

  delete: (id: string) => apiClient.delete<void>(`/sites/${id}`),

  /* ── Origin pool ─────────────────────────────────────────────────── */
  listUpstreams: (siteId: string) =>
    apiClient.get<Upstream[]>(`/sites/${siteId}/upstreams`),

  createUpstream: (siteId: string, data: CreateUpstreamRequest) =>
    apiClient.post<Upstream>(`/sites/${siteId}/upstreams`, data),

  updateUpstream: (
    siteId: string,
    upstreamId: string,
    data: Partial<CreateUpstreamRequest> & { health_status?: string },
  ) =>
    apiClient.put<Upstream>(`/sites/${siteId}/upstreams/${upstreamId}`, data),

  deleteUpstream: (siteId: string, upstreamId: string) =>
    apiClient.delete<void>(`/sites/${siteId}/upstreams/${upstreamId}`),

  /* ── TLS material ────────────────────────────────────────────────── */
  getSsl: (siteId: string) => apiClient.get<SslConfig | null>(`/sites/${siteId}/ssl`),

  upsertSsl: (siteId: string, data: UpsertSslRequest) =>
    apiClient.put<SslConfig>(`/sites/${siteId}/ssl`, data),
}

/** Query keys shared by every site consumer. */
export const siteKeys = {
  all: ['sites'] as const,
  list: (query?: SiteListQuery) => [...siteKeys.all, 'list', query ?? {}] as const,
  detail: (id: string) => [...siteKeys.all, 'detail', id] as const,
  upstreams: (id: string) => [...siteKeys.all, id, 'upstreams'] as const,
  ssl: (id: string) => [...siteKeys.all, id, 'ssl'] as const,
}

export default sitesApi
