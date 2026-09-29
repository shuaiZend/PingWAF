# PingWAF REST API Reference

Base URL: `http://your-server:9080/api/v1`

## Authentication

All endpoints (except `/auth/login`, `/auth/register`, `/auth/status`, `/health`, `/version`) require a Bearer token:

```
Authorization: Bearer <access_token>
```

### POST /auth/login

Authenticate and receive tokens.

**Request:**
```json
{
  "email": "admin@pingwaf.local",
  "password": "pingwaf123"
}
```

**Response (200):**
```json
{
  "access_token": "eyJ...",
  "refresh_token": "eyJ...",
  "token_type": "Bearer",
  "expires_in": 43200,
  "user": {
    "id": "uuid",
    "email": "admin@pingwaf.local",
    "name": "Administrator",
    "role": "admin",
    "created_at": "2024-01-01T00:00:00Z",
    "updated_at": "2024-01-01T00:00:00Z"
  }
}
```

### POST /auth/register

Create a new account (when registration is open).

**Request:**
```json
{
  "email": "user@example.com",
  "password": "secure-password",
  "name": "Optional Name"
}
```

**Response (201):** Same as login.

### POST /auth/refresh

Exchange a refresh token for new tokens.

**Request:**
```json
{
  "refresh_token": "eyJ..."
}
```

**Response (200):** Same as login.

### GET /auth/status

Check if setup is needed.

**Response (200):**
```json
{
  "needs_setup": false,
  "registration_open": true
}
```

### GET /auth/me

Get current user profile.

### PUT /auth/me

Update current user profile.

**Request:**
```json
{
  "name": "New Name"
}
```

### PUT /auth/password

Change password.

**Request:**
```json
{
  "current_password": "old-password",
  "new_password": "new-password"
}
```

---

## Health & Version

### GET /health

Database connectivity check.

**Response (200):**
```json
{"status": "ok", "database": "up"}
```

### GET /version

Build metadata.

**Response (200):**
```json
{
  "name": "pingwaf-server",
  "version": "0.14.8",
  "api": "/api/v1",
  "registration_open": true
}
```

---

## Sites

### GET /sites

List sites (paginated).

**Query Parameters:**
| Param | Type | Description |
|-------|------|-------------|
| `page` | int | Page number (default: 1) |
| `per_page` | int | Items per page (default: 20) |
| `search` | string | Filter by name/domain |
| `status` | string | Filter by status |

**Response (200):**
```json
{
  "items": [
    {
      "id": "uuid",
      "name": "My Site",
      "domain": "example.com",
      "status": "active",
      "plan": "free",
      "user_id": "uuid",
      "created_at": "...",
      "updated_at": "..."
    }
  ],
  "total": 1,
  "page": 1,
  "per_page": 20
}
```

### POST /sites

Create a new site. `upstream_address` is required: a site without an origin cannot serve traffic. The site is created with a `default` origin pool containing that origin.

**Request:**
```json
{
  "name": "My Site",
  "domain": "example.com",
  "upstream_address": "10.0.1.50:3000",
  "upstream_name": "origin",
  "upstream_tls": false,
  "status": "active",
  "plan": "free"
}
```

**Response (201):** Site object.

### GET /sites/{site_id}

Get site details including upstreams and SSL config.

### PUT /sites/{site_id}

Update site fields.

### DELETE /sites/{site_id}

Delete a site and all associated resources.

---

## Site Upstreams

### GET /sites/{site_id}/upstreams

List origin servers for a site.

### POST /sites/{site_id}/upstreams

Add an upstream (origin node). Without `pool_id` the node joins the site's default pool.

**Request:**
```json
{
  "name": "app-server-1",
  "address": "10.0.1.50:3000",
  "weight": 1,
  "tls": false,
  "pool_id": "uuid"
}
```

### PUT /sites/{site_id}/upstreams/{upstream_id}

Update an upstream.

### DELETE /sites/{site_id}/upstreams/{upstream_id}

Remove an upstream.

### GET /sites/{site_id}/upstream-pools

List origin pools for a site, default pool first. Each pool:

```json
{
  "id": "uuid",
  "site_id": "uuid",
  "name": "default",
  "lb_algorithm": "round_robin",
  "sni": null,
  "verify_cert": null,
  "is_default": true,
  "created_at": "2024-01-01T00:00:00Z"
}
```

