# PingWAF User Guide

## Getting Started

### First Login

1. Open the dashboard at `https://your-server:9080` (self-signed certificate
   until you upload a real one under Settings → Control plane HTTPS)
2. Log in with the seeded credentials:
   - Email: `admin@pingwaf.local`
   - Password: the one the deployment was seeded with — the `ADMIN_PASSWORD` you configured, or the random password the installer printed once. Only a bare binary started without `PINGWAF_ADMIN_PASSWORD` falls back to the shipped `pingwaf123`; compose and the installers have no default password
3. **Change your password when prompted**: the console blocks the whole UI
   behind a change-password dialog on the first sign-in (the server refuses
   every authenticated request except the auth endpoints). Changing the
   password revokes all of that account's sessions, so the console signs you
   out and asks you to sign in again with the new password — the flag is
   stored per account, not per browser

### Dashboard Overview

The dashboard provides:
- **Analytics**: Request trends with outbound bandwidth on one shared time axis, per-site traffic over time, geographic distribution (an interactive globe plus a country ranking that drills into the access logs), the top networks by ASN (name, `ASxxxxx`, requests and share of the window), top rules triggered, status codes, client-type mix (desktop / mobile / AI bots / other)
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

**Built-in rules**: each of the built-in managed rules has its own Block / Log-only switch, independent of the category switches. Seeded rows that mirror a managed rule are controls for that rule, not separate custom rules: disabling one (`enabled=false`) removes that rule from the compiled rule set on this site — it is no longer evaluated, scored or logged — and setting it to Log-only downgrades it to monitor. Their expression/action are owned by the managed ruleset and cannot be edited through the custom-rule API. These rows — and a rule group that contains any of them — cannot be deleted either: the row is the only record of the override, so removing it would silently put the rule back in blocking mode. Batch buttons switch all rules at once. Because a family's rules (SQLi, XSS, RCE score gates) and its category switch control the same signature surface, downgrading a category also flips its family rules to log-only; switching the category back removes those per-rule downgrades, resetting any individual overrides made in between.

Disabling a managed rule is **not** a way to stop that attack class from being blocked: the signature layer it shares with the other rules of the same family keeps running, so a disabled `SQL 注入检测` row still sees SQLi payloads blocked by the `libinjection-sqli` signature. Use **Log only** (`monitor`) when the goal is to observe a family without blocking; use **Off** at the site level only when inspection should stop entirely.

**Deep inspection** enables the strict managed rule set plus deep request-body inspection. Higher interception rate at a proportional performance cost. Custom rules receive the same request-body evaluation while deep inspection is on, so body-carrying bypass attempts are caught by user-defined expressions too. On the page the two posture switches — **Under-attack mode** and **Deep inspection** — sit side by side on one row.

**Backend stacks** (`java`, `php`, `python`, `node`) downgrade stack-specific detections the same way; language-agnostic detections are never affected by a stack switch. Stack switches are collapsed by default. Ticking a stack makes its detections log-only; it does not mean "my site runs on this stack". Tick one only while its rules are blocking legitimate traffic — every stack is always evaluated, and there is no switch that turns off the stacks a site does not use.

A site without explicit settings inherits the aggregate of its enabled rules: `block` when any rule blocks, otherwise `monitor`. It never silently lands on `off` — an unconfigured site keeps detecting and logging, and only an explicit **Off** (or the engine switch) turns inspection off.

The **WAF engine** switch covers the rule engine only — managed rules, custom rules and the monitor downgrades above. IP / geo access rules, bot protection, AI crawl control, CC protection and rate limiting are separate layers that keep enforcing while the engine is off; turn those off on their own pages.

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
| Skip layers | `*` (every detection/rate/challenge layer), or any of `waf`, `bot`, `access`, `basic_auth`, `mtls`, `ai_bot`, `rate_limit`, `challenge`. `*` never includes `mtls` or `basic_auth`; name those explicitly if an exception must bypass authentication. `challenge` waives **every** challenge source (WAF rule/scoring challenges, CC & rate-limit challenge actions, bot protection, IP/geo rules, the standalone challenge plugin) while blocks still enforce |
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

Scope matching uses a canonical path: percent-decoding, `//` collapse and
`.`/`..` resolution run before the exception is matched, so `/api/../admin`
cannot satisfy a `/api` exception. The original path is still what gets
logged and proxied.

**What an exception cannot bypass**, no matter which layers it names: a paused
site (maintenance page), a fail-closed edge, and an active dynamic IP ban —
those checks run before the exception and always enforce.

