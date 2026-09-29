# PingWAF REST API Reference

Base URL: `https://your-server:9080/api/v1`

The control plane serves HTTPS by default (`tls_enabled = true`) with a
self-signed certificate generated on first boot, so `curl` needs `-k` until the
certificate is trusted or replaced (see
[Control-plane certificate](#control-plane-certificate)). When TLS is disabled
(`tls_enabled = false`, e.g. TLS terminated by a reverse proxy) the same port
serves plain HTTP.

Cleartext requests to a TLS-enabled listener are answered with `308 Permanent
Redirect` to the `https://` URL on the same host, so scripts should either use
`https://` directly or follow redirects. The two health endpoints (`/healthz`
and `/api/v1/health`) are exempt and answer on both schemes, which keeps
container health checks working without a certificate.

## Authentication

All endpoints (except `/auth/login`, `/auth/register`, `/auth/status`, the
passkey sign-in pair `/auth/passkey/login/begin`, `/auth/passkey/login/finish`,
`/health` and `/version`) require a Bearer token:

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

Check if setup is needed, and which credential types this deployment offers.

**Response (200):**
```json
{
  "needs_setup": false,
  "registration_open": true,
  "passkey_enabled": true
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

Change password. The new password must be at least 8 characters.

**Request:**
```json
{
  "current_password": "old-password",
  "new_password": "new-password"
}
```

**Response (204):** empty.

A wrong `current_password` is a `400` (not a `401`), so clients do not treat a
typo as a dead session. Answers `204` on success.

---

## Passkeys

Passkeys (WebAuthn) are an alternative sign-in for a browser that owns a
platform authenticator or a security key. A passkey is always bound to an
account that is already signed in — the console mints them from Settings →
Passkeys, and the operator then signs in with the fingerprint/face/PIN prompt
instead of a password.

Two conditions must hold for the browser prompt to even appear:

* `GET /auth/status` reports `passkey_enabled: true` (the `passkey_enabled`
  server setting, on by default). When it is `false` every endpoint below
  answers `501`.
* The page is a secure context: HTTPS, or `localhost`. A passkey cannot be
  created for a bare IP address or over plain HTTP on a hostname. The control
  plane therefore serves HTTPS by default with a self-signed certificate that
  can be replaced under [Control-plane certificate](#control-plane-certificate);
  a bare-IP deployment needs that IP in the certificate's SANs.

### GET /auth/passkeys

List the passkeys bound to the signed-in account. Neither the credential id nor
the public key is exposed.

```json
[
  {
    "id": "uuid",
    "name": "Work laptop",
    "created_at": "2024-01-01T00:00:00Z",
    "last_used_at": "2024-02-01T09:12:00Z"
  }
]
```

### PATCH /auth/passkeys/{passkey_id}

Rename a passkey.

**Request:**
```json
{ "name": "Work laptop" }
```

**Response (200):** the updated summary. Names are 1-100 characters.

### DELETE /auth/passkeys/{passkey_id}

Remove a passkey. Answers `204`. The credential can no longer sign in; the
account's password still can.

### POST /auth/passkey/register/begin

Start binding a new passkey to the signed-in account.

**Response (200):** `PublicKeyCredentialCreationOptions` in the WebAuthn JSON
encoding (base64url strings, camelCase fields) to be handed to
`navigator.credentials.create()`.

```json
{
  "rp": { "id": "waf.example.com", "name": "PingWAF" },
  "user": { "id": "…", "name": "ops@example.com", "displayName": "Ops" },
  "challenge": "…",
  "pubKeyCredParams": [{ "type": "public-key", "alg": -7 }],
  "timeout": 300000,
  "excludeCredentials": [],
  "authenticatorSelection": { "residentKey": "preferred" },
  "attestation": "none"
}
```

### POST /auth/passkey/register/finish

Complete the ceremony with the credential returned by the browser.

**Request:** the standard registration response — `id`, `rawId`, `type`,
`response.{clientDataJSON, attestationObject, transports}` (base64url) plus an
optional `name` label and `clientExtensionResults`.

**Response (201):** the stored passkey summary, shaped like `GET /auth/passkeys`.

### POST /auth/passkey/login/begin

Start a password-less sign-in. Public — no bearer token.

**Response (200):** `PublicKeyCredentialRequestOptions` (base64url `challenge`,
`rpId`, `allowCredentials`) to be handed to `navigator.credentials.get()`.

Because the account is not known yet, the authenticator must be able to
discover the credential (a "discoverable"/resident key). Passkeys minted through
`register/finish` are created as `residentKey: "preferred"`: platform
authenticators make them discoverable by default, but a legacy security key that
refuses to store the credential cannot be used for password-less sign-in.

### POST /auth/passkey/login/finish

Verify the assertion and mint a session. Public — no bearer token.

**Request:** the standard assertion — `id`, `rawId`, `type`,
`response.{clientDataJSON, authenticatorData, signature, userHandle}` (base64url)
and `clientExtensionResults`.

**Response (200):** identical to `POST /auth/login`, so the client stores the
tokens the same way.

**Errors:** every verification failure is a `400` with an `error` message — a
`401` would make the console log the operator out mid-ceremony.

Ceremony state lives in the database with a 5 minute lifetime, so several
control plane replicas can serve one ceremony without sticky sessions.
Unauthenticated challenges share a process-wide budget (120/minute); exceeding
it answers `429`.

**Configuration** (`ServerConfig`, environment overrides):

| Env var | Default | Meaning |
|---------|---------|---------|
| `PINGWAF_PASSKEY_ENABLED` | `true` | Offer passkeys at all |
| `PINGWAF_PASSKEY_RP_NAME` | `PingWAF` | Relying party name shown by the browser |
| `PINGWAF_PASSKEY_RP_ID` | derived from `Host` | Relying party id (registrable domain) |
| `PINGWAF_PASSKEY_ORIGIN` | derived from `Host` | Exact origin the browser must report |
| `PINGWAF_PASSKEY_TRUST_FORWARDED_PROTO` | `false` | Honour `X-Forwarded-Proto` when deriving the origin |

Deriving from `Host` assumes the console is reached directly. Behind a reverse
proxy that terminates TLS, either set `PINGWAF_PASSKEY_ORIGIN` explicitly or turn
on `PINGWAF_PASSKEY_TRUST_FORWARDED_PROTO` — a wrong origin is an opaque
`SecurityError` in the browser, not a helpful message.

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
  "version": "0.17.0",
  "api": "/api/v1",
  "registration_open": true
}
```

---

## Sites

A site is one protected domain plus its origin pools, rules and TLS posture.

`status` is the site lifecycle:

| Value | Edge behaviour |
|-------|----------------|
| `active` | Traffic is proxied normally |
| `paused` | Every request is answered `503` with `Retry-After: 3600` before any rule runs — the certificate keeps serving and the request is still logged, so pausing is visible in the console rather than a silent outage. The site's custom `503` page is used when one is configured. |
| `pending` | Serves normally; a bookkeeping state for sites that are not finished being set up |

Only `paused` changes how traffic is handled.

### GET /sites

List sites (paginated).

**Query Parameters:**
| Param | Type | Description |
|-------|------|-------------|
| `page` | int | Page number (default: 1) |
| `page_size` | int | Items per page (default: 50, max: 200) |
| `search` | string | Filter by name/domain |
| `status` | string | Filter by status: `active`, `paused`, `pending` |

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
  "page_size": 50
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

Update site fields, including the status that pauses and resumes a site:

```json
{ "status": "paused" }
```

Only the fields present in the body are touched.

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

`address` is `host[:port]`. A leading `http://` or `https://` is accepted and
stripped (copy-pasting a URL is normal), a trailing `/` is ignored, and anything
longer — a path, query or fragment — is rejected with `400` rather than silently
dropped, because a dropped path also corrupts the SNI derived from the host. To
speak TLS to the origin set `tls: true` or give the pool an `sni`.

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

`lb_algorithm` accepts `round_robin`, `random`, `least_connections` or `hash:<type>[:<key>]` with type in `ip`, `url`, `path` (no key) or `header`, `cookie`, `query` (key required). A non-empty `sni` enables TLS to the origin.

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
  "ip_group_id": null,
  "ip_group_name": null,
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
  "pool_id": "uuid",
  "ip_group_id": "uuid"
}
```

`match_type` is `prefix`, `exact` or `regex`; `priority` (1-60000) is optional and defaults to the auto weight. `ip_group_id` is optional and must name an existing, enabled IP group visible to the caller whose membership is not empty; `ip_group_name` is filled in on responses.

A route with an IP group only matches clients whose address is inside one of the
group's CIDR ranges. Both conditions are ANDed, so the route above sends
`203.0.113.7` to the dedicated pool for `/api` but leaves every other client on
the default pool. This is how an internal-only or partner-only path is split off
without touching address-based access rules. The IP ranges are resolved when the
configuration is generated, so changing group membership takes effect after the
next configuration push — the agent does not need to be pointed at the group.
Routes without an IP group match any client.

### PUT /sites/{site_id}/routes/{route_id}

Update a route (`priority: 0` resets it to the auto weight; `ip_group_id: null`
clears the group).

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

### GET /sites/{site_id}/ssl-settings

The effective TLS posture of the site.

**Response (200):**
```json
{
  "https_enabled": true,
  "min_tls_version": "1.2",
  "max_tls_version": null,
  "self_signed": false,
  "certificate_id": "uuid",
  "mtls_enabled": false,
  "has_mtls_client_ca": false,
  "mtls_organization": null,
  "mtls_require_client_cert": false,
  "hsts_enabled": true,
  "hsts_max_age": 15552000,
  "always_use_https": true
}
```

### PUT /sites/{site_id}/ssl-settings

Update one or more fields; omitted fields keep their value. Requires the `write`
role.

**Request:**
```json
{
  "hsts_enabled": true,
  "hsts_max_age": 15552000,
  "always_use_https": true,
  "mtls_enabled": true,
  "mtls_require_client_cert": true,
  "mtls_organization": "Acme Devices"
}
```

| Field | Behaviour at the edge |
|-------|-----------------------|
| `hsts_enabled` / `hsts_max_age` | Adds `Strict-Transport-Security: max-age=<n>` to HTTPS responses. `max-age` is 1-63072000 seconds (two years); `hsts_max_age: 0` is what browsers read as "stop pinning", so the console defaults to `15552000` (180 days) when HSTS is switched on. |
| `always_use_https` | Plain HTTP requests are answered `301` with the `https://` equivalent before any rule runs. The two listeners share one location, so this is safe to enable with traffic on both ports. **Browsers cache HSTS**: when testing, keep `hsts_max_age` low or use a throwaway domain, or a redirect will haunt the browser for months. |
| `mtls_enabled` / `mtls_require_client_cert` / `mtls_organization` | Requires a client certificate; see [mTLS](#mtls). |
| `min_tls_version` / `max_tls_version` | Accepted as `1.0`, `1.1`, `1.2` (default) or `1.3`. |

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
| `page_size` | int | Items per page |

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
  "page_size": 50
}
```

### GET /ssl-events

Cross-certificate event log for every site the caller may see (paginated). Supports the same `certificate_id` and `event_type` filters; each event additionally carries the resolved `domain` and `site_domain`.

---

## mTLS

Mutual TLS for one site: the edge asks the client for a certificate, and a
request only reaches the origin when that certificate was issued by a CA the
site trusts, has not been revoked, and was issued for this site.

Two halves work together:

* **Trust and revocation** live here — a CA (generated by the control plane or
  imported) plus the client certificates issued from it.
* **Enforcement** is the site's TLS posture: set `mtls_enabled: true` via
  `PUT /sites/{site_id}/ssl-settings`, and `mtls_require_client_cert: true` to
  refuse requests that present no certificate at all. Setting `mtls_organization`
  pins the organization a client certificate must carry, which is what keeps a
  certificate issued for one site from being replayed against another.

Adding or removing a CA rewrites the site's trust anchor and pushes the
configuration; a site that loses its last CA is left with `mtls_enabled: false`
rather than enabled-but-unverifiable.

Only certificates generated here come with a private key, and the key is
returned **exactly once** — when the CA or the certificate is created. Imported
CAs act as trust anchors only.

> Certificates generated by the control plane are stored unencrypted in the
> database. Treat a database dump as key material.

### GET /sites/{site_id}/mtls/cas

List the site's CAs. Private keys are never included; `has_private_key` and the
SHA-256 fingerprint are.

```json
[
  {
    "id": "uuid",
    "site_id": "uuid",
    "name": "Acme device CA",
    "source": "generated",
    "cert_pem": "-----BEGIN CERTIFICATE-----\n…",
    "has_private_key": true,
    "subject_dn": "CN=Acme device CA",
    "serial": "0a1b…",
    "fingerprint_sha256": "9f2c…",
    "expected_organization": "Acme Devices",
    "not_before": "2024-01-01T00:00:00Z",
    "not_after": "2025-01-01T00:00:00Z",
    "is_active": true,
    "created_at": "2024-01-01T00:00:00Z",
    "certificate_count": 3
  }
]
```

### POST /sites/{site_id}/mtls/cas

Create a CA. Requires the `write` role.

**Request (generate):**
```json
{
  "name": "Acme device CA",
  "organization": "Acme Devices",
  "validity_days": 3650
}
```

**Request (import):**
```json
{
  "name": "Corporate PKI",
  "cert_pem": "-----BEGIN CERTIFICATE-----\n…",
  "organization": "Acme Devices"
}
```

`cert_pem` switches to import mode: the certificate becomes a trust anchor and
no private key is involved. `validity_days` is capped at 3650 for a CA.

**Response (201):** the CA plus `key_pem`, present only for a generated CA and
only on this response.

### DELETE /sites/{site_id}/mtls/cas/{ca_id}

Delete a CA. Refused with `400` while any client certificate issued from it still
exists — revoked ones included, since they are what keeps the revocation list
intact.

### GET /sites/{site_id}/mtls/certificates

List client certificates, revoked ones included (`status` is `active` or
`revoked`). `ca_name` is filled in for convenience.

```json
[
  {
    "id": "uuid",
    "site_id": "uuid",
    "ca_id": "uuid",
    "ca_name": "Acme device CA",
    "name": "laptop-1",
    "common_name": "laptop-1",
    "organization": "Acme Devices",
    "serial": "1f0c…",
    "fingerprint_sha256": "c4d1…",
    "cert_pem": "-----BEGIN CERTIFICATE-----\n…",
    "has_private_key": true,
    "not_before": "2024-01-01T00:00:00Z",
    "not_after": "2025-01-01T00:00:00Z",
    "status": "active",
    "revoked_at": null,
    "revocation_reason": null,
    "created_at": "2024-01-01T00:00:00Z"
  }
]
```

### POST /sites/{site_id}/mtls/certificates

Issue a client certificate from one of the site's CAs.

**Request:**
```json
{
  "ca_id": "uuid",
  "name": "laptop-1",
  "common_name": "laptop-1",
  "organization": "Acme Devices",
  "validity_days": 365
}
```

`name`, `common_name` and `organization` default to the CA's values; the
validity is capped at the CA's own `not_after`, because a certificate outliving
its CA verifies against nothing.

**Response (201):** the certificate plus `key_pem`, returned exactly once.

### POST /sites/{site_id}/mtls/certificates/{cert_id}/revoke

Revoke a certificate. The fingerprint joins the site's revocation list and the
configuration is pushed, so the very next request with that certificate is
refused with `403` — no waiting for a CRL or an OCSP check.

**Request:**
```json
{ "reason": "laptop lost" }
```

**Response (200):** the updated certificate.

### DELETE /sites/{site_id}/mtls/certificates/{cert_id}

Delete a certificate row. An `active` certificate must be revoked first (`400`
otherwise) so that removing a record cannot quietly re-admit a client that is
still holding the key. Revoking also removes its fingerprint from the deny list;
the request-detail history keeps naming it.

### GET /sites/{site_id}/mtls/certificates/{cert_id}/download

Download the certificate, and its private key when it has one, as a single PEM
bundle (`application/x-pem-file`) suitable for `curl --cert`.

### How a client presents it

```bash
curl --cert laptop-1.pem --key laptop-1.pem https://example.com/
```

Requests that reach the site from a client without a usable certificate are
answered `403` and recorded as access-log rows only — they are not WAF verdicts
and do not show up as security events.

---

## API Keys

Keys authenticate an agent (or any script) without a user password. The
plaintext is shown once, at creation; the row keeps a bcrypt hash and the first
8 characters for display. Agents send the key as `Authorization: Bearer pwk_…`.

### GET /keys

List API keys (paginated). Administrators see every key, other callers see their
own. The plaintext is never returned here.

### POST /keys

Create a new API key. Requires the `write` role.

**Request:**
```json
{
  "name": "Agent Key",
  "permissions": ["agent", "read"],
  "expires_at": "2025-01-01T00:00:00Z"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Label, 1-100 characters (required) |
| `permissions` | string[] | Any of `agent`, `read`, `write`; defaults to `["agent", "read"]` |
| `expires_at` | string | RFC 3339 instant in the future; omitted = never expires |

**Response (201):**
```json
{
  "id": "uuid",
  "user_id": "uuid",
  "name": "Agent Key",
  "key_prefix": "pwk_9f2c",
  "key": "pwk_9f2c8d1e…",
  "permissions": ["agent", "read"],
  "expires_at": "2025-01-01T00:00:00Z",
  "last_used_at": null,
  "created_at": "2024-01-01T00:00:00Z"
}
```

> The full key is returned **only** by this call. `last_used_at` is filled in as
> agents authenticate.

### DELETE /keys/{key_id}

Revoke an API key. Answers `204`; the key stops authenticating immediately.
Administrators may revoke any key, other callers only their own.

---

## Agents

### GET /agents

List registered agents.

**Query Parameters:**
| Param | Type | Description |
|-------|------|-------------|
| `page` | int | Page number |
| `page_size` | int | Items per page |
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
      "version": "0.17.0",
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
| `page_size` | int | Items per page |
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

`characteristics` accepts `ip`, `ip_nat`, `host`, `path`, `asn`, `country`, and the parameterized forms `header:<name>`, `cookie:<name>`, `query:<name>` (requests missing the value bucket together). Header names match case-insensitively; cookie and query names are case-sensitive. `ja3` is reserved for future use. `expression` is an optional filter — empty means all requests.

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

Every proxied request writes one `access_logs` row; a request the WAF acted on
also writes a `security_events` row that shares its `request_id` with the access
row, so a security event can be joined back to the full request and response.

> **Headers and bodies are stored verbatim.** `Cookie`, `Authorization` and
> `Set-Cookie` values are deliberately *not* redacted — the console is meant to
> reproduce what a client actually sent — so `access_logs` is sensitive data.
> Keep the retention window short (`DELETE /logs/purge`) and restrict access.

Request and response bodies are prefixes: the agent stores at most
`max_body_log_size` bytes per direction (8 KiB by default, 64 KiB ceiling,
`capture_response_body` can switch response capture off). Both directions report
the real size and a `*_truncated` flag, so a short body is never mistaken for a
complete one.

**Console search syntax.** The log pages accept one Kibana-style query string and
translate it into the parameters below, so the same filters are reachable from
the API:

```
client_ip:203.0.113.44      exact;  client_ip:203.0.113.*   prefix
client_ip:10.0.0.1,10.0.0.8 any of several values
path:/api/v1/users          substring;  status:404  status:4xx
method:POST,PUT   action:block   rule:xss-001
country:CN   cache_status:hit   latency:>500   site:<uuid>
from:2026-09-01  to:2026-09-02
api/v1/users                free text: matches path or host
```

Values containing spaces go in double quotes (`path:"/my files"`), `ip:` and
`rule:` alias `client_ip:` and `rule_id:`, and `method`, `action`, `rule` and
`cache_status` are only understood on the tab they belong to. A term the grammar
cannot place is reported to the operator rather than ignored.

### GET /logs/security

List security events, newest first.

**Query Parameters:**
| Param | Type | Description |
|-------|------|-------------|
| `page` | int | Page number (default: 1) |
| `page_size` | int | Items per page (default: 50, max: 200) |
| `site_id` | string | Filter by site; omitted = every site the caller may see |
| `from` | string | RFC 3339 start (default: 24 hours ago) |
| `to` | string | RFC 3339 end (default: now) |
| `client_ip` | string | Exact IP; `a,b` for a list; `10.0.0.*` for a prefix |
| `action` | string | `block`, `challenge`, `log`, `allow` (`a,b` for a list) |
| `rule_id` | string | Rule id (`a,b` for a list) |
| `host` | string | Request host (`*` wildcards allowed) |
| `path` | string | Substring match on the request path |
| `country_code` | string | ISO country code, upper-cased server side |
| `request_id` | string | Exact request id, e.g. from an `X-Request-ID` header |
| `q` | string | Free text; matches the path or the host |

`from` must be earlier than `to` and the window may not exceed 90 days.

**Response (200):** paginated `security_events` rows.

```json
{
  "items": [
    {
      "id": 1,
      "site_id": "uuid",
      "agent_id": "uuid",
      "request_id": "6f1c…",
      "timestamp": "2024-01-01T00:00:00Z",
      "client_ip": "203.0.113.7",
      "method": "GET",
      "host": "example.com",
      "path": "/search",
      "rule_id": "942100",
      "rule_name": "SQL Injection Attack Detected",
      "action": "block",
      "score": 5,
      "waf_details": "{\"matched\":\"1' or '1'='1\"}",
      "country_code": "US",
      "user_agent": "curl/8.4.0",
      "created_at": "2024-01-01T00:00:01Z"
    }
  ],
  "total": 1,
  "page": 1,
  "page_size": 50
}
```

### GET /logs/access

List access logs, newest first.

**Query Parameters:**
| Param | Type | Description |
|-------|------|-------------|
| `page` | int | Page number (default: 1) |
| `page_size` | int | Items per page (default: 50, max: 200) |
| `site_id` | string | Filter by site; omitted = every site the caller may see |
| `from` | string | RFC 3339 start (default: 24 hours ago) |
| `to` | string | RFC 3339 end (default: now) |
| `client_ip` | string | Exact IP; `a,b` for a list; `10.0.0.*` for a prefix |
| `method` | string | HTTP method, upper-cased server side (`a,b` for a list) |
| `status_code` | int | Exact status, 100-599 |
| `status_class` | int | Status class: `1`, `2`, `3`, `4` or `5` |
| `host` | string | Request host (`*` wildcards allowed) |
| `path` | string | Substring match on the request path |
| `cache_status` | string | `hit`, `miss`, … (lower-cased, `a,b` for a list) |
| `country_code` | string | ISO country code, upper-cased server side |
| `min_latency_ms` | int | Only rows at or above this total latency |
| `request_id` | string | Exact request id |
| `q` | string | Free text; matches the path or the host |

**Response (200):** paginated `access_logs` rows.

```json
{
  "items": [
    {
      "id": 1,
      "site_id": "uuid",
      "agent_id": "uuid",
      "request_id": "6f1c…",
      "timestamp": "2024-01-01T00:00:00Z",
      "client_ip": "203.0.113.7",
      "method": "GET",
      "host": "example.com",
      "path": "/search",
      "query_string": "q=hello",
      "status_code": 200,
      "response_size": 5120,
      "upstream_addr": "10.0.1.50:3000",
      "upstream_latency_ms": 12,
      "total_latency_ms": 15,
      "cache_status": null,
      "user_agent": "curl/8.4.0",
      "referer": null,
      "country_code": "US",
      "tls_version": "TLSv1.3",
      "scheme": "https",
      "protocol": "HTTP/1.1",
      "request_headers": {"accept": "*/*", "cookie": "session=…"},
      "request_body": "q=hello",
      "request_body_size": 7,
      "request_body_truncated": false,
      "response_headers": {"content-type": "text/html", "set-cookie": "…"},
      "response_body": "<!doctype html>…",
      "response_body_size": 5120,
      "response_body_truncated": true
    }
  ],
  "total": 1,
  "page": 1,
  "page_size": 50
}
```

Rows are only as complete as the agent that produced them: `scheme`, `protocol`
and the response fields come from agents on 0.15.0 or newer. Against an older
agent those columns stay `null` and the console shows the request side only.

### DELETE /logs/purge

Delete log rows older than a cutoff. Administrators only.

**Query Parameters:**
| Param | Type | Description |
|-------|------|-------------|
| `older_than_days` | int | Retention window in days, 1-3650 (default: 30) |
| `site_id` | string | Limit the purge to one site; omitted = every site |

```
DELETE /api/v1/logs/purge?older_than_days=30
```

**Response (200):**
```json
{
  "deleted_security_events": 120,
  "deleted_access_logs": 8421,
  "cutoff": "2024-01-01T00:00:00Z"
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
| `page_size` | int | Items per page |
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
  "page_size": 50
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

A custom error page replaces the body of any response with a matching status
code — including responses the edge itself produces (a WAF block, a rate limit,
a paused site), not only ones that came from the origin. At most one page per
status code per site.

### GET /sites/{site_id}/error-pages

List custom error pages (paginated), ordered by status code.

### POST /sites/{site_id}/error-pages

Create a custom error page.

**Request:**
```json
{
  "status_code": 403,
  "name": "Access denied",
  "content_type": "text/html",
  "body_template": "<html><body><h1>Access Denied</h1><p>{{ request_id }}</p></body></html>",
  "enabled": true
}
```

| Field | Description |
|-------|-------------|
| `status_code` | 400-599 |
| `name` | 1-200 characters |
| `content_type` | Defaults to `text/html` |
| `body_template` | Tera template, must not be empty |
| `enabled` | Defaults to `true`; a disabled page is skipped |

`body_template` renders with `request_id`, `error_code`, `error_message`,
`site_name`, `client_ip`, `timestamp`, `method`, `path` (aliased as
`request_path`), `host` (aliased as `request_host`) and `user_agent`. A template
that fails to render falls back to a plain-text body, so a typo cannot turn a
`403` into a `500`.

### PUT /sites/{site_id}/error-pages/{page_id}

Update an error page; omitted fields keep their value.

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

## Control-plane certificate

The certificate the dashboard and REST API are served with. Administrators
only: the private key never leaves the server and no endpoint returns it.

A self-signed pair (EC P-256, `CN=PingWAF Control Plane`) is generated on first
boot when TLS is enabled and nothing has been stored yet. Replacing it takes
effect immediately — the listener swaps the pair in place, so no restart is
needed. Passkeys require a secure origin, which is the reason this exists.

### GET /system/tls

Current status, including the active certificate's metadata.

**Response (200):**
```json
{
  "enabled": true,
  "has_certificate": true,
  "certificate": {
    "id": "uuid",
    "source": "self_signed",
    "subject_dn": "CN=PingWAF Control Plane",
    "common_name": "PingWAF Control Plane",
    "sans": ["localhost", "127.0.0.1", "waf.example.com"],
    "serial": "1f0c…",
    "fingerprint_sha256": "3b1a…",
    "not_before": "2026-09-30T00:00:00Z",
    "not_after": "2028-12-30T00:00:00Z",
    "created_at": "2026-09-30T00:00:00Z",
    "expires_in_days": 821
  },
  "default_sans": ["localhost", "127.0.0.1", "waf.example.com"],
  "max_validity_days": 3650
}
```

`enabled` mirrors `tls_enabled`; `has_certificate` reports whether the running
listener holds a usable pair. `certificate` is omitted when nothing is stored.

### GET /system/tls/certificate

Download the certificate in use (PEM, chain included) as
`application/x-pem-file`. Handy for pinning or for installing the self-signed
pair into a trust store.

### PUT /system/tls/certificate

Upload a certificate and its private key. The pair is validated before anything
is written: a mismatched key or an unparsable chain returns `400` and the
running listener keeps serving the previous certificate.

**Request:**
```json
{
  "cert_pem": "-----BEGIN CERTIFICATE-----\n…\n-----END CERTIFICATE-----\n",
  "key_pem": "-----BEGIN PRIVATE KEY-----\n…\n-----END PRIVATE KEY-----\n"
}
```

`cert_pem` carries the leaf first, then intermediates when the issuer is not a
root. `key_pem` accepts PKCS#8, PKCS#1 or SEC1 (`RSA`, `EC` and `PKCS8`
begin-lines are all understood).

**Response (200):** the same shape as `GET /system/tls`.

### POST /system/tls/certificate/self-signed

Generate and install a fresh self-signed certificate.

**Request** (every field optional):
```json
{
  "common_name": "PingWAF Control Plane",
  "sans": ["waf.example.com", "10.0.0.5"],
  "validity_days": 825
}
```

Defaults: the configured console common name, the deployment's alternative
names (configured `tls_sans` plus the host name, the passkey relying party,
`localhost` and `127.0.0.1`, and the host the request arrived on), and 825 days
(capped at `max_validity_days`, 3650).

**Response (200):** the same shape as `GET /system/tls`.

---

## Debug & Profiling

Built-in pprof-style profiling of the control-plane process. All endpoints require an admin token. Sampling is process-global: while a capture is running, further requests return `409 Conflict`.

CPU sampling requires Linux (the only platform with a signal-safe unwinder for the sampling handler); on other platforms the capture endpoints return `501 Not Implemented`, while `/debug/pprof/memory` works everywhere.

Symbols are resolved from the binary's symbol table. The default `release` profile strips symbols, so most frames read `Unknown` — profile a `release-perf` build (`make release-perf`) to get readable flame graphs.

### GET /debug/pprof/profile

Capture CPU samples and return a gzip-compressed pprof protobuf consumable by `go tool pprof` and speedscope.

**Query:** `?seconds=30&frequency=99` — capture window (1-120 s, default 30) and sampling frequency in Hz (1-1000, default 99).

```bash
go tool pprof -http=: -insecure https://localhost:9080/api/v1/debug/pprof/profile?seconds=30
```

### GET /debug/pprof/flamegraph

Capture CPU samples and return an SVG flamegraph viewable directly in a browser. Same query parameters as above.

```bash
curl -k -H "Authorization: Bearer $TOKEN" \
  "https://localhost:9080/api/v1/debug/pprof/flamegraph?seconds=30" > flamegraph.svg
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
| 429 | `too_many_requests` | Rate limited (passkey challenges) |
| 500 | `internal_error` | Server error |
| 501 | `not_implemented` | Feature disabled on this deployment (passkeys) |

---

## Pagination

List endpoints support pagination:

**Query:** `?page=1&page_size=50`

| Param | Type | Description |
|-------|------|-------------|
| `page` | int | Page number, 1-based (default: 1) |
| `page_size` | int | Items per page (default: 50, maximum: 200) |

**Response:**
```json
{
  "items": [...],
  "total": 100,
  "page": 1,
  "page_size": 50
}
```

`total` is the number of rows matching the filters, not the size of `items`.

## Log retention

`DELETE /logs/purge` deletes rows older than `older_than_days` (default 30) for
one site or, for an administrator, for every site. Operators that keep the
default `30` day window should schedule the call; there is no background purger.
