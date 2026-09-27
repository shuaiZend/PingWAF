/**
 * Wire types for the PingWAF control-plane REST API (`/api/v1`).
 *
 * Everything mirrors the Rust `serde` structs in `pingwaf-server/src/api/*`
 * exactly — field names are `snake_case` and enum-ish values are the short
 * lowercase strings persisted in PostgreSQL (`block`, `active`, `online`, …).
 */

/* ── Shared envelopes ─────────────────────────────────────────────── */

/** `api::common::Page<T>` — the envelope every list endpoint returns. */
export interface Page<T> {
  items: T[]
  total: number
  page: number
  page_size: number
}

/** `api::common::Pagination` query parameters. */
export interface PaginationQuery {
  page?: number
  page_size?: number
}

/* ── Auth ─────────────────────────────────────────────────────────── */

export type UserRole = 'admin' | 'viewer'

/** `api::auth::UserResponse` */
export interface User {
  id: string
  email: string
  name: string | null
  role: UserRole | string
  created_at: string
  updated_at: string
}

export interface LoginRequest {
  email: string
  password: string
}

export interface RegisterRequest {
  email: string
  password: string
  name?: string
}

/** `api::auth::TokenResponse` */
export interface LoginResponse {
  access_token: string
  refresh_token: string
  token_type: string
  expires_in: number
  user: User
}

/** `api::auth::AuthStatus` */
export interface AuthStatus {
  needs_setup: boolean
  registration_open: boolean
}

export interface ChangePasswordRequest {
  current_password: string
  new_password: string
}

export interface UpdateProfileRequest {
  name?: string | null
}

/* ── Sites ────────────────────────────────────────────────────────── */

export type SiteStatus = 'active' | 'paused' | 'pending'

/** `api::sites::SiteResponse` */
export interface Site {
  id: string
  name: string
  domain: string
  status: SiteStatus | string
  plan: string
  /** Disk budget, in MiB, the agents may use for this site's cache. */
  cache_quota_mb: number
  user_id: string
  created_at: string
  updated_at: string
}

/** `models::sites::site_upstreams::Model` */
export interface Upstream {
  id: string
  site_id: string
  name: string
  address: string
  weight: number
  tls: boolean
  health_status: string
  created_at: string
}

/** `api::sites::SslResponse` — never carries the private key. */
export interface SslConfig {
  id: string
  site_id: string
  domain: string
  issuer: string | null
  has_certificate: boolean
  has_private_key: boolean
  expires_at: string | null
  auto_renew: boolean
  acme_email: string | null
  acme_challenge_type: string | null
  acme_dns_provider: string | null
  acme_dns_config: unknown | null
  created_at: string
}

/** `api::sites::SiteDetail` */
export interface SiteDetail {
  site: Site
  upstreams: Upstream[]
  ssl: SslConfig | null
  rule_count: number
  rule_group_count: number
  rate_limit_count: number
  cache_rule_count: number
}

export interface CreateSiteRequest {
  name: string
  domain: string
  /**
   * Origin the site proxies to — a CDN/WAF hostname or the application itself.
   * Required: a site without an origin cannot serve traffic.
   */
  upstream_address: string
  upstream_name?: string
  upstream_tls?: boolean
  status?: string
  plan?: string
}

export interface UpdateSiteRequest {
  name?: string
  domain?: string
  status?: string
  plan?: string
}

export interface CreateUpstreamRequest {
  name: string
  address: string
  weight?: number
  tls?: boolean
}

export interface UpsertSslRequest {
  domain: string
  cert_pem?: string | null
  key_pem?: string | null
  issuer?: string | null
  expires_at?: string | null
  auto_renew?: boolean
  acme_email?: string | null
  acme_challenge_type?: string | null
  acme_dns_provider?: string | null
  acme_dns_config?: unknown | null
}

export interface SiteListQuery extends PaginationQuery {
  search?: string
  status?: string
}

