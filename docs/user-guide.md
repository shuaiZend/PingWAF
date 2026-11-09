# PingWAF User Guide

## Getting Started

### First Login

1. Open the dashboard at `https://your-server:9080` (self-signed certificate
   until you upload a real one under Settings → Control plane HTTPS)
2. Log in with the seeded credentials:
   - Email: `admin@pingwaf.local`
   - Password: `pingwaf123`
3. **Change your password when prompted**: the console blocks the whole UI
   behind a change-password dialog on the first sign-in (the server refuses
   every authenticated request except the auth endpoints). Changing the
   password revokes all of that account's sessions, so the console signs you
   out and asks you to sign in again with the new password — the flag is
   stored per account, not per browser

### Dashboard Overview

The dashboard provides:
- **Analytics**: Request trends, per-site traffic over time, top rules triggered, status codes, client-type mix (desktop / mobile / AI bots / other)
- **Sites**: Manage protected websites
- **Agents**: Monitor connected data-plane agents
- **Logs**: Security events and access logs
- **Account**: Profile, password, personal API keys and (for
  administrators) user management
- **Settings**: Elasticsearch, global preferences, origin certificate verification, control-plane HTTPS. Administrator-only pages (**Config versions**, **Lifecycle**) redirect other roles to the dashboard. System observation lives on the dedicated **/system** page (admin-only, from the account dropdown) rather than the settings page.
- **Config versions**: every configuration snapshot, with per-table diff and rollback

## Adding a Website

### Step 1: Create a Site

Navigate to **Sites → Add Site**:
- **Name**: Human-readable label (e.g., "Production API")
- **Domain**: The hostname to protect (e.g., `api.example.com`)
- **Plan**: `free`, `pro`, or `enterprise` (affects feature limits)

### Step 2: Configure Upstreams

Each site needs at least one origin server:

```
Sites → [your site] → Origin → Add node
```

The **Origin** tab is a site's default view. Every site starts with a `default` origin pool, and the first node you add goes into it — see [Origin Pools & Routes](#origin-pools--routes) for pools, load balancing and path-based dispatch.

| Field | Description |
|-------|-------------|
| Node name | Label for this origin (e.g., "origin-1") |
| Origin address | Origin `host:port` (e.g., `10.0.0.1:8080`) |
| Weight | 1–10000; round robin splits traffic by weight (default: 1) |
| Pool | Origin pool the node belongs to |

### Step 3: DNS Setup

Point your domain's DNS to the PingWAF server:
- **A record**: `api.example.com → YOUR_PINGWAF_IP`
- **CNAME**: If behind a load balancer

### Step 4: SSL Certificate

See [SSL Certificate Management](#ssl-certificate-management) below.

### Cloning a Site

Duplicate an existing site's whole configuration onto a new domain with the
**Clone** action on the site's row in **Sites** (API:
`POST /api/v1/sites/{id}/clone`). The clone is created **paused**, so enabling
it is always an explicit decision — it can never silently start doubling the
source site's traffic.

The clone receives a full copy of the source's configuration: origin pools,
nodes and routes, WAF / challenge / geo / bot / IP-access / cache / rate-limit
settings, custom rules with their rule groups, path exceptions, AI crawler
policy, IP-group bindings and mTLS trust anchors plus client certificates —
and, unlike earlier releases, **certificates (with their private keys),
certificate-sharing links, Basic Auth credentials, the OIDC login
configuration (including its cookie secret) and visitor authorizations**.
TLS protocol switches and ACME contact settings carry over too.

> ⚠️ **Cloning copies sensitive configuration.** Certificate private keys,
> Basic Auth passwords, the OIDC cookie secret and visitor grants all become
> readable on the new site. The console's clone dialog warns about this and
> requires an explicit acknowledgement checkbox before it will send the
> request — treat the Clone action as a secret-duplication decision. The
> clone is created **paused**, so nothing serves until you enable it
> deliberately.

Two things need attention before enabling the clone:

- **OIDC must be re-registered.** The callback (`redirect_url`, or the one
  derived from the request `Host`) still points at the source domain, so the
  identity provider keeps redirecting logins there. Register the new callback
  with the provider first. If the two domains are sibling subdomains under
  one wildcard `Domain`, also rotate `cookie_secret` — a shared secret would
  make the two sites share sessions.
- **Forwarded-header trust is reset** (`trust_proxy_headers`, trusted proxy
  ranges, trusted header, last-hop trust): it describes where the site sits,
  and the clone may sit somewhere else. Re-enable it if the clone is behind
  the same proxy path.

One detail to know when auditing a clone: any node or route whose upstream
pool no longer exists on the source (or predates pools) is repointed at the
clone's **default pool** instead of keeping a dangling reference. The clone's
certificate selection points at the clone's own certificate copy; a
certificate the source had *shared in* from another site is not cloned, so
such a selection is dropped until you link the certificate and choose it
again.

## Origin Pools & Routes

Open **Sites → [your site] → Basic** to manage origin pools, load balancing,
route dispatch and reverse-proxy trust.

### Reverse-Proxy Trust

When the site sits behind a CDN or a reverse proxy (Cloudflare, nginx, a load
balancer), the TCP peer address is the proxy, not the visitor. Enable
**Reverse-proxy trust** on the Basic page to resolve the client IP from a
trusted header instead:

| Setting | Description |
|---------|-------------|
| Front-end CDN / reverse proxy | Enables header-based client IP resolution |
| Trusted header | Which header carries the client IP; defaults to `X-Forwarded-For` (`CF-Connecting-IP`, `X-Real-IP`, `True-Client-IP` and `Forwarded` are also offered) |
| Trust last hop only | Use only the right-most value of the header and discard everything else |

With trust enabled, IP block lists, rate limiting (CC), access rules and the
client IP recorded in logs are all derived from that header. Only enable it
when every request really passes through the proxy — otherwise visitors could
spoof their IP by sending the header themselves.

### Origin Pools

An origin pool is a set of origin nodes sharing one load-balancing algorithm and origin protocol.

