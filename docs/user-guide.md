# PingWAF User Guide

## Getting Started

### First Login

1. Open the dashboard at `http://your-server:9080`
2. Log in with the seeded credentials:
   - Email: `admin@pingwaf.local`
   - Password: `pingwaf123`
3. **Immediately change your password**: Profile → Change Password

### Dashboard Overview

The dashboard provides:
- **Analytics**: Request trends, top rules triggered, status codes
- **Sites**: Manage protected websites
- **Agents**: Monitor connected data-plane agents
- **Logs**: Security events and access logs
- **Settings**: Elasticsearch, API keys, global preferences

## Adding a Website

### Step 1: Create a Site

Navigate to **Sites → Add Site**:
- **Name**: Human-readable label (e.g., "Production API")
- **Domain**: The hostname to protect (e.g., `api.example.com`)
- **Plan**: `free`, `pro`, or `enterprise` (affects feature limits)

### Step 2: Configure Upstreams

Each site needs at least one origin server:

```
Sites → [your site] → Upstreams → Add
```

| Field | Description |
|-------|-------------|
| Name | Label for this origin (e.g., "app-server-1") |
| Address | Origin `host:port` (e.g., `10.0.1.50:3000`) |
| Weight | Load-balancing weight (default: 1) |
| TLS | Enable if the origin uses HTTPS |

### Step 3: DNS Setup

Point your domain's DNS to the PingWAF server:
- **A record**: `api.example.com → YOUR_PINGWAF_IP`
- **CNAME**: If behind a load balancer

### Step 4: SSL Certificate