### POST /sites/{site_id}/upstream-pools

Create an origin pool.

**Request:**
```json
{
  "name": "primary",
  "lb_algorithm": "hash:cookie:session",
  "sni": "origin.example.com",
  "verify_cert": true
}
```

`lb_algorithm` accepts `round_robin` or `hash:<type>[:<key>]` with type in `ip`, `url`, `path` (no key) or `header`, `cookie`, `query` (key required). A non-empty `sni` enables TLS to the origin.

### PUT /sites/{site_id}/upstream-pools/{pool_id}

Update an origin pool (`sni: ""` disables origin TLS).

### DELETE /sites/{site_id}/upstream-pools/{pool_id}

Remove an origin pool. Refused with `400` for the default pool and `409` while routes still reference it or origin nodes remain in it.

### GET /sites/{site_id}/routes

List path-based routes for a site, highest priority first. Each route:

```json
{
  "id": "uuid",
  "site_id": "uuid",
  "name": "api-to-dedicated-pool",
  "match_type": "prefix",
  "path": "/api",
  "priority": 512,
  "enabled": true,
  "pool_id": "uuid",
  "created_at": "2024-01-01T00:00:00Z"
}
```

### POST /sites/{site_id}/routes

Create a route.

**Request:**
```json
{
  "name": "api-to-dedicated-pool",
  "match_type": "prefix",
  "path": "/api",
  "priority": 512,
  "enabled": true,
  "pool_id": "uuid"
}
```

`match_type` is `prefix`, `exact` or `regex`; `priority` (1-60000) is optional and defaults to the auto weight.

### PUT /sites/{site_id}/routes/{route_id}

Update a route (`priority: 0` resets it to the auto weight).

### DELETE /sites/{site_id}/routes/{route_id}

Remove a route; matching traffic falls back to the default pool.

---

## Site SSL

### GET /sites/{site_id}/ssl

Get SSL configuration for a site.

### PUT /sites/{site_id}/ssl

Create or update SSL settings. `domain` is required; TLS switches (https, HSTS, mTLS, TLS versions) are shared with the ssl-settings endpoint.

**Request:**
```json
{
  "domain": "example.com",
  "https_enabled": true,
  "min_tls_version": "1.2",
  "hsts_enabled": true,
  "hsts_max_age": 31536000
}
```

### DELETE /sites/{site_id}/ssl

Remove SSL settings.

---

## Certificates

### GET /sites/{site_id}/certificates

List certificates for a site.

### POST /sites/{site_id}/certificates

Add a certificate (manual upload or ACME request).

**Request (manual):**
```json
{
  "domain": "example.com",
  "cert_pem": "-----BEGIN CERTIFICATE-----\n...",
  "key_pem": "-----BEGIN PRIVATE KEY-----\n...",
  "issuer": "Let's Encrypt",
  "expires_at": "2025-12-31T23:59:59Z",
  "auto_renew": false
}
```

**Request (ACME):**
```json
{
  "domain": "example.com",
  "acme_email": "ops@example.com",
  "acme_challenge_type": "dns-01",
  "acme_dns_provider": "cloudflare",
  "acme_dns_config": {"api_token": "..."},
  "auto_renew": true
}
```

`acme_challenge_type` is `http-01` or `dns-01`; `acme_dns_provider` is one of `cloudflare`, `route53`, `digitalocean`, `aliyun`, `dnspod`, `cloudxns`, `manual`.

### GET /sites/{site_id}/certificates/{cert_id}

Get certificate details.

### PUT /sites/{site_id}/certificates/{cert_id}

Update certificate.

### DELETE /sites/{site_id}/certificates/{cert_id}

Remove certificate.

### POST /sites/{site_id}/certificates/{cert_id}/renew

Trigger manual certificate renewal (ACME-managed certificates only).

### GET /certificates/{cert_id}/events

Event log for one certificate, newest first (paginated).

**Query Parameters:**
| Param | Type | Description |
|-------|------|-------------|
| `event_type` | string | Filter by event type (e.g., `created`, `renewal_requested`, `renewed`, `failed`, `deleted`, `acme_raw`) |
| `page` | int | Page number |
| `per_page` | int | Items per page |