Other failures you may see when saving: a duplicate path on the same site
(`a path exception for '<path>' already exists on this site`), a site at the
64-exception limit, an unknown skip-layer token, or a wildcard path that does
not end in exactly one `*`.

**Why a save is rejected for agent capability:** path scopes are interpreted
by the edge agent, so the control plane refuses — with a `400` naming the
offending agent and version — when the agents serving the site cannot execute
what you are saving. This applies to **all five panels** that can carry a
scope: path exceptions, IP rules, geo restrictions, basic auth and the OIDC
configuration. For path exceptions, a `regex` match type requires every
serving agent to be **≥ 0.32.20** (`path scope is not supported by agent
<name> (<version>); regex scope requires agent >= 0.32.20`), and saving any
exception (its `skip_layers` is always non-empty) requires **≥ 0.32.21**
(`path scope is not supported by agent <name> (<version>); canonical path
matching requires agent >= 0.32.21`), because older agents match scopes
against the raw request path and a dot-segment request could bypass the
exception — and its WAF checks — entirely. When both apply, the stricter bar
(0.32.21) is the one named in the message. For the other four panels, **any
non-empty path scope** — regex or not — requires **≥ 0.32.20**
(`path scope is not supported by agent <name> (<version>); path scope
requires agent >= 0.32.20`): old agents have no path field at all, so a
scoped `allow` rule would become allow-everywhere and a scoped basic-auth
gate would ask for credentials on the whole site. An empty path (whole site)
is never gated.

Re-enabling counts too: turning a disabled path exception back on
(`enabled` `false` → `true`) re-arms its stored scope, so the same gate runs
and the save is rejected if the fleet cannot execute it. Sending
`{"enabled": true}` to an already-enabled exception is a no-op and is never
blocked.