/* ── WAF rule groups & rules ──────────────────────────────────────── */

/** `models::rules::action` */
export type RuleAction = 'block' | 'log' | 'challenge' | 'js_challenge' | 'allow'
/** `models::rules::mode` */
export type RuleMode = 'off' | 'monitor' | 'block'
/** `models::rules::phase` */
export type RulePhase = 'request' | 'response' | 'custom'

export const RULE_ACTIONS: RuleAction[] = [
  'block',
  'log',
  'challenge',
  'js_challenge',
  'allow',
]
export const RULE_MODES: RuleMode[] = ['off', 'monitor', 'block']
export const RULE_PHASES: RulePhase[] = ['request', 'response', 'custom']

/** `models::rules::rule_groups::Model` */
export interface RuleGroup {
  id: string
  site_id: string
  name: string
  phase: RulePhase | string
  priority: number
  enabled: boolean
  created_at: string
  updated_at: string
}

/** `models::rules::rules::Model` */
export interface Rule {
  id: string
  group_id: string | null
  site_id: string
  name: string
  description: string | null
  expression: string
  action: RuleAction | string
  severity: number
  tags: string[]
  enabled: boolean
  mode: RuleMode | string
  priority: number
  created_at: string
  updated_at: string
}

export interface CreateGroupRequest {
  name: string
  phase?: string
  priority?: number
  enabled?: boolean
}

export interface UpdateGroupRequest {
  name?: string
  phase?: string
  priority?: number
  enabled?: boolean
}

export interface CreateRuleRequest {
  name: string
  group_id?: string | null
  description?: string | null
  expression: string
  action?: string
  severity?: number
  tags?: string[]
  enabled?: boolean
  mode?: string
  priority?: number
}

export interface UpdateRuleRequest {
  name?: string
  group_id?: string | null
  /** `true` detaches the rule from its group. */
  clear_group?: boolean
  description?: string | null
  expression?: string
  action?: string
  severity?: number
  tags?: string[]
  enabled?: boolean
  mode?: string
  priority?: number
}

export interface RuleListQuery extends PaginationQuery {
  group_id?: string
  enabled?: boolean
  search?: string
}

/**
 * Site-level WAF posture derived on the frontend from the rule set, mirroring
 * `grpc::config::waf_config_to_proto`. There is no persisted site WAF record.
 */
export interface DerivedWafConfig {
  enabled: boolean
  mode: RuleMode
  paranoia_level: number
  active_rules: number
  total_rules: number
  detections: Record<WafDetection, boolean>
}

export type WafDetection = 'sqli' | 'xss' | 'rce' | 'lfi' | 'ssrf' | 'bot'

export const WAF_DETECTIONS: WafDetection[] = [
  'sqli',
  'xss',
  'rce',
  'lfi',
  'ssrf',
  'bot',
]

/** Tag aliases accepted by `waf_config_to_proto` for each detection family. */
export const DETECTION_TAGS: Record<WafDetection, string[]> = {
  sqli: ['sqli', 'sql-injection'],
  xss: ['xss'],
  rce: ['rce', 'command-injection'],
  lfi: ['lfi', 'file-inclusion'],
  ssrf: ['ssrf'],
  bot: ['bot'],
}

/* ── Rate limiting ────────────────────────────────────────────────── */

/** `models::rules::characteristic` */
export type RateLimitCharacteristic =
  | 'ip'
  | 'ip_nat'
  | 'host'
  | 'path'
  | 'header'
  | 'cookie'
  | 'query'
  | 'asn'
  | 'country'
  | 'ja3'

export const RATE_LIMIT_CHARACTERISTICS: RateLimitCharacteristic[] = [
  'ip',
  'ip_nat',
  'host',
  'path',
  'header',
  'cookie',
  'query',
  'asn',
  'country',
  'ja3',
]