**Response (200):**
```json
{
  "items": [
    {
      "id": "uuid",
      "certificate_id": "uuid",
      "site_id": "uuid",
      "event_type": "renewal_requested",
      "message": "Manual renewal requested for example.com",
      "details": null,
      "created_at": "2024-01-01T00:00:00Z",
      "domain": "example.com",
      "site_domain": null
    }
  ],
  "total": 1,
  "page": 1,
  "per_page": 20
}
```

### GET /ssl-events

Cross-certificate event log for every site the caller may see (paginated). Supports the same `certificate_id` and `event_type` filters; each event additionally carries the resolved `domain` and `site_domain`.

### GET /sites/{site_id}/ssl-settings

Get SSL settings (TLS version, HSTS, etc.).

### PUT /sites/{site_id}/ssl-settings

Update SSL settings.

---

## API Keys

### GET /keys

List API keys.

### POST /keys

Create a new API key.

**Request:**
```json
{
  "name": "Agent Key",
  "scopes": ["agent"]
}
```

**Response (201):**
```json
{
  "id": "uuid",
  "name": "Agent Key",
  "key": "pwaf_xxxxxxxxxxxx",
  "scopes": ["agent"],
  "created_at": "..."
}
```

> The full key is only returned at creation time.

### DELETE /keys/{key_id}

Revoke an API key.

---

## Agents

### GET /agents

List registered agents.

**Query Parameters:**
| Param | Type | Description |
|-------|------|-------------|
| `page` | int | Page number |
| `per_page` | int | Items per page |
| `site_id` | string | Filter by site |
| `status` | string | Filter: `online`, `offline`, `degraded` |

**Response (200):**
```json
{
  "items": [
    {
      "id": "uuid",
      "site_id": "uuid",
      "site_domain": "example.com",
      "hostname": "edge-01",
      "ip_address": "10.0.1.5",
      "version": "0.14.8",
      "os_info": "Linux 6.1.0",
      "cpu_cores": 4,
      "memory_bytes": 8589934592,
      "status": "online",
      "last_heartbeat": "2024-01-01T12:00:00Z",
      "registered_at": "2024-01-01T00:00:00Z",
      "connected": true,
      "pending_commands": 0
    }
  ],
  "total": 1
}
```

### GET /agents/{agent_id}

Get agent details.

### GET /agents/{agent_id}/samples

Host probe history (CPU, memory, network, disk), newest first. Cumulative counters (network and disk totals) are returned raw; difference consecutive rows to obtain rates.

**Query params:** `from`, `to` (RFC 3339), `page`, `page_size`.

### GET /agents/{agent_id}/metrics

Aggregated edge-metric history shipped by the agent (`pingwaf_requests_total`, `pingwaf_blocked_requests_total`, `pingwaf_active_connections`, `pingwaf_site_cache_used_bytes`). Rows are grouped by their label set and bucketed into `step`-second windows.

**Query params:**
| Param | Description |
|-------|-------------|
| `name` | Required. Metric name, e.g. `pingwaf_requests_total`. |
| `from` | Inclusive lower bound, RFC 3339. Defaults to one hour before `to`. |
| `to` | Exclusive upper bound, RFC 3339. Defaults to now. |
| `step` | Bucket width in seconds, minimum 10. Defaults to 60. |

**Response:**
```json
{
  "name": "pingwaf_site_cache_used_bytes",
  "from": "2024-01-01T11:00:00Z",
  "to": "2024-01-01T12:00:00Z",
  "step": 60,
  "series": [
    {
      "labels": { "site": "example.com" },
      "metric_type": 0,
      "points": [
        { "t": "2024-01-01T11:00:00Z", "avg": 1048576.0, "min": 1048576.0, "max": 1048576.0, "count": 2 }
      ]
    }
  ]
}
```

`metric_type`: `0` gauge, `1` counter. Metric samples are retained for 7 days by default (`PINGWAF_METRIC_RETENTION_DAYS`).

### DELETE /agents/{agent_id}

Deregister an agent.

### POST /agents/{agent_id}/commands

Send a command to an agent.

**Request:**
```json
{
  "command": "block_ip",
  "payload": {
    "ip": "1.2.3.4",
    "duration_secs": 3600
  }
}
```

