import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Capture the query object handed to apiClient.get without pulling in
// authStore/localStorage (which `client` transitively imports).
const { get } = vi.hoisted(() => ({ get: vi.fn() }))
vi.mock('./client', () => ({ apiClient: { get } }))

import { analyticsApi, analyticsKeys } from './analytics'

const read = (path: string) => readFileSync(resolve(process.cwd(), path), 'utf8')

/**
 * `GET /analytics/top-ips` contract for `blocked_only`:
 * - the Dashboard's "Top threat IPs" card must send `blocked_only=true` so
 *   only IPs with blocked requests appear;
 * - omitting it must not leak `blocked_only` into the query (server treats a
 *   missing param as false — the legacy "Top client IPs" behavior);
 * - the query key must differ so Dashboard and Traffic don't share cache.
 */
describe('GET /api/v1/analytics/top-ips blocked_only contract', () => {
  beforeEach(() => {
    get.mockReset()
    get.mockResolvedValue([])
  })

  it('sends blocked_only: true when requested', async () => {
    await analyticsApi.topIps({ blocked_only: true, limit: 8 })
    expect(get).toHaveBeenCalledTimes(1)
    const query = get.mock.calls[0]![1].query as Record<string, unknown>
    expect(query.blocked_only).toBe(true)
    expect(query.limit).toBe(8)
  })

  it('omits blocked_only from the query when not passed (undefined is dropped)', async () => {
    await analyticsApi.topIps({ limit: 8 })
    const query = get.mock.calls[0]![1].query as Record<string, unknown>
    expect('blocked_only' in query).toBe(false)
    expect(query.blocked_only).toBeUndefined()
  })

  it('query keys differ between blocked_only and plain top-ips requests', () => {
    expect(analyticsKeys.topIps({ blocked_only: true, limit: 8 })).not.toEqual(
      analyticsKeys.topIps({ limit: 8 }),
    )
  })

  it('DashboardPage requests blocked IPs; TrafficPage keeps the all-clients semantics', () => {
    const dashboard = read('src/pages/DashboardPage.tsx')
    expect(dashboard).toContain('blocked_only: true')
    const traffic = read('src/pages/sites/TrafficPage.tsx')
    expect(traffic).not.toContain('blocked_only')
    const trafficApi = read('src/api/traffic.ts')
    expect(trafficApi).not.toContain('blocked_only')
  })
})