/** `models::rules::rate_limit_rules::Model` */
export interface RateLimitRule {
  id: string
  site_id: string
  name: string
  expression: string
  characteristics: string[]
  period_seconds: number
  threshold: number
  action: RuleAction | string
  mitigation_timeout_seconds: number
  enabled: boolean
  priority: number
  created_at: string
  updated_at: string
}

export interface CreateRateLimitRequest {
  name: string
  expression?: string
  characteristics?: string[]
  period_seconds?: number
  threshold?: number
  action?: string
  mitigation_timeout_seconds?: number
  enabled?: boolean
  priority?: number
}

export interface UpdateRateLimitRequest {
  name?: string
  expression?: string
  characteristics?: string[]
  period_seconds?: number
  threshold?: number
  action?: string
  mitigation_timeout_seconds?: number
  enabled?: boolean
  priority?: number
}

/* ── Cache rules ──────────────────────────────────────────────────── */

/** `models::rules::cache_rules::Model` */
export interface CacheRule {
  id: string
  site_id: string
  name: string
  match_expression: string
  edge_ttl_seconds: number
  browser_ttl_seconds: number
  disk_quota_mb: number
  cache_eligible: boolean
  respect_origin: boolean
  enabled: boolean
  created_at: string
}

/* ── Logs ─────────────────────────────────────────────────────────── */

/** `models::security_events::security_event::Model` */
export interface SecurityEvent {
  id: number
  site_id: string | null
  agent_id: string | null
  request_id: string | null
  timestamp: string
  client_ip: string
  method: string
  host: string | null
  path: string | null
  rule_id: string | null
  rule_name: string | null
  action: string
  score: number | null
  waf_details: string | null
  country_code: string | null
  user_agent: string | null
  created_at: string
}

/** `models::security_events::access_log::Model` */
export interface AccessLog {
  id: number
  site_id: string | null
  agent_id: string | null
  request_id: string | null
  timestamp: string
  client_ip: string
  method: string
  host: string | null
  path: string | null
  query_string: string | null
  status_code: number | null
  response_size: number | null
  upstream_addr: string | null
  upstream_latency_ms: number | null
  total_latency_ms: number | null
  cache_status: string | null
  user_agent: string | null
  referer: string | null
  country_code: string | null
  tls_version: string | null
}

/** `api::logs::SecurityQuery` */
export interface SecurityLogQuery extends PaginationQuery {
  site_id?: string
  from?: string
  to?: string
  client_ip?: string
  action?: string
  rule_id?: string
  host?: string
  path?: string
  country_code?: string
}

/** `api::logs::AccessQuery` */
export interface AccessLogQuery extends PaginationQuery {
  site_id?: string
  from?: string
  to?: string
  client_ip?: string
  method?: string
  status_code?: number
  status_class?: number
  host?: string
  path?: string
  cache_status?: string
  country_code?: string
  min_latency_ms?: number
}

/** Convenience alias used by the logs page. */
export type LogQueryParams = SecurityLogQuery & AccessLogQuery

/** `api::logs::PurgeResult` */
export interface PurgeResult {
  deleted_security_events: number
  deleted_access_logs: number
  cutoff: string
}

/* ── Analytics ────────────────────────────────────────────────────── */

export type AnalyticsInterval = 'minute' | 'hour' | 'day' | 'week'

/** `api::analytics::RangeQuery` */
export interface RangeQuery {
  site_id?: string
  from?: string
  to?: string
  interval?: AnalyticsInterval | string
  limit?: number
}

/** `api::analytics::Summary` */
export interface AnalyticsSummary {
  from: string
  to: string
  site_id: string | null
  requests: number
  unique_ips: number
  cache_hits: number
  cache_hit_rate: number
  avg_latency_ms: number
  max_latency_ms: number
  client_errors: number
  server_errors: number
  security_events: number
  blocked_requests: number
  distinct_attackers: number
  rules_triggered: number
}

