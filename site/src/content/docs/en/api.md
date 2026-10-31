---
title: API
description: REST groups, authentication, the gRPC control-plane contract, API keys and automation entry points.
minutes: 7
---

## REST

Every endpoint lives under `/api/v1` on the control-plane address, `https://<host>:9080/api/v1`. Authentication is a bearer access token:

```bash
TOKEN=$(curl -s -X POST https://localhost:9080/api/v1/auth/login \
  -H 'content-type: application/json' \
  -d '{"email":"admin@pingwaf.local","password":"..."}' | jq -r .access_token)

curl -s https://localhost:9080/api/v1/sites -H "Authorization: Bearer $TOKEN"
```

A login returns `access_token`, `refresh_token`, `token_type: Bearer`, `expires_in` (43200 seconds) and `role`. Only `/auth/login`, `/auth/register`, `/auth/status`, the two passkey steps, `/health` and `/version` are unauthenticated. `/system/*` additionally requires an administrator.

## Endpoint groups

| Group | Representative endpoints |
| --- | --- |
| Auth | `POST /auth/login`, `POST /auth/refresh`, `GET|PUT /auth/me`, `PUT /auth/password` |
| Sites | `GET|POST /sites`, `GET|PUT|DELETE /sites/{id}`, `POST /sites/{id}/clone` |
| Rules | `GET|POST /sites/{id}/rules`, `GET|PUT|DELETE /sites/{id}/rules/{rule_id}`, `POST /sites/{id}/waf/defaults` |
| WAF | `GET|PUT /sites/{id}/waf/settings`, `GET /sites/{id}/waf/posture`, `GET /managed-rules` |
| Logs | `GET /logs/security`, `GET /logs/access`, `GET /logs/access/{id}`, `DELETE /logs/purge` |
| Agents | `GET /agents`, `POST /agents/enroll`, `POST /agents/{id}/commands` |
| Config versions | `GET /config-versions`, `GET /config-versions/{id}`, `POST /config-versions/{id}/rollback`, `POST /config-versions/prune` |
| System | `GET /system/metrics`, `/system/profile`, `/system/nodes`, `/system/status` |

Site-scoped calls read and write through the tenant boundary (`load_site_read` / `load_site_write`), so a token only sees the sites it is allowed to manage. The complete reference, including every query parameter and response shape, is `docs/api.md` in the repository.

## Certificate sharing

One certificate (e.g. a wildcard) can serve several sites. A site sees its owned certificates plus the ones shared with it — each with a `relation` field (`owner`, `linked`, or `available`). The certificate's owning site keeps driving renewals and ACME bookkeeping.

| Endpoint | Purpose |
| --- | --- |
| `GET /sites/{id}/certificate-links` | Certificates shared with this site (same shape as the certificate list) |
| `POST /sites/{id}/certificate-links` | Link a certificate: `{ "certificate_id": "uuid" }` → `201`; `409` if the site already owns or uses it; `404` if the owning site is not readable |
| `DELETE /sites/{id}/certificate-links/{cert_id}` | Unlink → `204`; `409` while the certificate is the site's current TLS selection |
| `GET /sites/{id}/certificates/available` | Certificates this site may still link |

`PUT /sites/{id}/ssl-settings` accepts a `certificate_id` the site owns or has linked (otherwise `400 does not belong to this site and is not linked to it`) and `clear_certificate: true` to detach the current one. Selecting a certificate clears the legacy inline PEM copy on `site_ssl` — the selected certificate is always what the edge serves.

## Path exceptions

A site in observation mode (`waf_settings.mode = monitor`) can whitelist request paths that skip selected protection layers (`waf`, `bot`, `access`, `basic_auth`, `mtls`, `ai_bot`, `rate_limit`, `challenge`, or `*`). Saving in any other mode fails with `409`; the data plane also ignores exceptions on non-observing sites. Paused sites, fail-closed edges and dynamic IP bans are never bypassed. With `log_hits: true` each hit is recorded as a `path_bypass` security event.

| Endpoint | Purpose |
| --- | --- |
| `GET /sites/{id}/path-exceptions` | List exceptions (≤64 per site, unique `path`) |
| `POST /sites/{id}/path-exceptions` | Create: `path` (starts with `/`, ≤512), `match_type` (`prefix`/`exact`/`wildcard`), `skip_layers`, `note`, `log_hits`, `enabled` |
| `GET|PUT|DELETE /sites/{id}/path-exceptions/{id}` | Read / partial update / delete (`204`) |

## Flush logs

`POST /agents/flush-logs` (administrators) asks every connected agent to ship its in-memory log buffer immediately instead of waiting out the batch interval. Returns `{ "delivered": N, "offline": M }`; offline agents are counted but not queued, and `delivered` means the command was handed over — not that the entries are stored yet.

## gRPC

Agents use the `ControlPlane` service on port `9090`, defined in `pingwaf-proto/proto/control_plane.proto`. It carries `RegisterAgent`, `Heartbeat`, `SyncRules`, `ShipLogs`, `ShipCertEvents`, `ShipMetrics` and `GetSiteConfig`, plus the visitor session RPCs used by the challenge flow. TLS is the default; `grpc_tls_mode = "off"` is a deliberate downgrade for trusted networks only.

## API keys

Service accounts authenticate with keys prefixed `pwk_`, generated with 128 bits of entropy and stored as bcrypt hashes — the plaintext is shown once, at creation. Each key carries `read` or `write` scope and an expiry.

```bash
curl -s -X POST https://localhost:9080/api/v1/keys \
  -H "Authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -d '{"name":"ci","scopes":["read"],"expires_in_days":90}'
```

## MCP

The control plane exposes a Streamable HTTP MCP endpoint at `POST /mcp` (root path, deliberately outside `/api/v1`). It accepts either a `pwk_…` API key or a JWT; write tools additionally require the key owner to be an administrator. `GET` and `DELETE` return `405`, and request bodies are capped at 256 KiB. This is the intended way to let an assistant inspect sites, rules and logs without handing it dashboard credentials.

## Notifications

Webhook notifications are configured with `PINGWAF_WEBHOOK_URL`, and the notification routes expose the current configuration from the console. Use them to fan out security events to your existing alerting pipeline.

## Automation recipes

- **Keep configuration in review** — snapshot with `GET /config-versions` after each change, and roll back with `POST /config-versions/{id}/rollback` in a break-glass job.
- **Enroll edges from CI** — `POST /agents/enroll` returns the install command, so a provisioning pipeline never stores an API key itself.
- **Ship logs** — poll `/logs/security` for blocked requests, or enable Elasticsearch shipping and query it directly.