| Field | Description |
|-------|-------------|
| Pool name | Label for the pool (e.g., "primary") |
| Load balancing | `Round robin` or a hash algorithm (see below) |
| SNI hostname | Non-empty enables **HTTPS to origin**. A bare hostname covered by the origin certificate, or `$host` to send each request's client `Host` (port stripped, lowercased) as the TLS SNI — useful when the upstream is a virtual host and the site's domains match the `Host` headers. `$host` requires origin certificate verification to be enabled: a pool that turns verification off is rejected with `$host requires origin certificate verification` |
| Verify origin certificate | Whether an HTTPS origin's certificate is validated. Unset inherits the site default (SSL page's origin certificate check) and then the global setting from the console's system settings; `true` requires a certificate that validates against the system trust store (or the pool's own CA); `false` accepts any certificate and is not recommended. |

Load-balancing algorithms:

| Algorithm | Spec | Behavior |
|-----------|------|----------|
| Round robin | `round_robin` | Spreads requests by node weight |
| Least connections | `least_connections` | Picks the node with the fewest in-flight requests |
| Random | `random` | Picks a node at random (weight-aware) |
| IP hash | `hash:ip` | Pins the same client IP to one node |
| URL hash | `hash:url` | Pins by request URL |
| Path hash | `hash:path` | Pins by request path |
| Header hash | `hash:header:<name>` | Pins by a request header value |
| Cookie hash | `hash:cookie:<name>` | Pins by a cookie value |
| Query hash | `hash:query:<name>` | Pins by a query-string parameter |

The site's default pool (marked **Default**) cannot be deleted, and a pool can only be deleted once no origin nodes and no routes reference it.

The **Verify origin certificate** field only matters when the pool has an SNI
hostname. Left unset it follows a three-level cascade: the site default on the
site's SSL page (**Origin certificate check**), and when the site has no
preference either, the deployment-wide default under **Settings → Origin TLS
verification**, which is off — accepting any certificate so that self-signed
and internal-PKI origins keep working. Turn the global default on once every
origin presents a certificate the proxy can chain to a trust store (a private
CA counts; upload it to the pool), pin the site default for a whole site, and
leave per-pool overrides unset unless a specific origin cannot comply.

> **`$host` SNI needs this switch on.** A pool using `$host` lets the client
> choose the name the origin certificate must cover, so origin certificate
> verification is the only thing that can reject an intercepted origin. A
> pool that pins verification off together with `$host` is refused
> (`$host requires origin certificate verification`), and turning
> verification off at site or global level quietly drops the same guarantee
> for every `$host` pool.

### Routes

Routes dispatch requests to different pools by path; unmatched traffic goes to the default pool.

| Field | Description |
|-------|-------------|
| Rule name | Label for the route (e.g., "api-to-dedicated-pool") |
| Match | `Prefix`, `Exact`, or `Regex` |
| Path | Starts with `/` (e.g., `/api`), or a regular expression (e.g., `^/static/.*`) |
| Priority weight | 1–60000; empty uses the auto weight (exact 1024 / prefix 512 / regex 256) |
| Target pool | Pool that receives matching traffic |
| Upstream host | Optional; overrides the `Host` header the origin sees for this route. Empty = forward the client's `Host` (historic behaviour) |
| Enabled | Toggle a route without deleting it |

A prefix route on `/` is rejected: it would conflict with the default-pool fallback.