/** `api::analytics::TimeBucket` */
export interface TimeBucket {
  bucket: string
  requests: number
  cache_hits: number
  client_errors: number
  server_errors: number
  avg_latency_ms: number
  blocked: number
}

/** `api::analytics::TopRule` */
export interface TopRule {
  rule_id: string
  rule_name: string
  action: string
  hits: number
  unique_ips: number
}

/** `api::analytics::TopIp` */
export interface TopIp {
  client_ip: string
  country_code: string | null
  requests: number
  blocked: number
}

/** `api::analytics::TopPath` */
export interface TopPath {
  path: string
  requests: number
  cache_hits: number
  avg_latency_ms: number
}

/** `api::analytics::StatusCodeCount` */
export interface StatusCodeCount {
  status_code: number
  requests: number
}

/** `api::analytics::SiteOverview` */
export interface SiteOverview {
  site_id: string
  name: string
  domain: string
  status: string
  plan: string
  requests: number
  blocked: number
}

/* ── Agents ───────────────────────────────────────────────────────── */

/** `models::agents::status` */
export type AgentStatus = 'online' | 'offline' | 'degraded'

/** `api::agents::AgentResponse` */
export interface Agent {
  id: string
  site_id: string | null
  site_domain: string | null
  hostname: string
  ip_address: string
  /** Egress address as seen from the public internet; null when unknown. */
  public_ip: string | null
  /** LAN address, with loopback and container bridges filtered out. */
  private_ip: string | null
  version: string | null
  os_info: string | null
  cpu_cores: number | null
  memory_bytes: number | null
  status: AgentStatus | string
  api_key_id: string | null
  config_hash: string | null
  last_heartbeat: string | null
  registered_at: string
  connected: boolean
  pending_commands: number
}

export interface AgentListQuery extends PaginationQuery {
  site_id?: string
  status?: string
  search?: string
}

/**
 * `models::host_samples::Model` — one point of the agent's 5-second host probe.
 *
 * Network and disk totals are cumulative counters; consecutive rows are
 * differenced client-side to draw throughput.
 */
export interface HostSample {
  id: number
  agent_id: string
  sampled_at: string
  cpu_usage_percent: number | null
  load1: number | null
  load5: number | null
  load15: number | null
  memory_total_bytes: number | null
  memory_used_bytes: number | null
  memory_available_bytes: number | null
  swap_total_bytes: number | null
  swap_used_bytes: number | null
  disk_total_bytes: number | null
  disk_used_bytes: number | null
  net_rx_bytes: number | null
  net_tx_bytes: number | null
  disk_read_bytes: number | null
  disk_write_bytes: number | null
  process_count: number | null
  tcp_connections: number | null
  uptime_secs: number | null
  created_at: string
}

export interface AgentSamplesQuery extends PaginationQuery {
  from?: string
  to?: string
}

export type AgentCommand =
  | 'restart'
  | 'purge_cache'
  | 'block_ip'
  | 'unblock_ip'
  | 'reload'

/** `api::agents::CommandRequest` */
export interface AgentCommandRequest {
  command: AgentCommand | string
  site_id?: string | null
  ip_addresses?: string[]
  urls?: string[]
  tags?: string[]
  duration_seconds?: number | null
  reason?: string | null
  graceful?: boolean
}

/** `202 Accepted` body of `POST /agents/{id}/commands`. */
export interface AgentCommandResult {
  agent_id: string
  command: string
  delivered: boolean
  queued: boolean
}

/** `api::agents::EnrollRequest` */
export interface AgentEnrollRequest {
  name?: string
}

/** `api::agents::EnrollResponse` — the token is shown once and never stored. */
export interface AgentEnrollResponse {
  key_id: string
  token: string
  server_url: string
  install_command: string
  binary_command: string
}

/* ── Elasticsearch settings ───────────────────────────────────────── */

