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
  "version": "0.1.0",
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

Create a new site.

**Request:**
```json
{
  "name": "My Site",
  "domain": "example.com",
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

Add an upstream.

**Request:**
```json
{
  "name": "app-server-1",
  "address": "10.0.1.50:3000",
  "weight": 1,
  "tls": false
}
```

### PUT /sites/{site_id}/upstreams/{upstream_id}

Update an upstream.

### DELETE /sites/{site_id}/upstreams/{upstream_id}

Remove an upstream.

---

## Site SSL

### GET /sites/{site_id}/ssl

Get SSL configuration for a site.

### PUT /sites/{site_id}/ssl

Create or update SSL settings.

**Request:**
```json
{
  "force_https": true,
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
  "name": "my-cert",
  "certificate": "-----BEGIN CERTIFICATE-----\n...",
  "private_key": "-----BEGIN PRIVATE KEY-----\n...",
  "domains": ["example.com"]
}
```

**Request (ACME):**
```json
{
  "name": "letsencrypt",
  "type": "acme",
  "domains": ["example.com", "*.example.com"],
  "challenge_type": "dns",
  "dns_provider": "cloudflare",
  "dns_config": {"api_token": "..."}
}
```

### GET /sites/{site_id}/certificates/{cert_id}

Get certificate details.

### PUT /sites/{site_id}/certificates/{cert_id}

Update certificate.

### DELETE /sites/{site_id}/certificates/{cert_id}

Remove certificate.

### POST /sites/{site_id}/certificates/{cert_id}/renew

Trigger manual certificate renewal.

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
      "version": "0.14.3",
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
  "enabled": true,
  "requests": 100,
  "window_secs": 60,
  "action": "block",
  "characteristics": ["ip"],
  "path_filter": "/api/*"
}
```

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
| `ip` | string | Filter by client IP |
| `start` | string | ISO 8601 start time |
| `end` | string | ISO 8601 end time |

### GET /logs/access

List access logs.

**Query Parameters:**
| Param | Type | Description |
|-------|------|-------------|
| `page` | int | Page number |
| `per_page` | int | Items per page |
| `site_id` | string | Filter by site |
| `status` | int | Filter by HTTP status |
| `path` | string | Filter by request path |
| `start` | string | ISO 8601 start time |
| `end` | string | ISO 8601 end time |

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

**Query Parameters:** `site_id`, `start`, `end`

**Response (200):**
```json
{
  "total_requests": 125000,
  "blocked_requests": 3200,
  "challenged_requests": 450,
  "unique_ips": 8900,
  "top_country": "US"
}
```

### GET /analytics/requests-over-time

Time-series request data.

**Query Parameters:** `site_id`, `start`, `end`, `interval` (e.g., `1h`, `1d`)

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

Create an IP rule.

**Request:**
```json
{
  "ip": "192.168.1.0/24",
  "action": "block",
  "note": "Known bad actor",
  "expires_at": "2025-12-31T23:59:59Z"
}
```

### PUT /sites/{site_id}/ip-rules/{rule_id}

Update an IP rule.

### DELETE /sites/{site_id}/ip-rules/{rule_id}

Remove an IP rule.

### POST /sites/{site_id}/ip-rules/bulk

Bulk import IP rules.

**Request:**
```json
{
  "rules": [
    {"ip": "1.2.3.4", "action": "block", "note": "..."},
    {"ip": "5.6.7.0/24", "action": "allow", "note": "..."}
  ]
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
  "mode": "block_listed",
  "countries": ["CN", "RU", "IR"],
  "action": "block"
}
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
  "mode": "detect",
  "allow_known_bots": true,
  "js_challenge": true,
  "fingerprinting": false
}
```

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