**Upstream host.** Fill this in when the route's pool fronts a virtual host
that routes by `Host` — for example a mirror route to `google.com`: without
an override the origin sees your site's domain and answers 404. The value
must be a bare host name (at most 253 bytes, no scheme `://`, no path `/`,
no whitespace or control characters; a port is accepted). Other rewrite
knobs on the same route: **Strip path prefix** removes the matched prefix
(`/api/users` → `/users`), **Rewrite path** replaces the whole forwarded
path — see the [API docs](api.md#post-sitessite_idroutes) for exact
semantics.

## WAF Configuration

The Web Protection page is organized into three tabs — **Managed rules** (attack categories, built-in rules, deep inspection, backend stacks), **Custom rules** (posture and user-defined rules) and **Rate limiting & challenge** (rate-limit entry plus challenge settings).

### Security Modes

Each site operates in one of three modes. The console labels are shown first;
the value in parentheses is what the API expects:

| Mode | API value | Behavior |
|------|-----------|----------|
| **Off** | `off` | Traffic passes through without inspection |
| **Detection** | `monitor` | Rules evaluated, matches logged, but not blocked |
| **Prevention** | `block` | Rules evaluated, matches blocked with configured action |

The enforcing value is `block` — **not** `enforce` or `prevention`. The API
rejects anything else with a `400` that lists `off|monitor|block`. The same enum
applies per rule; see [Rule modes](#rule-modes).

Set via: `Sites → [site] → Web Protection → Custom rules`

### Rule modes

Every rule has its own `mode`: `off` (fully disabled — the rule is not
compiled, not evaluated and produces no security event or log entry),
`monitor` (detect and record, never enforce) or `block` (enforce the rule's
action); `enabled: false` is equivalent to `off` and also takes the rule out
of evaluation. A brand-new site is seeded with the built-in rule set in
`block`, so switching the WAF on protects it immediately; restored defaults on
an existing site come back as `monitor` so a repair never starts blocking
traffic that site was serving unfiltered.

The site mode caps the rules: with the site in **Detection** (`monitor`), a rule
that says `block` is *downgraded*, not ignored. The request passes, and the
security event shows both sides — `action: monitor` with `waf_details` ending in
`downgraded=true would_enforce=block cause=site-mode-monitor`, plus the rule's
own `rule_action=block`. If the event has no `downgraded=true`, the rule itself
was configured to observe.

An **allow** rule is the whitelist exemption itself, so it only supports
**Detection** (`monitor`). Mode **block** would never apply, and mode **Off**
would remove the rule from the engine — the whitelist disappears and later
block rules match again. The console only offers `monitor` for `allow`, and the
API rejects both other modes with a `400`. This applies to editing an existing
rule too, even when you only change one of the two fields.

In short: the **action** decides what happens to a request the rule matches
(`block` / `log` / `challenge` / `allow`), the **rule mode** decides whether
this rule actually runs, and the **site mode** is the master switch — with the
site in Detection (`monitor`) or Off, rules are only recorded, never enforced.
The combinations that cover the common intentions:

| Goal | Action | Rule mode | Site mode |
|------|--------|-----------|-----------|
| Enforce the block | `block` | `block` | `block` |
| Watch first, block later | `block` | `monitor` | anything (`monitor` recommended) |
| Whitelist (exempt a pattern) | `allow` | `monitor` | — (`allow` cannot use `block`) |
| Record only | `log` | any | — |

`allow` + `block` is rejected by the API (400), and the dialog falls back to
`monitor` when you switch the action. An `allow` rule is an allowlist entry:
it wins before every other rule and is not suppressed by the site mode, so a
whitelisted path stays reachable even while the site is in monitor/off.

### Rule Groups

Rules are organized into groups that execute in order:

| Phase | Description |
|-------|-------------|
| `request` | Evaluated on incoming requests |
| `response` | Evaluated on upstream responses |
| `custom` | User-defined rule groups |

### Custom Rules

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

### Managed Rules

PingWAF includes built-in protection against SQL injection, XSS, remote code execution, path traversal, command injection and protocol violations. The protection page splits this into two cards:

**Attack categories** (9): `sqli`, `xss`, `rce`, `lfi`, `ssrf`, `deser`, `crlf`, `xxe`, `ssti`. Each category is a two-state switch — **Block** (default) or **Log only**. Log-only keeps the family's detection and logging fully active but stops blocking its matches. There is no per-category "off": detection always runs.

**Built-in rules**: each of the built-in managed rules has its own Block / Log-only switch, independent of the category switches. Batch buttons switch all rules at once. Because a family's rules (SQLi, XSS, RCE score gates) and its category switch control the same signature surface, downgrading a category also flips its family rules to log-only; switching the category back removes those per-rule downgrades, resetting any individual overrides made in between.

**Deep inspection** enables the strict managed rule set plus deep request-body inspection. Higher interception rate at a proportional performance cost. Custom rules receive the same request-body evaluation while deep inspection is on, so body-carrying bypass attempts are caught by user-defined expressions too. On the page the two posture switches — **Under-attack mode** and **Deep inspection** — sit side by side on one row.

**Backend stacks** (`java`, `php`, `python`, `node`) downgrade stack-specific detections the same way; language-agnostic detections are never affected by a stack switch. Stack switches are collapsed by default.

A site without explicit settings inherits the aggregate of its enabled rules: `block` when any rule blocks, otherwise `monitor`. It never silently lands on `off` — an unconfigured site keeps detecting and logging, and only an explicit **Off** (or the engine switch) turns inspection off.

### Under Attack Mode

```
Sites → [site] → Web Protection (top banner)
```

A site-wide emergency switch: every visitor is challenged with a JavaScript check, and CC protection is switched on automatically. Enable it only while the site is under an active flood attack and disable it once the attack subsides.

### Path Exceptions

A per-site list of request paths that skip selected protection layers — for
widening a rule that is too aggressive while you watch what a change actually
affects:

```
Sites → [site] → Web Protection → Path exceptions
```

| Field | Meaning |
|-------|---------|
| Path | Must start with `/` (except `regex`, which is a pattern), at most 512 characters, no whitespace or control characters |
| Match type | `prefix` (default; matches by path segment — `/api` covers `/api/x`, never `/apix`), `exact`, `wildcard` (a single trailing `*`, e.g. `/api/*`), or `regex` (unanchored Rust-regex match, case-sensitive by default, use `^…$` for a full match; must compile server-side) |
| Skip layers | `*` (every layer), or any of `waf`, `bot`, `access`, `basic_auth`, `mtls`, `ai_bot`, `rate_limit`, `challenge` — the hit skips exactly these checks. `challenge` waives **every** challenge source (WAF rule/scoring challenges, CC & rate-limit challenge actions, bot protection, IP/geo rules, the standalone challenge plugin) while blocks still enforce |
| Note | Free text, at most 200 characters; used as the label when a hit is logged |
| Log hits | When on, every hit is recorded as a `path_bypass` security event (action `log`) |
| Enabled | Disabled exceptions are not evaluated |

**All modes:** path exceptions can be saved and are enforced in both
**Detection** (`monitor`) and **Block** mode. A matching request still appears
in the access log; only the layers named in `skip_layers` are skipped. When
`rate_limit` is skipped, the standalone CC rate challenge is skipped too.

`*` covers the layers enforced inside the WAF plugin (WAF engine including
request-body inspection, bot protection, IP/geo restrictions, basic auth,
mTLS and CC). The independent cache and OIDC/visitor-auth plugins are not
part of this list; use their own configuration to scope them.

**What an exception cannot bypass**, no matter which layers it names: a paused
site (maintenance page), a fail-closed edge, and an active dynamic IP ban —
those checks run before the exception and always enforce.

Other failures you may see when saving: a duplicate path on the same site
(`a path exception for '<path>' already exists on this site`), a site at the
64-exception limit, an unknown skip-layer token, or a wildcard path that does
not end in exactly one `*`.

## Rate Limiting

Configure per-site rate limits:

```
Sites → [site] → Rate Limiting → Add Rule
```

| Field | Description |
|-------|-------------|
| Name | Rule label |
| Threshold | Max requests allowed in the period |
| Period (seconds) | Time window for counting, one number in seconds (1–86400, default 60). The input is a single seconds field with a live preview ("100 requests per 60 s") instead of separate unit pickers — one source of truth, no way to save "5 minutes" that is actually 5 seconds |
| Action | `block`, `log`, `challenge`, `js_challenge`, or `allow` |
| Characteristics | How to group traffic: `ip`, `ip_nat`, `host`, `path`, `asn`, `country`, plus parameterized forms `header:<name>`, `cookie:<name>`, `query:<name>` |
| Filter expression | Optional match condition (empty = all requests) |
| Mitigation timeout | How long the action applies once triggered (0–86400 seconds) |

Example: Limit to 100 requests per 60 seconds per IP on `/api/*`.

Requests missing the header, cookie or query parameter bucket together, so they still count against the limit. Header names match case-insensitively; cookie and query names are case-sensitive. The `ja3` characteristic is reserved for future use and is not yet accepted by the API.

## Challenges (CC Protection)

### Challenge Settings

```
Sites → [site] → Web Protection → Rate limiting & challenge
```

Dimension-based rate limit rules live on the separate Rate Limiting page; this tab links to it and holds the challenge settings below.

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

### Rule priority and presets

The cache page shows **one list** with two layers:

- **Explicit rules** (plain rows) are evaluated first, in list order.
- **Category presets** — rows with a *Preset* badge — are the five fixed
  families (`web_assets`, `image`, `media`, `other`, `html`) edited under
  *Edit presets* (collapsed by default). They are read-only in the list,
  evaluated after every explicit rule, and match **only by file extension**.
  First match wins.

Presets alone are enough: a site with no explicit rule still caches as long
as a preset family is enabled and lists extensions. But presets never match
API endpoints or extension-less paths — cover those with an explicit rule.
The whole layer also requires the deployment-wide cache master switch
(**Settings → Cache**, administrators) to be on.

**Convert to rule.** A preset row offers *Convert to rule*: it creates an
explicit rule with the same extensions and TTLs — the generated expression
looks like `http.request.uri.path matches "(?i)\.(?:png|jpg)$"` — and
disables the preset so the same requests are not matched twice. Use it when
a preset is almost right but needs conditions the preset cannot express.

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
- **DNS-01**: Supports wildcard certificates. Providers (`dns_provider` value in parentheses):
  - Aliyun (Alibaba Cloud) — `ali`
  - Cloudflare — `cf`
  - DNSPod (Tencent Cloud) — `tencent`
  - Huawei Cloud — `huawei`
  - Manual — anything else

Certificates auto-renew 30 days before expiry.

### Manual Upload

```
Sites → [site] → Certificates → Add → Upload
```

Provide:
- Certificate (PEM format, including chain)
- Private key (PEM format)
- Domain names covered

### Upload without a site, bind later

Administrators can upload a certificate **without** naming a site — useful
when the PEM is already in hand and the same certificate (a wildcard or a
multi-hostname SAN certificate) will serve several sites. The row lands
**unbound**: no site owns it, nothing serves it, and its status follows its
own validity dates. From there:

1. **Upload** — `POST /api/v1/certificates` with `domain`, `cert_pem` and
   `key_pem` (no `site_id`).
2. **Bind each site** — `POST /api/v1/certificates/{id}/bindings` with
   `{"site_id": "…"}`. Binding fails with `400` when the certificate does not
   cover the site's hostnames (the uncovered **site** names are listed; extra
   SANs on the certificate are fine), with `409` when the site already owns
   the certificate, and is idempotent — binding twice changes nothing.
3. **Unbind** — `DELETE /api/v1/certificates/{id}/bindings/{site_id}`, refused
   with `409` while the certificate is the site's current TLS selection.

Every certificate response shows the result as `bound_sites` / `bound_count`.
Unbound certificates cannot use ACME — issuance and renewal run on the edge,
driven by the site that carries the certificate — and deleting a certificate
that still has bindings is refused with `409`; once nothing is bound, it
deletes immediately. The site-scoped alternative stays available: link from
the site's SSL settings picker (see
[Sharing certificates between sites](#sharing-certificates-between-sites)).

### Certificate lifecycle

A certificate moves through four states, and each state decides what you may
do with it:

```
              upload with site_id          upload without site_id
                     │                            │
                     ▼                            ▼
            ┌─────────────────┐          ┌─────────────────┐
            │  Owner (site)   │          │    Unbound      │
            └─────────────────┘          └─────────────────┘
                     │                            │
        bind other   │ share                      │ bind the
        sites        ▼                            │ first site
            ┌─────────────────┐                    │
            │     Shared      │ ───────────────────┘
            └─────────────────┘
                     │
   a site (own or    │ the site's SSL page
   linked) selects   ▼ selects it as TLS
   it as TLS    ┌─────────────────┐
                │    In use       │
                └─────────────────┘
```

- **Unbound** — uploaded without a site (`site_id: null`, `bound_count: 0`).
  Nothing serves it; ACME and `activate` are refused (`400`) because both are
  per-site operations. Delete it any time (`204`) or bind a site to put it to
  work.
- **Owner** — the site the certificate was created under. The owner is why
  binding the certificate *back* to that site is refused (`409`): it already
  has it. The owner also drives renewals and ACME bookkeeping.
- **Shared** — bound to other sites via certificate bindings
  (`bound_count` counts these **shared sites only, not the owner**).
- **In use** — at least one site has selected the certificate as its TLS
  certificate. This can be the owner's own selection, so a certificate can be
  in use without any shared binding.

**Recommended order of operations** (the console guides you through the same
sequence):

1. **Upload** — with a site (`Sites → [site] → Certificates → Add`) or
   without one (see [Upload without a site, bind later](#upload-without-a-site-bind-later)).
2. **Bind** — from the certificate's bind dialog. Your own site shows an
   **owner** badge: it cannot be selected and has no unbind button. Other
   sites are either already bound (unbindable, with a confirmation reminding
   you the site falls back to its own certificate or none) or available to
   bind — the bind checks that the certificate covers the site's hostnames
   and fails with `400` listing the uncovered ones. Binding several sites at
   once submits them one by one: successes are kept, failures are listed as
   `site — reason`, and the dialog only closes when everything succeeded.
   Rebinding a site that is already bound is idempotent — the second call
   returns the same `200` decorated certificate instead of a conflict.
3. **Unbind** — refused with `409` while the certificate is that site's
   current TLS selection. Go to the site's SSL page first, switch to another
   certificate or clear the selection, then unbind. Unbinding a site that
   was only serving the certificate through the link drops it back to its own
   material — or to no HTTPS at all, which is why the confirmation exists.
4. **Delete** — refused with `409` while any site still selects the
   certificate **or** any binding still grants it. The delete dialog checks
   the certificate's usage first: if any site is serving or linked to it, the
   affected sites are listed and the button becomes **Go handle**, taking you
   to the first blocking site's SSL page. If the usage check itself is
   unavailable, deletion is conservatively refused rather than allowed
   through. Once nothing selects it and nothing is bound, the delete goes
   through immediately (`204`).

### SSL Settings

```
Sites → [site] → SSL Settings
```

- Certificate selection — pick a certificate the site **owns or one shared
  with it** (marked *linked* in the picker); see
  [Sharing certificates between sites](#sharing-certificates-between-sites).
  Choosing a certificate the site neither owns nor has linked fails with
  `does not belong to this site and is not linked to it`.
- Minimum TLS version (1.2 or 1.3)
- Force HTTPS redirect
- HSTS configuration
- OCSP stapling
- Origin certificate check — whether this site validates HTTPS origin certificates. This is the site default each origin pool's **Verify origin certificate** inherits (see [Origin Pools](#origin-pools)); the Basic page shows it as a read-only card with a link here, so the SSL page is the only place to edit it

> **Upgrading from an older release?** The first time you save the TLS settings
> after the upgrade, the legacy certificate/key copy stored directly on the
> site's SSL row is cleared when a managed certificate is selected — the
> selected certificate is now always what the edge serves. If you may need to
> roll back, back up the `site_ssl` table first (see the
> [deployment guide](deployment.md#upgrade-procedure)).

### Sharing certificates between sites

One wildcard or multi-hostname certificate can serve several sites. The
certificate keeps a single **owner** (the site it was created under); other
sites *link* to it to serve it:

Managed from the site's **SSL Settings** page (certificate picker): the
"link a certificate" option lists what is available, and linked certificates
appear in the same picker marked as shared.

- **Linking** lists the certificates owned by sites you can read that this
  site does not own or use yet. Linking fails with
  `certificate already belongs to this site` if the site already owns it.
  Linking a certificate that is already linked is idempotent: the call
  returns `200` with the fully decorated certificate (the first creation
  answers `201`) instead of the old `409` — only a site that *owns* the
  certificate is still refused with `409`.
- **Selecting** a linked certificate works exactly like an owned one — the
  SSL settings picker shows both, with the shared ones labelled.
- **Unlinking** is refused while the certificate is the site's current TLS
  certificate (`certificate is the site's current TLS certificate; clear or
  replace it first`) — otherwise the site would silently lose HTTPS. Clear the
  certificate selection (or pick another one) first.
- **Fallback serving**: a site that has not selected a certificate and has no
  inline PEM, ACME order or self-signed certificate serves the linked
  certificate that covers the most of its hostnames automatically — one that
  covers none is skipped with a warning rather than served. The injected
  certificate never renews anything: ACME stays off for a site serving a
  certificate it does not own.

**Why the owner model matters:** renewals, ACME bookkeeping and configuration
backups are tied to the owning site, so a certificate is renewed once and
every linked site picks up the new material. **Share deliberately**: a linked
site serves the same hostnames, so only link a certificate to sites that are
legitimate bearers of those hostnames.

### Certificate Status and Events

Agents report each site's certificate status in every heartbeat, shown on the SSL/TLS page:

- **Valid**
- **Expiring soon** — the certificate lapses within 48 hours
- **Expired**

The SSL/TLS page also keeps an event log per certificate — created, renewal requested, renewed, failed, deleted, plus raw ACME output — so issuance attempts can be followed from the console.

## Request/Response Rewriting

### Rewrite Rules

```
Sites → [site] → Rewrite Rules → Add
```

A rewrite rule has a **direction** (`request` or `response`), an optional
**condition** (Cloudflare-style expression; built with the visual builder or
written as an expression) and an ordered list of **operations**. Request rules
run during the request phase; response rules run during the response phase and
can additionally match on `http.response.status`. The condition builder offers
only fields valid for the chosen direction, and switching the direction clears
conditions and operations that are not valid for it — the console tells you
with a toast. `replace_body` is offered for response rules only.

Operations (the console's types):

| Operation | Direction | Effect |
|-----------|-----------|--------|
| Set / add / remove header | both | Modify a request or response header |
| Set path | request | Replace the whole forwarded path with a literal value |
| Regex replace path | request | Rewrite the path via a regex (`$1` / `${name}` capture groups in the replacement); **every match** is replaced |
| Set / remove query param | request | Set or remove a query-string parameter |
| Replace body | response | Replace text inside the response body (see below) |

**Replace body** options:

- **Match mode**: literal text or a regular expression. With a regex the
  pattern must compile (validated with a `400` at save time) and the
  replacement may reference capture groups with `$1` / `${name}`.
- **Greedy** (default on): replace every match; off = only the first
  occurrence.
- Size caps: pattern at most 512 bytes, replacement at most 2048 bytes.

Examples:
- Strip `/api/v1` prefix: regex replace path, `^/api/v1/(.*)` → `/$1`
- Add security headers: set header `X-Frame-Options: DENY`
- Remove `Server` header from responses: remove header `Server`
- Drop a tracking parameter: remove query param `utm_source`
- Swap a brand string in HTML responses: replace body, literal, `Acme Corp` → `PingWAF`

### Response Body Replacement

Beyond headers, a site can rewrite strings inside response bodies. Configure
it from the site's **Rewrite Rules** page (the card sits under the rule list)
or via `PUT /api/v1/sites/{site_id}` with a `response_replace` object — see
the [API docs](api.md#put-sitessite_id) for the full field list:

```json
{
  "path": "^/status",
  "status_codes": [200],
  "rules": [
    { "pattern": "Google", "replacement": "PingWAF", "is_regex": false }
  ]
}
```

| Field | Meaning |
|-------|---------|
| `path` | Regex the request path must match; empty = every path |
| `status_codes` | Status codes to rewrite; empty = all. Each entry 100–599 |
| `rules` | Up to 20 replacements, applied in order |
| `rules[].pattern` | Text (1–512 bytes) matched literally, or as a regex when `is_regex` is set (regex must compile) |
| `rules[].replacement` | Substitution text (1–2048 bytes); with `is_regex`, `$1` / `$name` refer to capture groups |
| `rules[].is_regex` | Compile `pattern` as a regex instead of a literal |

Two hard constraints, enforced with a `400` at save time: a rule's pattern
or replacement may **not contain a single quote**, and neither side may be
empty. Clearing the policy is done with `null` — or an empty `rules` list,
which counts as "no policy".

**Compressed responses.** The replacement understands `gzip` / `x-gzip`,
`deflate` (zlib wrapper, with raw deflate tried as a fallback), **`br`**
(brotli) and **`zstd`**: the body is decompressed, rewritten and recompressed
with the same algorithm, so `Content-Encoding` is preserved. If your origin
serves brotli- or zstd-compressed responses — previously passed through
untouched with no replacement — they are rewritten like the rest now. The
limits are the same for every supported encoding: decompressed bodies are
capped at **8 MiB** (anything larger is passed through as the original
compressed bytes), and decompression failure, recompression failure or **no
match at all** output the original compressed bytes — with no match the output
is byte-identical to the source, so the `ETag` and integrity digests stay
valid. When the plugin does take a response over, the usual header changes
apply even if the body fell back to the original bytes: `Content-Length`
removed, the response switched to chunked, strong `ETag` weakened to `W/`,
`Content-MD5` and `Digest` removed, `Vary: Accept-Encoding` added.
Multi-value encodings (e.g. `gzip, br`) are still passed through untouched,
no replacement.

## Custom Error Pages

```
Sites → [site] → Error Pages → Add
```

| Field | Description |
|-------|-------------|
| Status code | HTTP status to customize (e.g., 403, 429, 502) |
| Content type | `html`, `json`, or `text` |
| Body | Custom response content |

### Unmatched Host error pages

```
Settings → Error pages → Unmatched Host (unmatched_host)
```

Error pages carry a **scope**. `site` (the default) pages replace error
responses of sites that matched; `unmatched_host` pages answer requests that
reached the gateway but whose `Host` header matched **no** configured site —
someone poking at the bare IP + port, or a hostname that was never configured
(or has since been removed).

When to use one: to give scanners, stale DNS entries and direct-IP probes a
branded answer instead of the gateway's plain default 404.

- **Status codes**: an `unmatched_host` page may only use `404`, `421` or
  `503` (the dashboard defaults to 404); other codes are rejected. One page
  per status, per scope.
- **Template variables**: only `{{ host }}` (the requested Host, HTML-escaped
  before it reaches the template) and `{{ request_id }}`. No request header is
  ever reflected into the template. Responses are sent with
  `Cache-Control: no-store`.
- **Default behavior**: with no enabled `unmatched_host` page configured, the
  gateway keeps its plain default 404 — configuring one is opt-in.
- **Matched sites are never affected**: the fallback location sits below
  every site's own locations, so a request for a configured hostname resolves
  to that site as usual.

## Log Management

### Viewing Logs

**Security Events**: `Logs → Security`
- Filtered by site, rule, action, time range
- Shows matched rule, client IP, request details

**Access Logs**: `Logs → Access`
- All proxied requests with timing information
- Filter by status code, path, client IP
- Request headers and a body preview (first 1 KiB) are stored in PostgreSQL, so requests can be inspected from the dashboard without Elasticsearch
- Static-asset requests (path ending in a configured suffix) that triggered no security rule are not recorded when the deployment-wide filter is on — see [Log Retention](#log-retention). Requests that did trigger a rule always appear here

#### Unassigned (no Host header) traffic

Requests that arrive at a bare IP with no `Host` header (HTTP/1.0 clients,
health probes, scanners) are proxied by the default site and their managed-rule
protection stays active, but they cannot be attributed to any site. Both log
views record them with a null site — shown as an **unassigned** badge in the
global log views — so they remain traceable:

- The site filter offers an **unassigned traffic** option (or search
  `site:none`) to list only these requests.
- Search `type:` in Security Events narrows by event type (`managed`, `waf`,
  `ip_geo`, `bot`, `rate_limit`, `challenge`).

#### Flushing agent logs

Agents batch their logs and ship them periodically. When you are watching for
an event that just happened (an incident drill, a live test), the
**Flush logs** action on the log page — `POST /api/v1/agents/flush-logs` —
tells every connected agent to ship its in-memory buffer immediately instead
of waiting out the batch interval.

- The command goes to **connected agents only** and is never queued for
  offline ones; the response `{ "delivered": N, "offline": M }` splits the
  fleet for you. `delivered` means the command reached the agent — it does not
  guarantee the entries are stored yet.
- Logs dropped while an agent was offline are unrecoverable: the agent's log
  buffer holds at most 2048 entries and older entries are discarded under
  pressure. Flush does not resurrect them.

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

How long logs are kept — and which access-log rows are skipped — is a
deployment-wide decision, configured under **Settings → Log retention**
(administrators only):

| Setting | Description |
|---------|-------------|
| Access log retention | Days an `access_logs` row is kept, 1–3650 (default 180) |
| Security event retention | Days a `security_events` row is kept, 1–3650 (default 180) |
| Skip static access logs | Skip access-log rows for static-asset requests that triggered no security rule (default on). Requests that did trigger a rule are always logged, and security events are never filtered |
| Static suffixes | Path suffixes counted as static assets, matched case-insensitively without the leading dot |

A fresh deployment starts with 18 built-in suffixes (`css`, `js`, `mjs`,
`map`, `png`, `jpg`, `jpeg`, `gif`, `webp`, `avif`, `svg`, `ico`, `bmp`,
`woff`, `woff2`, `ttf`, `otf`, `eot`). Document-like extensions such as
`json`/`html` are deliberately not included — APIs commonly answer with them,
and skipping them would hide real traffic. The list accepts at most 64
entries, each 1–10 lowercase letters or digits; saving an empty list with the
switch on skips nothing. Saving any change pushes the new filter to every
agent with the next configuration sync (the control plane notifies them
immediately); agents that have not received the setting keep logging
everything.

Purge old logs manually:
```
Logs → Purge (select age threshold)
```

## Account & User Management

**Account** is a top-level menu, separate from Settings. It holds the signed-in
user's profile, password, API keys, and — for administrators — the user list.

### Profile & Password

Update your display name and email, or change your password (current password
required; the new one must be at least 8 characters). Changing the password,
or the email you sign in with, signs you out everywhere — including this
browser — so you are asked to sign in again. Your personal API keys live on the
same page: any account can mint keys for itself, but only an administrator's
key can carry more than `read` (a non-administrator's request is silently
narrowed to `read`).

### User Management (administrators)

Administrators see a **Users** card on the Account page:

- **Create** accounts with an email, initial password and role
- **Disable** accounts — a disabled account cannot sign in, and its existing
  tokens are refused on every request
- **Change role** or profile details

Roles:

| Role | Capabilities |
|------|--------------|
| `admin` | Read and write everything, manage users |
| `auditor` | Read-only across every resource — for review/compliance access |
| `viewer` | Read-only |

Self-lockout is prevented: an administrator can neither downgrade nor disable
their own account, and the last enabled administrator can never lose admin
access.

### Forced First-Sign-In Password Change

Accounts seeded with the shipped default password — the first administrator on
a fresh deployment, and any account created with `pingwaf user add-admin` —
start with a `must change password` flag. On the first sign-in the console
blocks the whole UI behind a change-password dialog, and the server refuses
every authenticated request except the auth endpoints until the password is
replaced. Changing the password clears the flag and revokes every session, so
the console sends you back to the login form.

### JWT Secret Provisioning

On the very first boot, if no JWT secret is configured (via
`PINGWAF_JWT_SECRET`), the control plane generates a random 256-bit secret and
persists it in the database; every later boot reuses it. This removes the
historical default-secret foot-gun in open-source deployments: without an
explicitly configured secret, tokens are still signed by a unique per-install
key. An explicitly configured secret always wins.

## Agent Management

### Adding Agents

1. Generate an API key: **Account → API keys → Create**
2. Install the agent on the edge server:
   ```bash
   pingwaf agent --server-url https://control-plane:9090 --server-ca-cert /var/lib/pingwaf/data/grpc-ca.pem --api-key YOUR_KEY
   ```
3. The agent appears automatically in **Agents**

### Monitoring Agents

The Agents page shows:
- Display name (an optional operator label — left empty, the hostname is shown; rename via `PATCH /api/v1/agents/{id}`, search matches the label, hostname and IP)
- Connection status (online/offline/degraded)
- Last heartbeat time
- System info (hostname, OS, CPU, memory)
- Agent version
- Pending commands

**Where the CPU number comes from.** The fleet view
(`GET /api/v1/system/nodes`) labels each agent's CPU figure with a
`cpu_source` field, and the three values read differently:

- `probe` — a live reading from the agent's host probe (~5 s delta sample).
  Comparable across agent versions; values can move sharply between polls.
- `heartbeat` — the node has **not reported a probe sample yet**, so the
  figure comes from the heartbeat. For an agent that has not been upgraded to
  report probes, that is an **average since boot**, not the current load.
- `none` — neither source produced a reading (`cpu_percent` is `null`).

When graphing or alerting on node CPU, group by `cpu_source`: `probe` and
`heartbeat` figures are different measures and must not be mixed in one
series.

### Agent Commands

Push commands to connected agents:
- **Reload Config**: Force immediate config sync
- **Block IP**: Temporarily block an IP address
- **Unblock IP**: Remove a temporary block
- **Purge Cache**: Clear local cache
- **Restart Agent**: Graceful agent restart

## Access Control

Per-site IP rules and geo restrictions live on one page:

```
Sites → [site] → Access control
```

Switch between the **IP rules** and **Geo restrictions** tabs with `?tab=ip` / `?tab=geo`; links to the former separate IP-rules and geo pages redirect here automatically.

### IP Rules

| Field | Description |
|-------|-------------|
| Rule name | Label for the rule (e.g., "Block scraper subnet") |
| Target | A manual **IP / CIDR** list, or an **IP group reference** whose ranges always follow the group's latest contents |
| IP/CIDR | Address or range (e.g., `192.168.1.0/24`) — manual mode only |
| Action | `block`, `allow`, `challenge`, `js_challenge`, or `basic_auth` |
| Note | Why this rule exists (audit) |
| Priority | Lower values are matched first |

IP rules are evaluated before the WAF. Bulk import accepts one IP or CIDR per line.

### Geo Restrictions

Block or allow traffic by country and ASN:

- **Mode**: `block_list` (block specific countries) or `allow_list` (allow only specific countries)
- **Countries**: ISO 3166-1 alpha-2 codes (e.g., US, CN, DE)
- **Blocked ASNs**: deny entire autonomous systems (e.g., `AS13335`)
- **Block unknown locations**: deny traffic whose location cannot be determined
- **Action**: how matched traffic is handled

A "Requests by country" chart (last 24 hours) sits next to the policy editor.

### Global IP Groups

**IP Groups** (in the sidebar) manages named collections of IP ranges, either **global** (applied to all sites) or **per-site** (assigned to selected sites). Each group has an action: `block` or `allow`.

Groups can subscribe to an external list instead of (or in addition to) manually entered ranges:

| Field | Description |
|-------|-------------|
| Subscription Source URL | URL of the list to fetch (e.g., `https://example.com/ip-list.txt`) |
| Refresh interval | Manual only, hourly, every 6 hours, daily, or weekly |

A background scheduler refreshes enabled subscriptions when their interval elapses; use **Sync now** to refresh immediately. A failed sync keeps the previous ranges and records the error, which is shown on the group's row as the last sync error.

One group, **Cloudflare**, is seeded on first boot: the official Cloudflare IP ranges from `https://api.cloudflare.com/client/v4/ips`, refreshed daily. It starts disabled — enable it once you know how it fits your traffic.

Site IP rules can reference a group instead of enumerating ranges; see [IP Rules](#ip-rules) above.

### Path Scopes for Access Restrictions

All four access-control settings — **IP rules**, **geo restrictions**,
**basic auth** and **OIDC** — accept an optional path scope made of two
fields:

| Field | Meaning |
|-------|---------|
| `path` | The paths the setting applies to. Empty (the default for existing settings) = the whole site. |
| `match_type` | How `path` is interpreted: `prefix` (default) / `exact` / `wildcard` / `regex` — the same semantics as [Path Exceptions](#path-exceptions). An empty `path` is always stored as `prefix`. |

Per setting, a non-matching path means:

- **IP rules**: the rule does not apply — this works for the `allow`, `block`, `challenge` and `basic_auth` actions alike.
- **Geo restrictions**: the geo policy is skipped entirely.
- **Basic auth**: for the site-wide switch, no credentials are asked for outside the gate's scope. An IP or geo rule with the `basic_auth` action is scoped by its own path: once it matches, the request must authenticate even when the gate's scope names another path, so a matched rule is never silently turned into a bypass.
- **OIDC**: the path is fully public — no redirect to the identity provider, no session check. The OIDC callback, logout and `both`-mode provider entry stay reachable regardless of the scope.

Validation matches path exceptions: a `regex` scope is compile-checked when saved (invalid patterns are rejected with `400`), and agents that predate a `match_type` value degrade it to `prefix` — no visible change for empty paths, but a `regex` scope takes effect only after the agents are upgraded.

Examples:

- Ask for basic auth only under `/admin`: set the basic-auth `path` to `/admin` with `match_type: "prefix"`.
- Restrict a country block list to the API only: set the geo `path` to `/api/*` with `match_type: "wildcard"`.

## Bot Protection

```
Sites → [site] → Bot Protection
```

| Setting | Description |
|---------|-------------|
| Enable bot protection | Turn classification on or off |
| User-Agent analysis | Inspect the User-Agent header to classify traffic |
| Bot action | `block`, `challenge`, or `log` — applied to traffic classified as a bot (non-browser user agents) |
| Known bots | Whitelist of good crawlers that always pass |

How a request is classified:

1. Verified bots whose User-Agent matches a **known bots** entry pass.
2. Real browsers (known browser user agents) pass.
3. Everything else receives the configured **bot action**.

Known-bot entries are case-insensitive substrings matched against the User-Agent header (e.g., `Googlebot`); the console offers quick-add suggestions for common crawlers.

JavaScript detection, TLS fingerprinting and behavioral analysis appear in the console as *Coming soon* — they are roadmap items, not implemented yet.

## AI Assistant

```
Settings → AI assistant · sidebar assistant icon
```

The console embeds an AI assistant that answers questions about your deployment by calling the same MCP tools external clients use (see below). Configure an OpenAI-compatible provider under **Settings → AI assistant**:

| Setting | Description |
|---------|-------------|
| Enable assistant | Master switch; the AI Assistant page prompts for setup while it is off |
| Base URL | OpenAI-compatible endpoint, e.g. `https://api.openai.com/v1` |
| Model | Model name, e.g. `gpt-4o-mini` |
| API key | Provider credential, stored server-side and shown masked afterwards |
| Temperature | 0–2; lower answers change less between runs |
| Max tool rounds | How many tool-call rounds one answer may take (1–10) |
| System prompt | Prefilled with the built-in default; clear and save to reuse it |
| Allow write tools | Off by default; enables the tools that change state (observation mode, site enable/disable) |

**Test draft** verifies the provider without saving. Once enabled, ask a question on the AI Assistant page and watch tool calls appear inline as the answer streams. Conversations are stored server-side, so history survives reloads, and the assistant is administrator-only.

## MCP Server

```
Settings → MCP server
```

Every deployment hosts an MCP (Model Context Protocol) endpoint at `/mcp` on the console origin, so external AI agents — Claude Code, IDE copilots, scripts — can inspect and operate the firewall. The Settings card shows the live endpoint, the tool inventory (write tools tagged), and copy-paste client snippets.

To connect a client you need an API key (**Account → API keys**) with the `read` permission; add `write`, whose owner must be an administrator, for the two tools that change state. Then paste the card's JSON into the client, or for Claude Code run:

```bash
claude mcp add --transport http pingwaf https://waf.example.com:9080/mcp \
  --header "Authorization: Bearer pwk_YOUR_KEY"
```

Read-only credentials see only the read tools; administrators with a `write` key may also call `set_observation_mode` and `set_site_status`. The endpoint sits behind the same API protection as the console (access log, IP allowlist, WAF), so it obeys whatever **Settings → API protection** allows.

When the console is behind a load balancer or reverse proxy, enable **Settings →
API protection → Trust reverse proxy headers**, name the forwarded header
(`x-forwarded-for` by default), and list the proxy addresses/CIDRs under
**Trusted proxy ranges**. The 9080 access log and WAF then record the client
address from that header; the IP allowlist deliberately keeps using the TCP
peer so a spoofed header cannot widen access. An empty range list trusts no
peer, even with the switch on.

## Config Versions

**Config versions** in the sidebar records a version of the configuration on
every change and lets you roll one back. Administrators only: other roles are
redirected away from the page.

- **The list** shows each version's scope (site domain, global settings or full
  deployment), config hash, source, operator and row counts. Filter by scope
  and date, and page through the history. Versions older than 30 days and
  entries beyond the newest 50 per scope are pruned at startup, daily and on
  demand with **Prune old versions**.
- **The detail view** compares a version with the one before it, table by
  table, so you can see exactly which rows a change touched before deciding.
- **Rollback** archives the live state first, restores the selected snapshot
  and pushes it to the agents, behind a confirmation. It is recorded as a new
  version and can be rolled back in turn.

### Encrypted configuration backups

The same page also manages complete encrypted backups:

1. **Add a target** (optional): an S3-compatible endpoint/bucket or a GitHub
   repository. Credentials are sealed with `PINGWAF_SECRET_KEY` and never
   returned by the API. Use **Test connection** before the first upload.
2. **Create encrypted backup now** and enter a backup password of at least 12
   characters. The password is used only for that operation and is not stored
   or logged. Choose a remote target or keep the backup local, then download
   the `.pwb.json` envelope for safekeeping.
3. **Upload backup file** to restore. Enter the same password and review the
   dry-run: before/after rows, additions, removals, changes, owner mappings
   and warnings. No secret values are shown. A deployment restore requires a
   complete schema-2 snapshot; an invalid password, checksum or scope is
   refused.
4. **Confirm full replacement** only after reviewing the impact. PingWAF
   creates a fresh encrypted pre-restore backup and applies the change in one
   serializable transaction. If anything fails, no partial configuration or
   pre-restore record is left. On success the pre-restore backup remains
   available as a way back, and runtime refresh failures are reported as
   warnings rather than hidden.

Backups include the restorable database configuration (sites, rules, shared
resources, OIDC, visitors and grants, notifications, AI, cache and
certificates). They exclude console identities, agent state, logs, temporary
authentication state, environment variables and local GeoIP files. Restoring
sealed AI or notification credentials requires the current instance's
`PINGWAF_SECRET_KEY`. Backup records follow the same 30-day and per-scope
retention policy, and abandoned uploads become retryable after their lease
expires.

## Internationalization (i18n)

The dashboard supports multiple languages:
- English (default)
- 中文 (Chinese)
- 日本語 (Japanese)

Language is auto-detected from the browser. Override via the language selector in the dashboard header.