/** `es::config::EsConfig` — secrets come back masked as `***`. */
export interface EsConfig {
  urls: string[]
  index_prefix: string
  username: string | null
  password: string | null
  api_key: string | null
  bulk_max_size: number
  bulk_flush_interval_ms: number
  max_body_size: number
  enabled: boolean
  buffer_dir: string | null
  buffer_max_size_mb: number
  channel_capacity: number
  request_timeout_secs: number
}

/** `es::client::EsHealth` */
export interface EsHealth {
  status: string
  cluster_name?: string | null
  number_of_nodes?: number | null
  active_shards?: number | null
}

/** `api::settings::EsSettingsView` */
export interface EsSettingsView {
  config: EsConfig
  running: boolean
  health?: EsHealth | null
  requires_restart: boolean
}

/** `api::settings::EsTestResult` */
export interface EsTestResult {
  ok: boolean
  health?: EsHealth | null
  error?: string | null
  template_installed: boolean
}

/* ── API keys ─────────────────────────────────────────────────────── */

export type ApiKeyPermission = 'agent' | 'read' | 'write'

/** `api::keys::ApiKeyResponse` — `key` is only set by `POST /keys`. */
export interface ApiKey {
  id: string
  user_id: string
  name: string
  key_prefix: string
  permissions: string[]
  expires_at: string | null
  last_used_at: string | null
  created_at: string
  key?: string
}

export interface CreateApiKeyRequest {
  name: string
  permissions?: string[]
  expires_at?: string | null
}

/* ── Misc ─────────────────────────────────────────────────────────── */

/** `GET /api/v1/health` */
export interface HealthResponse {
  status: 'ok' | 'degraded' | string
  database: 'up' | 'down' | string
}

/** `GET /api/v1/version` */
export interface VersionResponse {
  name: string
  version: string
  api: string
  registration_open: boolean
}

/* ── SSL / TLS ────────────────────────────────────────────────────── */

/** ACME challenge types supported by the control plane. */
export type AcmeChallengeType = 'http-01' | 'dns-01'

export const ACME_CHALLENGE_TYPES: AcmeChallengeType[] = ['http-01', 'dns-01']

/** DNS providers wired into the ACME solver. */
export const ACME_DNS_PROVIDERS = [
  'cloudflare',
  'route53',
  'digitalocean',
  'aliyun',
  'dnspod',
  'cloudxns',
  'manual',
] as const

export type TlsVersion = '1.0' | '1.1' | '1.2' | '1.3'

export const TLS_VERSIONS: TlsVersion[] = ['1.0', '1.1', '1.2', '1.3']

/** `api::ssl::cert_status` */
export type CertificateStatus = 'active' | 'pending' | 'expired' | 'failed'

export const CERTIFICATE_STATUSES: CertificateStatus[] = [
  'active',
  'pending',
  'expired',
  'failed',
]

/** `api::ssl::CertificateResponse` — never carries the private key. */
export interface SslCertificate {
  id: string
  site_id: string
  domain: string
  issuer: string | null
  not_before: string | null
  expires_at: string | null
  auto_renew: boolean
  acme_email: string | null
  acme_challenge_type: string
  acme_dns_provider: string | null
  acme_dns_config: unknown | null
  status: CertificateStatus | string
  has_certificate: boolean
  has_private_key: boolean
  created_at: string
  updated_at: string
}

/** `api::ssl::CertificateWithSite` — a row of the cross-site certificate list. */
export interface CertificateWithSite extends SslCertificate {
  site_domain: string
  site_name: string
}

/** `api::ssl::CertificateSummary` */
export interface CertificateSummary {
  total: number
  active: number
  pending: number
  failed: number
  expired: number
  expiring_soon: number
}

/** `api::ssl::GlobalListQuery` */
export interface CertificateListQuery extends PaginationQuery {
  site_id?: string
  status?: string
}