**Available commands:**
| Command | Payload |
|---------|---------|
| `rule_update` | `{}` |
| `config_reload` | `{}` |
| `block_ip` | `{"ip": "...", "duration_secs": 3600}` |
| `unblock_ip` | `{"ip": "..."}` |
| `purge_cache` | `{"pattern": "..."}` |
| `update_site` | `{"site_id": "..."}` |
| `restart_agent` | `{}` |

---

## WAF Rules

### GET /sites/{site_id}/rule-groups

List rule groups.

### POST /sites/{site_id}/rule-groups

Create a rule group.

**Request:**
```json
{
  "name": "Custom Rules",
  "phase": "request",
  "priority": 100,
  "enabled": true
}
```

### PUT /sites/{site_id}/rule-groups/{group_id}

Update a rule group.

### DELETE /sites/{site_id}/rule-groups/{group_id}

Delete a rule group and its rules.

### GET /sites/{site_id}/rules

List rules.

**Query Parameters:**
| Param | Type | Description |
|-------|------|-------------|
| `page` | int | Page number |
| `per_page` | int | Items per page |
| `group_id` | string | Filter by group |
| `enabled` | bool | Filter enabled/disabled |
| `search` | string | Search by name/description |

### POST /sites/{site_id}/rules

Create a rule.

**Request:**
```json
{
  "group_id": "uuid",
  "name": "Block SQL Injection",
  "description": "Detects common SQLi patterns",
  "enabled": true,
  "priority": 10,
  "mode": "prevention",
  "conditions": [
    {
      "field": "uri",
      "operator": "regex",
      "value": "(?i)(union\\s+select|drop\\s+table)"
    }
  ],
  "action": "block",
  "tags": ["sqli", "owasp-a03"]
}
```

### GET /sites/{site_id}/rules/{rule_id}

Get rule details.

### PUT /sites/{site_id}/rules/{rule_id}

Update a rule.

### DELETE /sites/{site_id}/rules/{rule_id}

Delete a rule.

---

## Rate Limiting

### GET /sites/{site_id}/rate-limit-rules

List rate limit rules.

### POST /sites/{site_id}/rate-limit-rules

Create a rate limit rule.

**Request:**
```json
{
  "name": "API Rate Limit",
  "expression": "",
  "enabled": true,
  "threshold": 100,
  "period_seconds": 60,
  "action": "block",
  "mitigation_timeout_seconds": 60,
  "characteristics": ["ip"],
  "priority": 100
}
```

`characteristics` accepts `ip`, `ip_nat`, `host`, `path`, `header`, `cookie`, `query`, `asn`, `country`, `ja3`. `expression` is an optional filter — empty means all requests.

### PUT /sites/{site_id}/rate-limit-rules/{rule_id}

Update a rate limit rule.

### DELETE /sites/{site_id}/rate-limit-rules/{rule_id}

Delete a rate limit rule.

---

## Cache

### GET /sites/{site_id}/cache-rules

List cache rules for a site.

### POST /sites/{site_id}/cache-rules

Create a cache rule.

**Request:**
```json
{
  "name": "Static Assets",
  "path_pattern": "/static/*",
  "edge_ttl": 86400,
  "browser_ttl": 3600,
  "status": "enabled"
}
```

### PUT /sites/{site_id}/cache-rules/{rule_id}

Update a cache rule.

### DELETE /sites/{site_id}/cache-rules/{rule_id}

Delete a cache rule.

### GET /cache/status

Get cache usage across all sites.

### GET /cache/status/{site_id}

Get cache usage for a specific site.

### POST /cache/purge

Purge cached content.

**Request:**
```json
{
  "site_id": "uuid",
  "pattern": "/images/*"
}
```

### GET /cache/rules

List all cache rules (cross-site, query by `site_id`).

### POST /cache/rules

Create cache rule with `site_id` in body.

### PUT /cache/rules/{rule_id}

Update by rule ID.

### DELETE /cache/rules/{rule_id}

Delete by rule ID.

---

## Logs

### GET /logs/security

List security events.

**Query Parameters:**
| Param | Type | Description |
|-------|------|-------------|
| `page` | int | Page number |
| `per_page` | int | Items per page |
| `site_id` | string | Filter by site |
| `action` | string | Filter: `block`, `allow`, `challenge`, `log` |
| `client_ip` | string | Filter by client IP |
| `rule_id` | string | Filter by rule |
| `path` | string | Filter by request path |
| `request_id` | string | Filter by request id |
| `from` | string | ISO 8601 start time |
| `to` | string | ISO 8601 end time |