See [SSL Certificate Management](#ssl-certificate-management) below.

## WAF Configuration

### Security Modes

Each site operates in one of three modes:

| Mode | Behavior |
|------|----------|
| **Off** | Traffic passes through without inspection |
| **Detection** | Rules evaluated, matches logged, but not blocked |
| **Prevention** | Rules evaluated, matches blocked with configured action |

Set via: `Sites → [site] → Rules → Security Mode`

### Rule Groups

Rules are organized into groups that execute in order:

| Phase | Description |
|-------|-------------|
| `request` | Evaluated on incoming requests |
| `response` | Evaluated on upstream responses |
| `custom` | User-defined rule groups |

### WAF Rules

Each rule defines a condition and an action:

**Conditions** (match any):
- Request URI path (regex or prefix)
- HTTP method
- Request headers
- Query parameters
- Request body content
- IP address / CIDR range
- User-Agent string
- Country (GeoIP)

**Actions**:
- `allow` — Skip remaining rules, permit request
- `block` — Return 403 Forbidden
- `challenge` — Show CAPTCHA/JS challenge
- `rate_limit` — Apply rate limiting
- `log` — Log only, do not block
- `redirect` — Redirect to a URL

### Managed Rulesets

PingWAF includes built-in protection against:
- SQL Injection (SQLi)
- Cross-Site Scripting (XSS)
- Remote Code Execution (RCE)
- Path Traversal
- Command Injection
- Protocol violations

Enable/disable individual rules within managed groups.

## Rate Limiting

Configure per-site rate limits:

```
Sites → [site] → Rate Limiting → Add Rule
```

| Field | Description |
|-------|-------------|
| Name | Rule label |
| Requests | Max requests in the window |
| Window (seconds) | Time window for counting |
| Action | `block`, `challenge`, or `log` |
| Characteristics | How to group traffic: `ip`, `headers:x-api-key`, `cookie:session` |
| Path filter | URI pattern to apply the rule (empty = all) |

Example: Limit to 100 requests per 60 seconds per IP on `/api/*`.

## CC Protection & Challenges

### Challenge Settings

```
Sites → [site] → Challenge
```

| Setting | Options |
|---------|---------|
| Mode | `off`, `passive`, `active` |
| Type | `js_challenge`, `captcha`, `managed` |
| Expiry | How long a passed challenge remains valid |

- **Passive**: Challenge only when suspicious behavior is detected
- **Active**: Challenge all requests matching configured criteria

## Cache Configuration

### Cache Rules

```
Sites → [site] → Cache Rules
```

| Field | Description |
|-------|-------------|
| Name | Rule label |
| Path pattern | URI pattern to cache (e.g., `/static/*`) |
| Edge TTL | How long to cache at the edge (seconds) |
| Browser TTL | `Cache-Control: max-age` sent to clients |
| Status | `enabled` or `disabled` |

### Cache Purging

Purge cached content immediately:

```
Cache → Purge
```

Options:
- Purge all cache for a site
- Purge by URL pattern
- Purge by cache tag

### Disk Quotas

Monitor cache disk usage:

```
Cache → Status
```

Shows per-site cache size and hit rates.

## SSL Certificate Management

### Automatic (ACME / Let's Encrypt)

```
Sites → [site] → Certificates → Add → Let's Encrypt
```

**Challenge Types:**
- **HTTP-01**: Requires port 80 reachable. Best for most setups.
- **DNS-01**: Supports wildcard certificates. Providers:
  - Cloudflare
  - AliDNS (Alibaba Cloud)
  - Huawei DNS
  - Tencent DNS
  - Manual (CNAME delegation)

Certificates auto-renew 30 days before expiry.

### Manual Upload

```
Sites → [site] → Certificates → Add → Upload
```

Provide:
- Certificate (PEM format, including chain)
- Private key (PEM format)
- Domain names covered

### SSL Settings

```
Sites → [site] → SSL Settings
```

- Minimum TLS version (1.2 or 1.3)
- Force HTTPS redirect
- HSTS configuration
- OCSP stapling

## Request/Response Rewriting

### Rewrite Rules

```
Sites → [site] → Rewrite Rules → Add
```

| Field | Description |
|-------|-------------|
| Type | `request` or `response` |
| Match | URL pattern (regex) |
| Replace | Replacement string |
| Headers | Add/remove/modify headers |

Examples:
- Strip `/api/v1` prefix: match `^/api/v1/(.*)` → replace `/$1`
- Add security headers: `X-Frame-Options: DENY`
- Remove `Server` header from responses

## Custom Error Pages

```
Sites → [site] → Error Pages → Add
```

| Field | Description |
|-------|-------------|
| Status code | HTTP status to customize (e.g., 403, 429, 502) |
| Content type | `html`, `json`, or `text` |
| Body | Custom response content |

## Log Management

### Viewing Logs

**Security Events**: `Logs → Security`
- Filtered by site, rule, action, time range
- Shows matched rule, client IP, request details

**Access Logs**: `Logs → Access`
- All proxied requests with timing information
- Filter by status code, path, client IP

### Elasticsearch Integration

For high-volume deployments, ship logs to Elasticsearch:

```
Settings → Elasticsearch
```

| Setting | Description |
|---------|-------------|
| URLs | Comma-separated ES endpoints |
| Index prefix | Prefix for daily indices |
| Username/Password | Basic auth credentials |
| API Key | Alternative to basic auth |
| Max body size | Truncation limit for logged bodies |

Enable the Elasticsearch shipper via environment:
```bash
PINGWAF_ES_ENABLED=true
PINGWAF_ES_URLS=http://elasticsearch:9200
```

### Log Retention

Purge old logs:
```
Logs → Purge (select age threshold)
```

## Agent Management

### Adding Agents

1. Generate an API key: **Settings → API Keys → Create**
2. Install the agent on the edge server:
   ```bash
   pingwaf agent --server-url http://control-plane:9090 --api-key YOUR_KEY
   ```
3. The agent appears automatically in **Agents**

### Monitoring Agents

The Agents page shows:
- Connection status (online/offline/degraded)
- Last heartbeat time
- System info (hostname, OS, CPU, memory)
- Agent version
- Pending commands

### Agent Commands

Push commands to connected agents:
- **Reload Config**: Force immediate config sync
- **Block IP**: Temporarily block an IP address
- **Unblock IP**: Remove a temporary block
- **Purge Cache**: Clear local cache
- **Restart Agent**: Graceful agent restart

## Geo Restrictions

```
Sites → [site] → Geo
```

Block or allow traffic by country:
- **Mode**: `block_listed` or `allow_listed`
- **Countries**: ISO 3166-1 alpha-2 codes (e.g., US, CN, DE)

## IP Access Rules

```
Sites → [site] → IP Rules
```

| Field | Description |
|-------|-------------|
| IP/CIDR | Address or range (e.g., `192.168.1.0/24`) |
| Action | `block`, `allow`, `challenge`, `rate_limit` |
| Note | Description for audit |
| Expiry | Optional auto-removal time |

Bulk import supported via CSV upload.

## Bot Protection

```
Sites → [site] → Bot Protection
```

| Setting | Description |
|---------|-------------|
| Mode | `off`, `detect`, `block` |
| Known bots | Allow verified crawlers (Google, Bing, etc.) |
| JS challenge | Require JavaScript execution |
| Fingerprinting | Browser fingerprint validation |

## Internationalization (i18n)

The dashboard supports multiple languages:
- English (default)
- 中文 (Chinese)
- 日本語 (Japanese)

Language is auto-detected from the browser. Override via the language selector in the dashboard header.