Restoring is gated the same way: rolling back a config version or restoring
an encrypted backup whose snapshot contains a scope the current agents cannot
execute is refused outright with a `400` naming the site, the agent, its
version and the missing capability — nothing is partially applied. The fix is
operational: upgrade the agents first (or clear the offending scope's `path`)
and retry. A site with no bound agent is never gated, and an agent whose
version cannot be read is treated as unsupported (fail-closed).

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
| Base difficulty | Proof-of-work leading zero bits a normal visitor solves (default 20) |
| Max difficulty | Ceiling the difficulty rises to under pressure (default 24) |
| Challenge limit / min | Challenge pages issued per IP per minute (default 120) |
| Verify limit / min | Solution submissions accepted per IP per minute (default 300) |

- **Passive**: Challenge only when suspicious behavior is detected
- **Active**: Challenge all requests matching configured criteria

#### Adaptive difficulty

Every challenge is issued with its own difficulty, stored server-side and
checked on submission — the client never supplies it and a settings change does
not invalidate pages already in flight. The difficulty starts at the base and
rises per host+IP when that client keeps failing verification (one step per
failure, forgotten after a quiet minute) or sends more requests than the rate
threshold (one step per threshold-worth above it). It stops at the max, and a
successful verification drops it back to the base.

Set **Max difficulty** equal to **Base difficulty** to pin the work at a fixed
value. Both must be 4–28; max may not be below base.

#### Per-IP challenge limits

Both limits are per host+IP over a 60-second window, and both apply to every
challenge the site can mint — the CC gate and a WAF rule / IP-geo rule / bot /
rate-limit challenge alike, which all use the site's difficulty and the same
budget. Above the issuance limit the visitor gets a `429` rate-limit page
instead of a challenge page (it carries `x-pingwaf-mitigated: block`), so the
challenge endpoint cannot be used as an amplifier. Above the verification limit
the submission is refused with `429` before any work is done, and the pending
challenge is left intact — a legitimate visitor can still complete it once the
window rolls over. Both refusals appear in the access log.

The bounded counter table holds 8192 host+IP keys per window. When it is full of
*other* clients, issuance keeps working (the page is minted without a budget
entry) so an attacker cannot lock every new visitor out, while verification
fails closed so a full table never becomes an unmetered guessing channel. Raise
the limits for sites whose visitors share one NAT address; the counters are per
edge process, so N instances allow N × the configured value.

### Challenge page template

The challenge page is customized from the same global template editor as the
error pages: **Settings → Custom Pages → Challenge page** (`scope =
challenge`, fixed status 503). It is a **separate scope**: editing it does not
change the site or unmatched-host 503 error pages, and those pages cannot
replace a challenge response. One template applies to every site, matching the
existing global error-page model. The template is presentation only: the
fingerprinting, proof-of-work, verification and redirect JavaScript is compiled
into the binary and injected at `{{ challenge_scripts | safe }}`; the template
cannot replace it.

The template must:

- inject the compiled scripts with `{{ challenge_scripts | safe }}` (without
  `| safe`, Tera HTML-escapes them and the page is rejected);
- contain the elements `countdown`, `title`, `subtitle`, `status`,
  `spinnerWrap`, `progressBar` and `progressFill` (the compiled script looks
  them up by id; single- or double-quoted `id` works);
- not contain its own `<script>` tags (only the compiled scripts may run).

Available variables: `challenge_scripts`, `brand_name`, `request_id`,
`difficulty`, `verify_endpoint`, `original_url`. `brand_name` and `request_id`
are HTML-escaped by Tera.

A template that fails to render, omits the `| safe` filter, loses a required
DOM id or contains `<script>` falls back to the built-in page with a warning.
The challenge page applies to the JS, managed and interactive challenge types
because they share the same renderer.

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

### Image Optimization

**Site → Cache → Image optimization** switches on the data plane's
`image_optim` plugin for this site. When enabled, a JPEG or PNG response is
re-encoded into the first of the site's **output formats** (`WebP`, `AVIF`)
that the request's `Accept` header lists — and only then; a client that accepts
neither gets the original bytes. The qualities (`PNG`, `JPEG`, `AVIF`, 1-100)
and the AVIF encoder speed (1 = slowest and smallest, 10 = fastest) tune the
trade-off per site.

Three properties are worth knowing before turning it on:

- **The cache keeps one variant per accepted format.** The plugin extends the
  cache key with the image types the request's `Accept` header lists, so a
  client that accepts WebP/AVIF and one that accepts neither are different
  entries: the first is stored optimized, the second keeps the origin bytes.
  The plugin runs at the upstream stage, so what is stored is the response the
  client received — a later hit with the same accepted formats is served as
  stored, and a client with a different `Accept` set is a different key.
- **Only plain, sized image responses are touched.** The plugin acts on a
  `200` with `Content-Type: image/jpeg` or `image/png`, a `Content-Length` of at
  most 16 MiB, and a request whose `Accept` header explicitly lists the chosen
  format. Everything else — a chunked image (no declared length), a `HEAD`, a
  `206` range, a `304`, an image over 16 MiB, or a client that only sent
  `Accept: */*` — passes through with the origin's bytes and content type. The
  response gains `Vary: Accept`, so a CDN in front of the edge caches the
  variants apart. If a corrupt image fails to decode, the origin's bytes are
  sent rather than an empty body.
- **The binary must be built with the `imageoptim` feature** (the release
  artifacts and the Docker image are). A build without it accepts the policy but
  logs one warning per site and leaves responses untouched.

Turning the switch on does not rewrite entries cached before it: those were
stored under a key without the accepted-format suffix, so the next request
simply misses and fills the new variant.

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
  - Amazon Route 53 — `route53`

Route 53 uses the AWS SDK credential chain of the issuing process (an EC2
instance role on AWS, or the standard `AWS_*` environment/configuration). Set
`acme_dns_config` to `{"hosted_zone_id":"Z123EXAMPLE"}` to pin a public hosted
zone, or leave it empty to discover the closest public zone; discovery also
checks the zone's public NS delegation. A wildcard and its apex share one
`_acme-challenge` name, so a combined certificate keeps both TXT values until
the CA validates the order. Other values (`manual`, `digitalocean`, `cloudxns`)
are not accepted for managed DNS-01; rows written before this check are
migrated back to HTTP-01.

The control plane imports the issuer's local ACME state only when the entry is
ACME-managed, matches the certificate row's hostname set, and is currently
valid: expired, not-yet-valid, older-than-stored, and (unless
`PINGAP_ACME_STAGING` is set on the control plane) staging-CA certificates are
rejected with a warning instead of being pushed to agents.

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
- Self-signed certificate — PingWAF issues and renews a leaf covering the
  site's domain and alternate hostnames, and browsers warn because it is not
  publicly trusted. New sites start in this mode; uploading a certificate,
  selecting one, or configuring ACME replaces it automatically. Enabling HTTPS
  with no certificate source at all is refused (`HTTPS is enabled but the site
  has no certificate`) rather than serving a listener that cannot complete a
  handshake.
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

## Custom Pages

```
Settings → Custom Pages
```

The same global editor holds the error-response templates and the challenge
page. Each page carries a **scope**:

| Scope | Use |
|-------|-----|
| `site` | Error responses of matched sites (403, 429, 502, 503, 504, …) |
| `unmatched_host` | Requests whose `Host` matches no configured site |
| `challenge` | The JS / managed / interactive challenge page (fixed 503) |

| Field | Description |
|-------|-------------|
| Status code | HTTP status to customize (challenge is fixed at 503) |
| Content type | `html`, `json`, or `text` (challenge is always `text/html`) |
| Body | Tera template rendered by the edge |

The API and database keep the historical `error-pages` / `error_pages` names
for compatibility; the console calls the feature **Custom Pages**.

### Unmatched Host pages

```
Settings → Custom Pages → Unmatched Host (unmatched_host)
```

`site` (the default) pages replace error responses of sites that matched;
`unmatched_host` pages answer requests that reached the gateway but whose
`Host` header matched **no** configured site — someone poking at the bare IP +
port, or a hostname that was never configured (or has since been removed).

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

### Challenge page

```
Settings → Custom Pages → Challenge page (challenge)
```

Presentation-only template for the JS / managed / interactive challenge page.
The compiled challenge scripts are injected at
`{{ challenge_scripts | safe }}`; the required DOM ids and the no-`<script>`
rule are described under [Challenges](#challenges-cc-protection).

## Log Management

### Viewing Logs

**Security Events**: `Logs → Security`
- Filtered by site, rule, action, time range
- Shows matched rule, client IP, request details

**Access Logs**: `Logs → Access`
- All proxied requests with timing information
- Filter by status code, path, client IP
- Request headers and a body preview (up to 8 KiB by default — `--max-body-log-size`, `0` disables body capture) are stored in PostgreSQL, so requests can be inspected from the dashboard without Elasticsearch. Headers are kept **verbatim**, including `Authorization`, `Cookie` and `Set-Cookie`, so treat the log store as a credential store: restrict database/Elasticsearch access, encrypt their backups, and keep the retention window as short as investigations allow — see [Log Retention](#log-retention).
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
  buffer holds at most 2048 entries and new entries are discarded once it is
  full. Flush does not resurrect them. The node's detail panel on the
  **Agents** page shows how many entries it has discarded since its agent
  process started.

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

### Passkeys (WebAuthn)

Passkeys add a phishing-resistant way to sign in. Register one from **Profile**
(the account page at `/profile`, which also lists and renames existing
passkeys), then use the authenticator — Touch ID, Windows Hello or a hardware
security key — on the login page instead of typing the password. Passkeys
require a **secure context**: HTTPS under a hostname, or `localhost`. They do
not work over plain `http://` or a bare IP, and behind a TLS-terminating proxy
`PINGWAF_PASSKEY_ORIGIN` must name the public origin (see
[deployment.md](./deployment.md)). A lost device is not a lockout: sign in with
the password, delete the old passkey on **Profile**, and register the new
device. Administrators can turn the feature off deployment-wide with
`PINGWAF_PASSKEY_ENABLED=false`.

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

### Assigning sites to an agent

By default an agent receives every site owned by its API key — the legacy
behaviour, and the highest-memory mode. On a multi-site fleet, narrow it:

```
Agents → [agent] → Manage sites
```

- **All sites** — the default; the agent carries every site its key owner has.
- **Selected sites** — only the checked sites are pushed. The rule bundle,
  certificate set and per-site caches then scale with the selection instead of
  the whole account; an empty selection leaves the agent idle until assigned.

The assignment is validated against the API key's owner and takes effect
immediately for a connected agent; an offline node receives it on reconnect.
The same setting is available as `PUT /api/v1/agents/{id}/sites`.

### ACME issuer scope

An agent either **issues** certificates or only **serves** them. Only an issuer
receives the site's `acme_dns_config` (the Cloudflare/Aliyun/Tencent/Huawei
token) and only an issuer opens ACME orders; a consumer keeps serving the PEM
the control plane distributes and never sees the DNS credential. Route 53 is
different: its `acme_dns_config` holds only a hosted-zone ID, and the AWS
credentials come from the issuer's own runtime.

- The embedded all-in-one issuer is marked automatically (it registers over
  loopback).
- A remote agent that should issue must be marked explicitly:
  `PATCH /api/v1/agents/{id}` with `{"acme_issuer": true}`, or the toggle in
  **Agents → [agent]**.
- Mark consumer-only edges `acme_issuer=false` so a compromised edge cannot
  open orders or read the provider token. The console shows the scope in the
  agent detail panel.

A control-plane-only deployment with no embedded issuer has no issuer until one
is marked. A remote issuer that is marked (and assigned to the site) can return
its issued PEM over the authenticated `PublishCertificate` gRPC call: the
control plane validates the caller, the site assignment, the certificate's
hostname set, the key/leaf pair and the certificate's validity before promoting
it and fanning it out to every assigned agent. The embedded all-in-one issuer
keeps working through its local ACME state file; the two paths are idempotent.

### Per-certificate issuer

The agent-level flag decides whether a node may issue at all. To narrow one
certificate to one agent, set `acme_issuer_agent_id` on the certificate (the
"Issuer agent" selector in the certificate form, or the field on
`POST/PUT /api/v1/certificates`). When it is set:

- only that agent receives the certificate's ACME material;
- only that agent's `PublishCertificate` call is accepted for it;
- every other agent gets the consumer bundle and serves the distributed PEM.

Leave it empty to keep the agent-level scope (the embedded all-in-one issuer
and single-agent deployments need no designation). The designated agent must
already be marked `acme_issuer=true` and serve the certificate's site.

### Monitoring Agents

The Agents page shows:
- Display name (an optional operator label — left empty, the hostname is shown; rename via `PATCH /api/v1/agents/{id}`, search matches the label, hostname and IP)
- Connection status (online/offline/degraded). A node is **degraded** while its
  data plane could not apply the latest configuration; the detail panel names
  the failing reload step, and the node retries on its own
- Log entries the node discarded before the control plane stored them (detail
  panel, shown only when non-zero)
- Last heartbeat time
- System info (hostname, OS, CPU cores, host memory, agent binary memory)
- Agent version
- Pending commands

**Which address you are looking at.** A row carries three, and they answer
different questions:

- the address under the hostname (`ip_address`) is what the agent reports about
  itself;
- **Public IP / Private IP** (detail panel) come from the agent's own egress and
  LAN probes — a useful guess, empty when the probe failed;
- **Connection source** (`peer_ip`) is the address the control plane actually
  accepted the agent's stream from. It is the one value the agent cannot
  misreport, so it is what you need when a node sits behind NAT: the row shows
  `via <address>` whenever it differs from the reported one, and the detail
  panel adds how long ago that address was seen live.

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

**Binary memory vs host memory.** The detail panel shows memory twice, and
they answer different questions: **Host memory** is the node's RAM (capacity
at registration, usage once the host probe reports), while **Binary memory**
is the RSS of the agent process itself (`process_rss_bytes`) — the figure that
answers "how much memory is the WAF binary using". It shows `—` until an agent
build that reports the field connects: `null` means unknown, not zero. The
probe memory chart overlays the process RSS as a separate line on its own
byte axis.

### Agent Commands

Push commands to connected agents:
- **Reload Config**: Force immediate config sync
- **Block IP**: Temporarily block an IP address
- **Unblock IP**: Remove a temporary block
- **Purge Cache**: Clear local cache
- **Restart Agent**: Graceful agent restart

### Fleet versions

The System page (observability) now opens with a **Version** card: the control
plane's own build, the REST API prefix, and how many agents report that same
build. Agents on another version are listed by name and version, and agents that
have never reported one are counted separately.

This is the first thing to check when a rule behaves differently on one edge
than another: two builds can disagree on defaults, rule semantics or config
keys, and the card makes that visible without opening the inventory. Edges that
opt into [self-upgrade](deployment.md#agent-self-upgrade) close the gap
on their own; the card shows them converging.

## Tunnels (internal services without an inbound port)

A tunnel exposes a service that lives inside a private network — a NAS, an
intranet app, a database admin page — **without opening any inbound port and
without a public IP on that host**. It is the Cloudflare Tunnel model, wired
into the data plane: the connector dials out, the edge terminates the public
side, and every protection (WAF, rate limiting, challenges, cache, logs,
certificates) applies exactly as it does for a direct origin.

### When to use one

- The origin is behind NAT or a firewall you cannot (or will not) open.
- The origin's address is only meaningful inside that network
  (`10.0.0.5:8080`, `app.internal:8443`).
- You want the public entry point to stay on the WAF, not on the internal host.

A tunnel is **not** a VPN: it carries the traffic of the origin pools bound to
it, nothing else.

### Three states that are not the same

The console renders them separately, and most confusion comes from collapsing
them:

| State | Meaning | Where it comes from |
| --- | --- | --- |
| **Paired** | a credential exists, so the tunnel is usable | set when a connector redeems its pairing token |
| **Enabled** | the operator's per-tunnel switch | `PATCH /tunnels/{id}` with `status` |
| **Connector online** | a connector currently holds a control stream | reported live by the control plane |

*Paired* and *Enabled* with the connector offline is a working configuration
with a disconnected connector, not a broken tunnel; *Connector online* is
impossible without being paired.

### 1. Create the tunnel and pair the connector

**Agents → Tunnels → Create tunnel.** The console creates the tunnel and shows a
one-time pairing command:

```bash
curl -fsSL https://raw.githubusercontent.com/shuaiZend/PingWAF/main/install.sh \
  | sudo bash -s -- --mode tunnel \
      --server-url https://control-plane:9090 \
      --server-ca-cert control-plane.pem \
      --token <pairing-token>
```

Run it on a host inside the private network. It installs `pingwaf-tunnel`,
redeems the token for a long-lived credential, writes
`/etc/pingwaf/tunnel.toml` (`0600`) and registers
`pingwaf-tunnel.service`.

Without the installer, the same two steps by hand:

```bash
sudo pingwaf-tunnel pair \
  --config /etc/pingwaf/tunnel.toml \
  --server-url https://control-plane:9090 \
  --server-ca-cert /etc/pingwaf/tunnel-ca.pem \
  --token <pairing-token>
sudo pingwaf-tunnel run --config /etc/pingwaf/tunnel.toml
``` The token is shown **once** and expires (30 minutes by
default); minting a new one rotates the credential and disconnects the current
connector.

### 2. Enable the listener on the edge that will terminate it

The connector dials the edges, so at least one edge must accept tunnel
connections:

```toml
# pingwaf.toml on the public edge
[agent]
tunnel_addr = "0.0.0.0:9443"
```

or `--tunnel-addr 0.0.0.0:9443` / `PINGWAF_TUNNEL_ADDR`. Then give that edge a
**tunnel ingress address** — the public `host:port` connectors dial — in the
agent's detail panel (Agents → [node] → **Tunnel ingress**). The panel also
shows the listener state and the SHA-256 of the certificate connectors pin, and
it explains a listener that is configured but failed to bind (for example the
port is already in use).

The listener binds at startup: changing `tunnel_addr` needs an edge restart. The
ingress *address* and the tunnel policy are hot — they reach the connectors and
the edges through the next push (≤5s on a connected edge), so a new ingress
address or a re-paired credential does not need one.

**Tunnels → Settings** carries the deployment-wide limits: a master switch (off
closes live sessions on the next reload and stops connectors from redialing),
the pairing-token lifetime, streams per session, targets per tunnel, the session
liveness timeout, the heartbeat cadence and the session-token lifetime.

Open the tunnel port on the edge's firewall. Nothing needs to be open on the
private-network side.

### 3. Bind an origin pool to the tunnel

In the site's origin pool (**Site → Basic → Origin pools → edit**), choose the
tunnel in the **Tunnel** selector. The pool's node addresses are then resolved
by the connector inside the private network instead of by the edge:

```
pool "app"   tunnel = branch-office
  node 10.0.0.5:8080
```

The tunnel's target list is derived from the pools bound to it — there is no
second place to configure it.

### How a request flows

```
client ──TLS──► edge :443 ──► WAF pipeline ──► pool bound to the tunnel
                                                    │
                                         127.0.0.1:<bridge>
                                                    │
                                        TLS + multiplexed session
                                                    │
                                       connector (private network)
                                                    │
                                       10.0.0.5:8080 (the origin)
```

The bridge is an ordinary origin, so nothing in the request path is
special-cased: the WAF decides, the cache caches, the site's log records, and
the access log renders the origin as `tunnel://<name>/<target>` instead of a
loopback port. The connector only dials the target list the control plane
pushed (optionally narrowed by its local `allowed_targets`), so the edge cannot
make it reach an address outside the tunnel's policy.

### Verify it works

Three checks, in the order they fail:

1. **The connector is online** — **Agents → Tunnels**: the row shows *Paired*
   and *Connector online*, and the detail panel lists at least one edge under
   **Edges**. `pingwaf-tunnel status --config /etc/pingwaf/tunnel.toml` prints
   the same live state locally (the ingress endpoints with a session, the
   targets in force, and the last revocation reason).
2. **The edge offers an ingress** — the same detail panel shows the edge and
   the tunnel's **Targets**; an empty *Edges* list means no edge has the
   listener enabled or its ingress address set (step 2 above).
3. **The site answers through it** — request the site as a client would, with
   the site's own Host header:
   ```bash
   curl -sv -H 'Host: app.example.com' https://<edge-address>/      --resolve app.example.com:443:<edge-address>
   ```
   A `200` from the origin means the whole path works; a `502/503` means the
   edge has no session (back to check 1) or the connector cannot reach the
   origin (check the connector's log for `target is unreachable`).

The **Used by** list in the detail panel names the sites bound to the tunnel —
that is the blast radius of disabling or deleting it.

### No direct egress: going through a SOCKS5 proxy

A host that can only reach the internet through a SOCKS5 proxy (a jump host, a
corporate gateway) can still run the connector: point it at the proxy and
**both** channels — the control-plane stream and every edge session — go
through it. There is no separate switch per channel, and no silent fallback to
a direct dial: a host that needs the proxy cannot reach anything without it.

```toml
# /etc/pingwaf/tunnel.toml
socks5_proxy    = "10.0.0.1:1080"          # or socks5://10.0.0.1:1080
socks5_username = "connector"               # optional
socks5_password = "…"                       # optional
```

or on the command line / in the installer:

```bash
pingwaf-tunnel run --config /etc/pingwaf/tunnel.toml \
  --socks5-proxy 10.0.0.1:1080 --socks5-username connector

curl -fsSL …/install.sh | sudo bash -s -- --mode tunnel \
  --server-url https://control-plane:9090 --token <pairing-token> \
  --socks5-proxy 10.0.0.1:1080 --socks5-username connector
```

Notes:

- **Target names are resolved by the proxy** (SOCKS5 ATYP 3), which is the
  point in a network where local DNS is the broken part. A target that is an
  IP literal is sent as an address.
- Credentials may be embedded as `socks5://user:pass@host:port`; the explicit
  `--socks5-username`/`--socks5-password` (or the config keys) win, and the
  password is better supplied through `PINGWAF_TUNNEL_SOCKS5_PASSWORD` than on
  a command line that `ps` can read.
- The handshake (greeting, optional authentication, CONNECT) has its own
  timeout; a proxy that stalls fails the dial with a message naming the step
  (`refused every authentication method`, `rejected the username/password`,
  `connection refused`, `timed out`) instead of hanging the connector.
- The proxy is used for the connector's **outbound** dials only. It does not
  change how the edge reaches the origin (that is the tunnel's job).

### Connector commands and config

```bash
pingwaf-tunnel pair   --token <pairing-token> --server-url https://cp:9090 [--config PATH]
pingwaf-tunnel run    [--config PATH]
pingwaf-tunnel status [--config PATH]
```

| Key in `tunnel.toml` | Meaning |
| --- | --- |
| `tunnel_id`, `credential` | written by `pair` |
| `server_url`, `server_ca_cert`, `server_tls_name` | control plane and its TLS trust |
| `ingress` | `host:port[=sha256]` endpoints; filled from the control plane, can be pinned locally |
| `allowed_targets` | optional local allowlist, intersected with the policy (never merged) |
| `socks5_proxy`, `socks5_username`, `socks5_password` | egress through a SOCKS5 proxy |
| `session_token` | short-lived token for the edges, refreshed by the control plane |

`status` never opens a control-plane stream — that would take the session slot
of the connector actually serving traffic. It reads the state `run` publishes to
`/var/lib/pingwaf/tunnel/status.json` (connected ingress endpoints, the targets
in force, and the last revocation reason).

### Operations

| Action | Effect |
| --- | --- |
| **Re-pair** | Mints a new token and rotates the credential; the running connector is disconnected and must be paired again. |
| **Revoke** | Drops the credential immediately. Bound sites answer 503 until the connector is paired again. |
| **Disable** | Keeps the credential but stops accepting sessions (deployment switch or per-tunnel). |
| **Delete** | Removes the tunnel. Refused with `409` while origin pools still reference it; `?force=true` unbinds them and they fall back to **direct** origins. |
| **Settings** | Deployment-wide limits: master switch, pairing-token lifetime, streams per session, targets per tunnel, session liveness, heartbeat, session-token lifetime. |

### Troubleshooting

- **`Connector offline` on the tunnel, `—` on last seen** — the connector cannot
  reach the control plane. Check `journalctl -u pingwaf-tunnel`, the
  `--server-url`, and `--server-ca-cert` (the control plane's certificate is
  self-signed on first boot).
- **Connector online but `Edges 0`** — no edge is offering itself. Enable
  `tunnel_addr` on an edge and set its ingress address.
- **`Targets None`** — no origin pool is bound to the tunnel yet.
- **Requests answer 503/502 while the tunnel looks online** — the edge has no
  session (its tunnel listener is off, or the pinned certificate changed). The
  agent's detail panel shows the listener state and fingerprint; a changed
  fingerprint is picked up by the connector on its next config refresh.
- **A target refuses to connect** — the address is resolved by the connector, so
  it must be reachable *from the connector host*, and it must be in the
  control plane's target list (and in the connector's `allowed_targets`, when
  that is set).

### Security model

- **The edge never sees the connector's credential.** The connector presents a
  short-lived **session token** that the control plane signs with an Ed25519
  key (derived from the deployment's JWT secret); the edge verifies the
  signature, the tunnel id and the expiry against a public key that arrives in
  the rule bundle. The credential itself only ever authenticates the connector's
  control stream to the control plane.
- A captured token is therefore useless after its lifetime (one hour by
  default, `session_token_ttl_seconds`) and cannot be replayed against another
  tunnel. The token lifetime is also what bounds how long a **partitioned**
  edge keeps accepting a revoked tunnel, so shorten it if revocation must
  converge faster than the rule-bundle push.
- The edge's tunnel certificate is self-signed and generated locally; the
  connector **pins its SHA-256** and fails closed on a mismatch. It is
  persisted, so a restart keeps the same fingerprint. To rotate it, stop the
  edge, delete the certificate files under its data directory
  (`cert.pem` / `key.pem`), and start it again: the new
  fingerprint reaches the connectors on the next heartbeat.
- The edge only opens streams for targets its own sites reference, and the
  connector only dials targets the control plane lists (optionally narrowed by
  its local `allowed_targets`).
- Bridge ports are loopback-only. A process already running on an edge host can
  reach them, exactly as it could reach any local proxy; the edge host is
  trusted.
- **One tunnel is one trust domain.** Every edge that serves a site bound to a
  tunnel can ask the connector for any target in that tunnel's list, not only
  the ones its own sites reference. Split network segments across separate
  tunnels.
- **Revocation converges while the edge can reach the control plane.** The
  policy (including the token's public key) is cached on disk, so an edge that
  is partitioned from the control plane keeps accepting tokens it can verify
  until they expire — the same fail-open behaviour the deployment applies to
  its rules. A revoked tunnel stops working as soon as the edge receives the
  next bundle (≤5s on a connected edge).

### Limits in this version

- HTTP, HTTPS and WebSocket through a site; raw TCP is not exposed.
- One tunnel is one trust domain: every edge serving a site bound to it can ask
  the connector for any of that tunnel's targets. Split network segments across
  separate tunnels.
- Per-tunnel targets and per-session streams are capped by the settings row.
- The edge listener binds at startup; the ingress address and the policy are
  hot (≤5s on a connected edge).

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

IP rules are evaluated before the WAF. Bulk import accepts one IP or CIDR per line. Entries are validated at save time: each must be a valid IP address or CIDR, and an invalid value (e.g. `10.0.0.0/33`) is rejected with a `400` naming it — it is never stored as a rule that silently matches nothing.

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

Range entries are validated when a group is saved: each must be a valid IP address or CIDR, and an invalid value is rejected with `400` naming it. Subscription syncs skip fetched lines that are not valid IP/CIDR entries.

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

Validation matches path exceptions: a `regex` scope is compile-checked when saved (invalid patterns are rejected with `400`), and saving any of the four settings with a **non-empty `path`** — regex or not — requires every agent serving the site to be **≥ 0.32.20**; otherwise the save is rejected with a `400` naming the agent and version (see [Path Exceptions](#path-exceptions) for the message format and the reasoning). An empty `path` (whole site) is never gated. Re-enabling a disabled path exception runs the same gate.

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

**Data boundary:** the assistant calls the same MCP tools as external clients. Every question and every tool result it receives — site metadata, access logs, WAF events and, for an administrator, control-plane audit entries — is sent to the provider configured above. Enable it only where that data may leave your deployment, and review the provider's retention and data-processing terms. **Allow write tools** is off by default; leave it off unless you accept that a prompt-injected model could change live configuration.

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

**Scope:** a `read` key is accepted but narrowed to the sites its owner owns; only a `write` key whose owner is an administrator is fleet-wide and can call the admin-only tools (the control-plane audit log, defense settings, site pause). Read-only credentials see only the read tools; administrators with a `write` key may also call `set_observation_mode` and `set_site_status`. Treat a `write` key like a dashboard administrator credential — do not hand it to an AI client you do not control. The endpoint sits behind the same API protection as the console (access log, IP allowlist, WAF), so it obeys whatever **Settings → API protection** allows.

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
  version and can be rolled back in turn. A rollback whose snapshot contains a
  path scope the current agents cannot execute is refused with a `400` before
  anything changes — upgrade the agents (or clear the offending scope's
  `path`) and retry (see [Path Exceptions](#path-exceptions)).

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
