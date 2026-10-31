import { apiClient } from './client'
import type {
  CreatePathExceptionRequest,
  PathException,
  UpdatePathExceptionRequest,
} from './types'

/**
 * Path exceptions — `/api/v1/sites/{siteId}/path-exceptions`.
 *
 * Per-site rules that bypass named protection layers for matched request
 * paths. The server only accepts writes while the site's WAF mode is
 * `monitor` (otherwise 409, message mentions `observation mode`), so the UI
 * gates the editor behind that mode.
 */
export const pathExceptionsApi = {
  list: (siteId: string) =>
    apiClient.get<PathException[]>(`/sites/${siteId}/path-exceptions`),

  create: (siteId: string, data: CreatePathExceptionRequest) =>
    apiClient.post<PathException>(`/sites/${siteId}/path-exceptions`, data),

  update: (siteId: string, id: string, data: UpdatePathExceptionRequest) =>
    apiClient.put<PathException>(`/sites/${siteId}/path-exceptions/${id}`, data),

  remove: (siteId: string, id: string) =>
    apiClient.delete<void>(`/sites/${siteId}/path-exceptions/${id}`),
}

export const pathExceptionKeys = {
  all: (siteId: string) => ['sites', siteId, 'path-exceptions'] as const,
  list: (siteId: string) => [...pathExceptionKeys.all(siteId), 'list'] as const,
}

export default pathExceptionsApi