export interface CreateSslRequest {
  /** Required by the global `/certificates` endpoint, ignored by the per-site one. */
  site_id?: string
  domain: string
  /** ACME automation. */
  auto_renew?: boolean
  acme_email?: string | null
  acme_challenge_type?: AcmeChallengeType | string | null
  acme_dns_provider?: string | null
  acme_dns_config?: unknown | null
  /** Manual upload. */
  cert_pem?: string | null
  key_pem?: string | null
  issuer?: string | null
  not_before?: string | null
  expires_at?: string | null
  /** Global endpoint only: attach to the site and turn HTTPS on. */
  activate?: boolean
}

export type UpdateSslRequest = Partial<Omit<CreateSslRequest, 'site_id' | 'activate'>>

/** `api::ssl::SslSettingsResponse` — the site's TLS posture. */
export interface SslSettings {
  https_enabled: boolean
  min_tls_version: TlsVersion | string
  max_tls_version: TlsVersion | string | null
  self_signed: boolean
  certificate_id: string | null
  mtls_enabled: boolean
  has_mtls_client_ca: boolean
  hsts_enabled: boolean
  hsts_max_age: number
  always_use_https: boolean
}

/** `api::sites::TlsPostureRequest` — the write side of `SslSettings`. */
export interface UpdateSslSettingsRequest {
  https_enabled?: boolean
  min_tls_version?: TlsVersion | string
  max_tls_version?: TlsVersion | string | null
  self_signed?: boolean
  certificate_id?: string | null
  mtls_enabled?: boolean
  mtls_client_ca?: string | null
  hsts_enabled?: boolean
  hsts_max_age?: number
  always_use_https?: boolean
}

/* ── Certificate events ────────────────────────────────────────────── */

export type CertEventType = 'created' | 'renewal_requested' | 'renewed' | 'failed' | 'deleted'

export const CERT_EVENT_TYPES: CertEventType[] = [
  'created',
  'renewal_requested',
  'renewed',
  'failed',
  'deleted',
]

export interface CertificateEvent {
  id: string
  certificate_id: string
  site_id: string | null
  event_type: CertEventType | string
  message: string
  details: Record<string, unknown> | null
  created_at: string
  domain: string | null
  site_domain: string | null
}

export interface CertificateEventListQuery extends PaginationQuery {
  certificate_id?: string
  event_type?: string
}

/* ── Cache rules ──────────────────────────────────────────────────── */

export interface CreateCacheRuleRequest {
  name: string
  match_expression?: string
  edge_ttl_seconds?: number
  browser_ttl_seconds?: number
  disk_quota_mb?: number
  cache_eligible?: boolean
  respect_origin?: boolean
  enabled?: boolean
}

export type UpdateCacheRuleRequest = Partial<CreateCacheRuleRequest>

/**
 * `api::cache::CacheSettingsResponse`. The disk budget belongs to the site, not
 * to an individual rule — the agent enforces one ceiling per hostname, and all
 * of a site's hostnames share it.
 */
export interface CacheSettings {
  site_id: string
  quota_mb: number
  /** True while at least one cache rule of the site is enabled. */
  cache_enabled: boolean
  rule_count: number
  enabled_rule_count: number
}

export interface UpdateCacheSettingsRequest {
  quota_mb?: number
}

/**
 * `api::cache::CacheStatusView`. The `configured_*` figures come from the
 * database; the rest comes from agent heartbeats and stays zero until one
 * reports — `reporting_edges === 0` is how the two are told apart.
 */
export interface CacheStatus {
  site_id: string
  domain: string
  configured_quota_mb: number
  enabled_rule_count: number
  reporting_edges: number
  disk_bytes: number
  quota_bytes_per_edge: number
  quota_bytes_total: number
  items: number
  evictions_total: number
  usage_percent: number
  last_reported_at: string | null
}

export interface PurgeCacheRequest {
  site_id: string
  /** Explicit URLs; ignored when `purge_all` is set. */
  urls?: string[]
  purge_all?: boolean
}