### GET /logs/access

List access logs.

**Query Parameters:**
| Param | Type | Description |
|-------|------|-------------|
| `page` | int | Page number |
| `per_page` | int | Items per page |
| `site_id` | string | Filter by site |
| `status_code` | int | Filter by HTTP status |
| `path` | string | Filter by request path |
| `client_ip` | string | Filter by client IP |
| `request_id` | string | Filter by request id |
| `from` | string | ISO 8601 start time |
| `to` | string | ISO 8601 end time |

### DELETE /logs/purge

Purge old logs.

**Request:**
```json
{
  "older_than_days": 30,
  "site_id": "uuid"
}
```

---

## Analytics

### GET /analytics/summary

Aggregate statistics for a time range.

**Query Parameters:** `site_id`, `from`, `to` (ISO 8601; default window is the last 24 hours, capped at 31 days)

**Response (200):**
```json
{
  "from": "2024-01-01T00:00:00Z",
  "to": "2024-01-02T00:00:00Z",
  "site_id": null,
  "requests": 125000,
  "unique_ips": 8900,
  "cache_hits": 98000,
  "cache_hit_rate": 0.784,
  "avg_latency_ms": 42,
  "max_latency_ms": 5120,
  "client_errors": 2100,
  "server_errors": 130,
  "security_events": 3650,
  "blocked_requests": 3200,
  "distinct_attackers": 412,
  "rules_triggered": 1880
}
```

### GET /analytics/requests-over-time

Time-series request data.

**Query Parameters:** `site_id`, `from`, `to`, `interval` (`minute`, `hour` (default), `day` or `week`), `limit`

### GET /analytics/sites-over-time

Request volume over time for the busiest sites — one series per site, for the dashboard's multi-line chart. Without `site_id` the top 8 sites by in-window traffic are returned; non-administrators are scoped to their own sites.

**Query Parameters:** `site_id`, `from`, `to`, `interval` (`minute`, `hour` (default), `day` or `week`)

**Response (200):**
```json
[
  {
    "bucket": "2024-01-01T12:00:00Z",
    "site_id": "uuid",
    "site_domain": "example.com",
    "site_name": "My Site",
    "requests": 1520
  }
]
```

### GET /analytics/top-rules

Most triggered rules.

### GET /analytics/top-ips

Top client IPs by request count.

### GET /analytics/top-paths

Most requested paths.

### GET /analytics/status-codes

Distribution of HTTP response codes.

### GET /analytics/sites

Overview of all sites with request counts.

---

## IP Access Rules

### GET /sites/{site_id}/ip-rules

List IP access rules.

### POST /sites/{site_id}/ip-rules

Create an IP rule. Set either `ip_ranges` (manual mode) or `group_id` (reference a global/per-site IP group) — not both.

**Request:**
```json
{
  "name": "Known bad actor",
  "ip_ranges": ["192.168.1.0/24"],
  "action": "block",
  "note": "Blocked subnet",
  "enabled": true,
  "priority": 100
}
```

`action` is `block`, `allow`, `challenge` or `js_challenge`. Sending `group_id` instead of `ip_ranges` makes the rule track the group's latest ranges.

### PUT /sites/{site_id}/ip-rules/{rule_id}

Update an IP rule (sending `ip_ranges` or `group_id` switches between manual and group mode).

### DELETE /sites/{site_id}/ip-rules/{rule_id}

Remove an IP rule.

### POST /sites/{site_id}/ip-rules/bulk

Bulk import IP rules (one IP or CIDR per entry).

**Request:**
```json
{
  "ip_ranges": ["1.2.3.4", "5.6.7.0/24"],
  "action": "block",
  "note": "Imported blocklist",
  "enabled": true
}
```

---

## IP Groups

Named collections of IP ranges, global (`is_global`, applied to all sites) or per-site. Each group carries an `action` of `block` or `allow`, and can subscribe to an external list via `source_url` with a `sync_interval_minutes` refresh interval (`null` = manual sync only).

### GET /ip-groups

List IP groups (paginated), each with a `site_count`.

