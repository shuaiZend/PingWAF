import { apiClient } from './client'
import type {
  CacheRule,
  CacheStats,
  CreateCacheRuleRequest,
  Page,
  PaginationQuery,
  PurgeCacheRequest,
  PurgeCacheResult,
  UpdateCacheRuleRequest,
} from './types'

/**
 * Caching — `/api/v1/cache/*`.
 *
 * Rules are site-scoped (`?site_id=`) and decide what the edge stores, for how
 * long, and within what disk budget. `POST /cache/purge` evicts either an
 * explicit URL set or the whole site cache.
 */
export const cachingApi = {
  listRules: (siteId: string, query: PaginationQuery = {}) =>
    apiClient.get<Page<CacheRule>>('/cache/rules', {
      query: { site_id: siteId, page_size: 200, ...query },
    }),

  createRule: (siteId: string, data: CreateCacheRuleRequest) =>
    apiClient.post<CacheRule>('/cache/rules', { ...data, site_id: siteId }),

  updateRule: (siteId: string, id: string, data: UpdateCacheRuleRequest) =>
    apiClient.put<CacheRule>(`/cache/rules/${id}`, data, { query: { site_id: siteId } }),

  deleteRule: (siteId: string, id: string) =>
    apiClient.delete<void>(`/cache/rules/${id}`, { query: { site_id: siteId } }),

  toggleEnabled: (siteId: string, id: string, enabled: boolean) =>
    apiClient.put<CacheRule>(`/cache/rules/${id}`, { enabled }, { query: { site_id: siteId } }),

  stats: (siteId: string) =>
    apiClient.get<CacheStats>('/cache/stats', { query: { site_id: siteId } }),

  purge: (data: PurgeCacheRequest) =>
    apiClient.post<PurgeCacheResult>('/cache/purge', data),
}

export const cacheKeys = {
  all: (siteId: string) => ['cache', siteId] as const,
  rules: (siteId: string, query?: PaginationQuery) =>
    ['cache', siteId, 'rules', query ?? {}] as const,
  stats: (siteId: string) => ['cache', siteId, 'stats'] as const,
}

export default cachingApi