/** `api::cache::PurgeResponse` */
export interface PurgeCacheResult {
  site_id: string
  purge_all: boolean
  urls: number
  /** Agents the command reached over a live stream. */
  delivered: number
  /** Agents it was queued for; delivered on their next heartbeat. */
  queued: number
}

/* ── CC protection / challenge ────────────────────────────────────── */

/** Default challenge posture applied when CC protection trips. */
export type ChallengeLevel = 'none' | 'js_challenge' | 'managed' | 'interactive'

export const CHALLENGE_LEVELS: ChallengeLevel[] = [
  'none',
  'js_challenge',
  'managed',
  'interactive',
]

export interface ChallengeConfig {
  site_id: string
  enabled: boolean
  under_attack: boolean
  challenge_level: ChallengeLevel | string
  /** Minutes a solved challenge stays valid for a client. */
  clearance_duration: number
  /** Requests/minute per IP before a challenge is issued. */
  rate_threshold: number
  exempt_paths: string[]
  browser_integrity_check: boolean
  tls_fingerprint_check: boolean
}

export type UpdateChallengeRequest = Partial<Omit<ChallengeConfig, 'site_id'>>

export interface ChallengeStats {
  challenges_served: number
  pass_rate: number
  block_rate: number
}

/* ── IP access rules ──────────────────────────────────────────────── */

export type IpRuleAction = 'block' | 'challenge' | 'allow'

export const IP_RULE_ACTIONS: IpRuleAction[] = ['block', 'challenge', 'allow']

export interface IpRule {
  id: string
  site_id: string
  /** Single address or CIDR range. */
  ip_cidr: string
  action: IpRuleAction | string
  note: string | null
  enabled: boolean
  created_at: string
}

export interface CreateIpRuleRequest {
  ip_cidr: string
  action?: IpRuleAction | string
  note?: string | null
  enabled?: boolean
}

export type UpdateIpRuleRequest = Partial<CreateIpRuleRequest>

/* ── Rewrite rules ────────────────────────────────────────────────── */

export type RewriteDirection = 'request' | 'response'

export const REWRITE_DIRECTIONS: RewriteDirection[] = ['request', 'response']

/** Discriminated union of the operations a rewrite rule can perform. */
export type RewriteOperationType =
  | 'set_header'
  | 'add_header'
  | 'remove_header'
  | 'set_path'
  | 'regex_replace_path'
  | 'set_query_param'
  | 'remove_query_param'
  | 'replace_body'

export const REWRITE_OPERATION_TYPES: RewriteOperationType[] = [
  'set_header',
  'add_header',
  'remove_header',
  'set_path',
  'regex_replace_path',
  'set_query_param',
  'remove_query_param',
  'replace_body',
]

export interface RewriteOperation {
  type: RewriteOperationType | string
  /** Header name / query key / regex pattern / search string. */
  name?: string
  /** Header value / replacement / regex substitution. */
  value?: string
}

export interface RewriteRule {
  id: string
  site_id: string
  name: string
  direction: RewriteDirection | string
  /** Wirefilter-flavoured match expression; empty means "always". */
  condition: string
  operations: RewriteOperation[]
  priority: number
  enabled: boolean
  created_at: string
}

export interface CreateRewriteRuleRequest {
  name: string
  direction?: RewriteDirection | string
  condition?: string
  operations?: RewriteOperation[]
  priority?: number
  enabled?: boolean
}

export type UpdateRewriteRuleRequest = Partial<CreateRewriteRuleRequest>

/* ── Custom error pages ───────────────────────────────────────────── */

export type ErrorPageContentType = 'html' | 'json' | 'text'

export const ERROR_PAGE_CONTENT_TYPES: ErrorPageContentType[] = ['html', 'json', 'text']

/** Status codes the console offers a custom page for out of the box. */
export const ERROR_PAGE_STATUS_CODES = [403, 429, 502, 503, 504] as const

