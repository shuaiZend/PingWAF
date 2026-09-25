import { apiClient } from './client'
import type {
  Agent,
  AgentCommandRequest,
  AgentCommandResult,
  AgentListQuery,
  Page,
} from './types'

/**
 * Agents — `/api/v1/agents`.
 *
 * Agents are registered implicitly when they first connect with an API key, so
 * the console can only list, inspect, command and deregister them.
 */
export const agentsApi = {
  list: (query: AgentListQuery = {}) =>
    apiClient.get<Page<Agent>>('/agents', { query: { page_size: 200, ...query } }),

  get: (id: string) => apiClient.get<Agent>(`/agents/${id}`),

  remove: (id: string) => apiClient.delete<void>(`/agents/${id}`),

  /**
   * Queues a command for the agent. Answers `202` immediately; `delivered` is
   * true when the agent holds an open stream, otherwise it is `queued` and sent
   * on the next heartbeat.
   */
  sendCommand: (id: string, data: AgentCommandRequest) =>
    apiClient.post<AgentCommandResult>(`/agents/${id}/commands`, data),
}

/** Status ordering used for the summary counters. */
export const AGENT_STATUSES = ['online', 'degraded', 'offline'] as const

export function countByStatus(agents: Agent[]): Record<string, number> {
  return agents.reduce<Record<string, number>>((acc, agent) => {
    acc[agent.status] = (acc[agent.status] ?? 0) + 1
    return acc
  }, {})
}

export const agentKeys = {
  all: ['agents'] as const,
  list: (query?: AgentListQuery) => [...agentKeys.all, 'list', query ?? {}] as const,
  detail: (id: string) => [...agentKeys.all, 'detail', id] as const,
}

export default agentsApi