**Query Parameters:**
| Param | Type | Description |
|-------|------|-------------|
| `page` | int | Page number |
| `per_page` | int | Items per page |
| `action` | string | Filter: `block` or `allow` |
| `is_global` | bool | Filter global/per-site groups |
| `enabled` | bool | Filter enabled/disabled |

**Response (200):**
```json
{
  "items": [
    {
      "id": "uuid",
      "name": "Cloudflare",
      "description": "Official Cloudflare IP ranges, refreshed daily from api.cloudflare.com.",
      "ip_ranges": ["173.245.48.0/20"],
      "action": "block",
      "is_global": true,
      "source_url": "https://api.cloudflare.com/client/v4/ips",
      "sync_interval_minutes": 1440,
      "last_synced_at": "2024-01-01T12:00:00Z",
      "last_sync_error": null,
      "enabled": false,
      "created_at": "2024-01-01T00:00:00Z",
      "updated_at": "2024-01-01T00:00:00Z",
      "site_count": 0
    }
  ],
  "total": 1,
  "page": 1,
  "per_page": 20
}
```

### POST /ip-groups

Create an IP group (up to 10000 ranges).

**Request:**
```json
{
  "name": "Office allowlist",
  "description": "Corporate egress IPs",
  "ip_ranges": ["203.0.113.7", "198.51.100.0/24"],
  "action": "allow",
  "is_global": false,
  "source_url": "https://example.com/ip-list.txt",
  "sync_interval_minutes": 1440,
  "enabled": true
}
```

### GET /ip-groups/{group_id}

Get an IP group with its `site_count`.

### PUT /ip-groups/{group_id}

Update an IP group (all fields optional).

### DELETE /ip-groups/{group_id}

Delete an IP group.

### GET /ip-groups/{group_id}/sites

List the sites associated with a per-site group.

### PUT /ip-groups/{group_id}/sites

Replace the site associations of a per-site group. Refused with `400` for global groups.

**Request:**
```json
{
  "site_ids": ["uuid"]
}
```

### POST /ip-groups/{group_id}/sync

Trigger an immediate subscription sync from `source_url`. On success the fetched ranges replace the stored ones; on failure the previous ranges are kept, the error is recorded in `last_sync_error`, and the API returns `502`.

**Response (200):**
```json
{
  "id": "uuid",
  "synced_at": "2024-01-01T12:00:00Z",
  "ip_count": 15
}
```

---

## Geo Restrictions

### GET /sites/{site_id}/geo

Get geo restriction settings.

### PUT /sites/{site_id}/geo

Update geo restrictions.

**Request:**
```json
{
  "enabled": true,
  "mode": "block_list",
  "countries": ["CN", "RU", "IR"],
  "blocked_asns": ["AS13335"],
  "block_unknown": false,
  "action": "block"
}
```

`mode` is `block_list` or `allow_list`; `blocked_asns` entries are ASN numbers, optionally prefixed with `AS`.

### GET /sites/{site_id}/geo/stats

Requests per country over the last 24 hours (up to 20 countries), for the bar chart next to the policy editor. Traffic whose country could not be resolved is skipped.

**Response (200):**
```json
[
  {"country_code": "CN", "requests": 5210},
  {"country_code": "US", "requests": 1240}
]
```

---

## Bot Protection

### GET /sites/{site_id}/bot-protection

Get bot protection settings.

### PUT /sites/{site_id}/bot-protection

Update bot protection.

**Request:**
```json
{
  "enabled": true,
  "ua_analysis": true,
  "action": "challenge",
  "known_bots_whitelist": ["Googlebot", "bingbot"]
}
```

`action` is `block`, `challenge`, `js_challenge`, `log` or `allow` and is applied to traffic classified as a bot (non-browser user agents). `known_bots_whitelist` is a JSON array of case-insensitive User-Agent substrings that always pass.

---

## Challenge Settings

### GET /sites/{site_id}/challenge

Get challenge configuration.

### PUT /sites/{site_id}/challenge

Update challenge settings.

**Request:**
```json
{
  "enabled": true,
  "mode": "passive",
  "type": "js_challenge",
  "expiry_secs": 3600
}
```

---

## Rewrite Rules

### GET /sites/{site_id}/rewrite-rules

List rewrite rules.

### POST /sites/{site_id}/rewrite-rules

Create a rewrite rule.