export interface ErrorPage {
  id: string
  site_id: string
  status_code: number
  name: string
  content_type: ErrorPageContentType | string
  template: string
  enabled: boolean
  created_at: string
}

export interface UpsertErrorPageRequest {
  status_code: number
  name?: string
  content_type?: ErrorPageContentType | string
  template?: string
  enabled?: boolean
}

/* ── Bot protection ───────────────────────────────────────────────── */

export type BotAction = 'allow' | 'challenge' | 'block'

export const BOT_ACTIONS: BotAction[] = ['allow', 'challenge', 'block']

export interface KnownBot {
  id: string
  name: string
  /** Substring or regex matched against the User-Agent header. */
  ua_pattern: string
  action: BotAction | string
}

export interface BotConfig {
  site_id: string
  enabled: boolean
  user_agent_analysis: boolean
  js_detection: boolean
  tls_fingerprinting: boolean
  behavioral_analysis: boolean
  action: BotAction | string
  known_bots: KnownBot[]
}

export type UpdateBotRequest = Partial<Omit<BotConfig, 'site_id' | 'known_bots'>>

export interface BotStats {
  bot_request_pct: number
  verified_bots: number
  likely_bots: number
}

/* ── Geo restrictions ─────────────────────────────────────────────── */

export type GeoMode = 'block' | 'allow'

export const GEO_MODES: GeoMode[] = ['block', 'allow']

export type GeoAction = 'block' | 'challenge' | 'js_challenge'

export const GEO_ACTIONS: GeoAction[] = ['block', 'challenge', 'js_challenge']

export interface BlockedAsn {
  asn: number
  description: string
}

export interface GeoConfig {
  site_id: string
  /** `block` = deny listed countries, `allow` = deny everything except listed. */
  mode: GeoMode | string
  countries: string[]
  blocked_asns: BlockedAsn[]
  block_unknown: boolean
  action: GeoAction | string
}

export type UpdateGeoRequest = Partial<Omit<GeoConfig, 'site_id'>>

export interface GeoCountryStat {
  country_code: string
  requests: number
}

/* ── Traffic analytics (site-scoped) ──────────────────────────────── */

/** Preset windows offered by the traffic page time-range selector. */
export type TrafficRange = '1h' | '6h' | '24h' | '7d' | '30d'

export const TRAFFIC_RANGES: TrafficRange[] = ['1h', '6h', '24h', '7d', '30d']

/** Everything the traffic page needs, resolved in one fan-out. */
export interface TrafficOverview {
  summary: AnalyticsSummary
  series: TimeBucket[]
  topPaths: TopPath[]
  topIps: TopIp[]
  topRules: TopRule[]
  statusCodes: StatusCodeCount[]
}

/* ── IP groups (blacklist / whitelist) ────────────────────────────── */

export type IpGroupAction = 'block' | 'allow'

export const IP_GROUP_ACTIONS: IpGroupAction[] = ['block', 'allow']

export interface IpGroup {
  id: string
  name: string
  description: string | null
  ip_ranges: string[]
  action: IpGroupAction | string
  is_global: boolean
  source_url: string | null
  sync_interval_minutes: number | null
  last_synced_at: string | null
  enabled: boolean
  created_at: string
  updated_at: string
}

/** Returned by list/show endpoints — includes the associated site count. */
export interface IpGroupResponse {
  group: IpGroup
  site_count: number
}

export interface CreateIpGroupRequest {
  name: string
  description?: string | null
  ip_ranges: string[]
  action?: IpGroupAction | string
  is_global?: boolean
  source_url?: string | null
  sync_interval_minutes?: number | null
  enabled?: boolean
}

export type UpdateIpGroupRequest = Partial<CreateIpGroupRequest>

export interface IpGroupListQuery extends PaginationQuery {
  action?: string
  is_global?: boolean
  enabled?: boolean
}