**Request:**
```json
{
  "name": "Strip API prefix",
  "type": "request",
  "match_pattern": "^/api/v1/(.*)",
  "replace_with": "/$1",
  "headers_add": {"X-Forwarded-By": "PingWAF"},
  "headers_remove": ["Server"],
  "priority": 10,
  "enabled": true
}
```

### PUT /sites/{site_id}/rewrite-rules/{rule_id}

Update a rewrite rule.

### DELETE /sites/{site_id}/rewrite-rules/{rule_id}

Delete a rewrite rule.

---

## Error Pages

### GET /sites/{site_id}/error-pages

List custom error pages.

### POST /sites/{site_id}/error-pages

Create a custom error page.

**Request:**
```json
{
  "status_code": 403,
  "content_type": "html",
  "body": "<html><body><h1>Access Denied</h1></body></html>"
}
```

### PUT /sites/{site_id}/error-pages/{page_id}

Update an error page.

### DELETE /sites/{site_id}/error-pages/{page_id}

Delete an error page.

---

## Settings

### GET /settings/elasticsearch

Get Elasticsearch configuration.

### PUT /settings/elasticsearch

Update Elasticsearch settings.

**Request:**
```json
{
  "enabled": true,
  "urls": ["http://elasticsearch:9200"],
  "index_prefix": "pingwaf",
  "username": "elastic",
  "password": "secret",
  "max_body_size": 8192
}
```

### POST /settings/elasticsearch/test

Test Elasticsearch connectivity.

**Response (200):**
```json
{
  "success": true,
  "message": "Connected to Elasticsearch 8.12.0"
}
```

---

## Debug & Profiling

Built-in pprof-style profiling of the control-plane process. All endpoints require an admin token. Sampling is process-global: while a capture is running, further requests return `409 Conflict`.

CPU sampling requires Linux (the only platform with a signal-safe unwinder for the sampling handler); on other platforms the capture endpoints return `501 Not Implemented`, while `/debug/pprof/memory` works everywhere.

Symbols are resolved from the binary's symbol table. The default `release` profile strips symbols, so most frames read `Unknown` — profile a `release-perf` build (`make release-perf`) to get readable flame graphs.

### GET /debug/pprof/profile

Capture CPU samples and return a gzip-compressed pprof protobuf consumable by `go tool pprof` and speedscope.

**Query:** `?seconds=30&frequency=99` — capture window (1-120 s, default 30) and sampling frequency in Hz (1-1000, default 99).

```bash
go tool pprof -http=: http://localhost:9080/api/v1/debug/pprof/profile?seconds=30
```

### GET /debug/pprof/flamegraph

Capture CPU samples and return an SVG flamegraph viewable directly in a browser. Same query parameters as above.

```bash
curl -H "Authorization: Bearer $TOKEN" \
  "http://localhost:9080/api/v1/debug/pprof/flamegraph?seconds=30" > flamegraph.svg
```

### GET /debug/pprof/memory

JSON snapshot of process RSS and system memory (no sampling, returns immediately).

**Response (200):**
```json
{
  "rss_bytes": 120258048,
  "virtual_bytes": 805984256,
  "total_memory_bytes": 17179869184,
  "used_memory_bytes": 9663676416,
  "rss": "121.2 MB",
  "total_memory": "16.0 GB",
  "used_memory": "9.0 GB",
  "collected_at_ms": 1727443200000
}
```

The data plane (pingap) exposes the same three endpoints under its admin path: `GET {admin-path}/api/pprof/profile`, `.../flamegraph` and `.../memory`, authenticated by the admin plugin's credentials. See [Profiling](profiling.md).

---

## Error Responses

All errors follow a consistent format:

```json
{
  "error": {
    "code": "not_found",
    "message": "Site not found"
  }
}
```

| HTTP Status | Code | Description |
|-------------|------|-------------|
| 400 | `bad_request` | Invalid input |
| 401 | `unauthorized` | Missing or invalid token |
| 403 | `forbidden` | Insufficient permissions |
| 404 | `not_found` | Resource does not exist |
| 409 | `conflict` | Duplicate resource |
| 422 | `unprocessable` | Validation failed |
| 500 | `internal_error` | Server error |

---

## Pagination

List endpoints support pagination:

**Query:** `?page=1&per_page=20`

**Response:**
```json
{
  "items": [...],
  "total": 100,
  "page": 1,
  "per_page": 20
}
```
