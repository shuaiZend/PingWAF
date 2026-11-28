# PingWAF Deployment Guide

## Prerequisites

| Requirement | Minimum | Recommended |
|-------------|---------|-------------|
| OS | Linux (amd64/arm64), macOS | Ubuntu 22.04+, Debian 12+ |
| PostgreSQL | 14+ | 16+ — native packages follow the distribution (`postgresql-18` on Ubuntu 26.04, `postgresql-16` on 24.04); check with `apt-cache search --names-only '^postgresql-[0-9]+$'` |
| RAM | 512 MB | 2 GB+ |
| Disk | 1 GB | 10 GB+ (logs, cache) |
| Ports | 80, 443, 9080, 9090 | — |

## Quick Start (Docker Compose)

The fastest way to get PingWAF running — one command, no clone, no root:

```bash
curl -fsSL https://raw.githubusercontent.com/shuaiZend/PingWAF/main/deploy/compose/install.sh | bash
```

It creates `./pingwaf`, downloads [`docker-compose.yml`](../docker-compose.yml),
writes a private `.env` (mode `0600`) and starts the stack. Re-run it to upgrade
(an existing `.env` is never overwritten without `--force`, and the data volumes
are kept). The installer:

- **generates a random administrator password** when none is given and prints it
  **once**, at the end — store it before closing the terminal;
- **generates `PINGWAF_SECRET_KEY`** into `.env` and reuses it on `--force`;
  rotating it silently would make every sealed notification/AI/backup credential
  unreadable;
- **publishes the console (9080) on `0.0.0.0` and gRPC (9090) on
  `127.0.0.1`.** The console has to be reachable for a fresh install to be
  usable; restrict it with `PINGWAF_ADMIN_BIND` (an admin interface), a
  firewall, or a reverse proxy, and change the administrator password at the
  first sign-in. Set `PINGWAF_GRPC_BIND=0.0.0.0` only when remote agents need
  it;
- **never deletes data volumes.** `--force` rewrites `.env` but keeps
  `postgres-data` / `pingwaf-data` / `pingwaf-certs`; volume pruning is a
  separate, explicit action (see [Upgrade Procedure](#upgrade-procedure)).

Add `--tag`, `--admin-email`, `--tls-sans` or the port flags to customise, or use
the generator at <https://www.pingwaf.com/install/> (Chinese:
<https://www.pingwaf.com/zh/install/>).

> The compose file is downloaded from the release tag. A tag whose compose file
> predates the install hardening (no `PINGWAF_SECRET_KEY` pass-through, public
> 9080/9090 binds, default admin password) prints a warning — upgrade to a
> release whose compose file includes the hardening instead of assuming the
> installer's guarantees apply.

Prefer to inspect the files first? Clone the repository and run the same compose
file manually:

```bash
git clone https://github.com/shuaiZend/PingWAF.git
cd PingWAF

# Set secrets (required — there is no built-in default password any more)
export JWT_SECRET=$(openssl rand -hex 32)
export ADMIN_PASSWORD=$(openssl rand -hex 16)
export POSTGRES_PASSWORD=$(openssl rand -hex 16)
export PINGWAF_SECRET_KEY=$(openssl rand -hex 32)

docker compose up -d

# Verify (the health endpoint answers on HTTP and HTTPS alike)
curl http://localhost:9080/healthz
```

Dashboard: `https://localhost:9080` — served over TLS with a self-signed
certificate generated on first boot, so the browser warns until it is trusted
or replaced (**Settings → Control plane HTTPS**). Set `TLS_SANS` to the hostname
you use if it is not `localhost`.  
Account: `admin@pingwaf.local` / the `ADMIN_PASSWORD` you set. `ADMIN_PASSWORD`
has **no default**: an empty value makes the first boot fail instead of seeding
a known account, and the dashboard forces a password change at first login.

### Binding Ports 80/443 in Containers

The data plane listens on 80/443 and, inside the image, runs as the
unprivileged `pingwaf` user. Binding a port below 1024 therefore needs
`CAP_NET_BIND_SERVICE`; without it the proxy panics during startup:

```
thread 'Pingora HTTP Proxy Service' panicked at …/pingora-core-0.9.x/src/services/listening.rs:293:14:
Failed to build listeners: … PermissionDenied … bind() failed on [::]:443
```

Both listeners bind the IPv6 wildcard `[::]` by default with
`IPV6_V6ONLY=false`, so one socket serves IPv4 and IPv6: an IPv4 client simply
arrives as `::ffff:a.b.c.d` and the data plane canonicalises it back to
`a.b.c.d` before IP access rules, rate-limit keys, auto-ban and logs see it. Two
consequences worth knowing:

- A host with IPv6 disabled at the kernel level cannot bind `[::]`, so the
  default falls back to `0.0.0.0` automatically instead of failing to start.
- To pin one family, set `PINGWAF_LISTEN_HTTP_ADDR` / `PINGWAF_LISTEN_HTTPS_ADDR`
  (e.g. `0.0.0.0:80`), which is also what the `bind() failed on [::]:443`
  message above is telling you to look at.

Images built from this repository grant that capability to the binary at build
time (`setcap cap_net_bind_service=+ep /usr/local/bin/pingwaf`, asserted with
`getcap` in the build), so `docker compose up -d` and a plain `docker run` both
bind 80/443 without running as root.

File capabilities are ignored when the runtime policy forbids them. In that case
grant the capability at runtime instead:

| Symptom / environment | Fix |
|---|---|
| `Permission denied` on `0.0.0.0:443` with this compose file | The capability was stripped from the image, or a bind mount replaced the binary: check `getcap`, and keep the `cap_add: [NET_BIND_SERVICE]` entry the compose file already ships |
| `--security-opt no-new-privileges`, rootless Docker, hardened Kubernetes | File capabilities do not apply; add the runtime capability — `--cap-add=NET_BIND_SERVICE`, or `securityContext.capabilities.add: ["NET_BIND_SERVICE"]` |
| `address already in use` | Not a capability problem: the host port is taken. Change `PINGWAF_HTTP_PORT` / `PINGWAF_HTTPS_PORT`, or switch to host networking (below) |

Check a running container:

```bash
docker exec pingwaf getcap /usr/local/bin/pingwaf
# /usr/local/bin/pingwaf cap_net_bind_service=ep
docker exec pingwaf id -u
# non-zero (the system user allocated by useradd -r) — never root
```

#### Host networking

`docker-compose.yml` ships a commented-out `network_mode: host`. Host mode is
the right choice when the proxy must see the real client IP without PROXY
protocol, or when 80/443 cannot be published on the host at all. It changes the
port semantics:

- `ports:` must be removed: Compose refuses to publish ports together with
  host networking (`conflicting options: port publishing and the container type
  network mode`). The container binds the host's 80/443 directly.
- `networks:` must be commented out as well — Compose rejects
  `network_mode: host` together with a `networks` entry.
- 9080 (dashboard + REST API) and 9090 (gRPC) listen directly on the host, so
  restrict them with the host firewall.

```yaml
services:
  pingwaf:
    network_mode: host
    # ports: …    # must be removed: Compose rejects publishing ports in host mode
    # networks: … # must be removed: Compose rejects both together
```

Host mode does not bypass the capability check: the process still runs as
`pingwaf` and relies on the file capability (or `--cap-add`).

The systemd path solves the same problem a different way — the unit grants
`AmbientCapabilities=CAP_NET_BIND_SERVICE`; see
[Manual Installation](#manual-installation-binary--systemd).

### Environment Variables (docker-compose)

| Variable | Default | Description |
|----------|---------|-------------|
| `PINGWAF_TAG` | `latest` | PingWAF image tag — pin a release (or a digest) in production |
| `PINGWAF_HTTP_PORT` / `PINGWAF_HTTPS_PORT` | `80` / `443` | Host ports for proxied traffic |
| `PINGWAF_ADMIN_PORT` / `PINGWAF_GRPC_PORT` | `9080` / `9090` | Host ports for the console/API and the agent gRPC endpoint |
| `PINGWAF_ADMIN_BIND` / `PINGWAF_GRPC_BIND` | `0.0.0.0` / `127.0.0.1` | Host address the console/API and the agent gRPC endpoint publish on. The console is published on every interface so a fresh install is reachable — set it to a specific admin interface (or firewall the port / front it with a reverse proxy) and change the administrator password at the first sign-in. gRPC stays loopback unless remote agents need it |
| `PINGWAF_TLS_SANS` | empty | Comma-separated SANs for the self-signed console certificate |
| `TLS_ENABLED` | `true` | Serve the console over HTTPS |
| `POSTGRES_PASSWORD` | `pingwaf` (development fallback) | PostgreSQL password. **Set a random value in `.env`** (`openssl rand -hex 16`) for any real deployment: the compose file still falls back to `pingwaf` when unset, and the compose installer does not generate this one |
| `POSTGRES_BIND` | `127.0.0.1` | Host address the PostgreSQL port publishes on (`0.0.0.0` exposes it to the network) |
| `POSTGRES_PORT` | `5432` | Host port for PostgreSQL |
| `JWT_SECRET` | placeholder | JWT signing secret — leaving the placeholder is safe: a random secret is generated on first boot and persisted. When setting it, use 32 random bytes (`openssl rand -hex 32`); the server enforces a 16-character floor but a 32-byte random value is the recommended value |
| `PINGWAF_SECRET_KEY` | unset (required for sealing) | Instance key that seals notification-channel secrets, the AI provider key and backup-target credentials at rest. **Set it to 32 random bytes** (`openssl rand -hex 32`); without it those fields are stored in clear text (only a warning is logged). The official installers generate it (`deploy/compose/install.sh` into `.env`; the host installer into `/etc/pingwaf/pingwaf.env`, mode `0600`). Back it up separately and read the rotation note in [`SECURITY.md`](../SECURITY.md#credentials-and-secrets) — there is no online rotation |
| `ADMIN_EMAIL` | `admin@pingwaf.local` | Initial admin email |
| `ADMIN_PASSWORD` | **none — required on a fresh database** | Initial admin password. An empty value makes the first boot fail instead of seeding a known account; `deploy/compose/install.sh` generates a random one and prints it once. The dashboard forces a change at first login |
| `ALLOW_REGISTRATION` | `false` | Allow new user signups |
| `HEARTBEAT_INTERVAL` | `15` | Agent heartbeat interval (seconds) |
| `RUST_LOG` | `info,sqlx=warn` | Log level filter |

> `.env` holds the DSN, `PINGWAF_SECRET_KEY`, the JWT secret and the admin
> password. Keep it `chmod 600`, out of version control, and prefer a secret
> manager (systemd credentials, Kubernetes Secret, Docker secret) over a plain
> file.

## Manual Installation (Binary + systemd)

### 1. Install the Binary

The installer runs in two modes:

- **Interactive install** — download the script and run it on a terminal. With
  no arguments it asks, in order: deployment mode (all-in-one / server /
  agent), then for agent mode the control-plane gRPC URL and enrollment API
  key, or for server/all-in-one the administrator e-mail, the administrator
  password twice (leaving it empty generates a random password, printed once at
  the end), the PostgreSQL URL, the certificate SAN list of the dashboard, and
  the version to install. It then prints an **Installation plan** summary and
  asks `Continue? [Y/n]` before touching anything.
- **One-line unattended install** — any argument switches the script to
  fully non-interactive: it installs with the given values and defaults and
  never prompts. Because `curl | bash` has no terminal on stdin, piping the
  script installs non-interactively automatically; pass options through the
  pipe with `bash -s --`.

```bash
# Interactive: answer the questions, defaults in brackets
curl -fsSL https://raw.githubusercontent.com/shuaiZend/PingWAF/main/install.sh -o install.sh
bash install.sh

# One line, unattended (add sudo when the user cannot write /usr/local/bin)
curl -fsSL https://raw.githubusercontent.com/shuaiZend/PingWAF/main/install.sh | sudo bash -s -- --mode all-in-one

# Agent enrolled against a remote control plane. The gRPC listener serves
# TLS by default, so use https:// and point --server-ca-cert at the CA that
# signed the control plane's certificate (Settings → Control plane HTTPS):
curl -fsSL https://raw.githubusercontent.com/shuaiZend/PingWAF/main/install.sh | sudo bash -s -- \
  --mode agent --server-url https://10.0.0.1:9090 --api-key pwk_xxx \
  --server-ca-cert /path/to/grpc-ca.pem
```

Options beyond `--version`, `--mode`, `--server-url`, `--api-key`, `--db-url`
and `--no-systemd`:

| Option | Environment variable | Purpose |
| --- | --- | --- |
| `--admin-email` | `PINGWAF_ADMIN_EMAIL` | Seed administrator e-mail (server / all-in-one) |
| `--admin-password` | `PINGWAF_ADMIN_PASSWORD` | Seed administrator password (server / all-in-one); when empty a random password is generated and printed once |
| `--listen-public` | `PINGWAF_LISTEN_PUBLIC` | Also bind the agent gRPC endpoint (9090) on `0.0.0.0`; it stays on `127.0.0.1` by default and only remote agents need it opened. The dashboard/REST API (9080) already listens on every interface — restrict it to your admin network |
| `--tls-sans` | `PINGWAF_TLS_SANS` | Comma-separated SANs of the dashboard certificate |
| `--server-ca-cert` | `PINGWAF_SERVER_CA_CERT` | PEM file with the CA that signed the control plane's gRPC certificate (agent mode, https); readability is verified before installing |
| `--server-tls-name` | `PINGWAF_SERVER_TLS_NAME` | Name used for SNI and certificate verification when the certificate has no SAN for the URL host (agent mode; requires a release that knows the flag) |
| `--interactive` | — | Prompt even when options were given |
| `--non-interactive`, `--yes` | — | Never prompt; use the given values and defaults |
| `--dry-run` | — | Print the resolved plan, then exit without installing |

`curl | bash` cannot show interactive prompts, so secrets should be handed
over as environment variables (`PINGWAF_ADMIN_EMAIL` /
`PINGWAF_ADMIN_PASSWORD` / `PINGWAF_TLS_SANS`) or as flags — not typed into a
prompt that will never appear. Run with `--dry-run` first to check the plan
the script resolved from your options.

**What the installer verifies and generates** (fail-closed where noted):

- **Release checksum** — every release asset ships a `.sha256` sidecar. The
  installer downloads it and **aborts** when the sidecar is missing, malformed
  or does not match the archive; a tampered or truncated download is never
  installed.
- **Administrator password** — when `--admin-password` /
  `PINGWAF_ADMIN_PASSWORD` is empty, a random password is generated and printed
  **once** at the end of the run. There is no built-in default password.
- **`PINGWAF_SECRET_KEY`** — generated when unset and written to
  `/etc/pingwaf/pingwaf.env` (mode `0600`, owned by `pingwaf`); the shipped
  systemd unit reads it through `EnvironmentFile=-/etc/pingwaf/pingwaf.env`.
  An existing key is kept on upgrade: rotating it silently would make every
  sealed notification/AI/backup credential unreadable.
- **mimalloc arena** — the generated unit pins `MIMALLOC_ARENA_RESERVE` and
  `MIMALLOC_ARENA_EAGER_COMMIT` from the memory ceiling (cgroup v2/v1 limit,
  else `MemTotal`): 64 MiB below 1 GiB, 256 MiB below 4 GiB, 512 MiB up to
  16 GiB, else 1 GiB; eager commit is enabled only at 4 GiB and above. This is
  required because mimalloc creates its first arena before
  `allocator_tune::apply()` can set the option, and an existing arena cannot
  be shrunk. Override the values with `systemctl edit pingwaf` when the host
  has a non-standard memory layout.
- **Listen addresses** — the console listens on `0.0.0.0:9080` and the agent
  gRPC endpoint on `127.0.0.1:9090`. The console binds every interface so a
  fresh install is reachable from the operator's browser; restrict 9080 to your
  admin network (firewall or `admin_addr`) or front it with a TLS reverse proxy,
  and change the administrator password at the first sign-in — a fresh database
  refuses every write until you do. `--listen-public` (or
  `PINGWAF_LISTEN_PUBLIC=1`) opens gRPC too, which only remote agents need.

> Prebuilt release assets are published for **Linux** (amd64/arm64) only; on
> macOS, [build from source](./quick-start.md#path-2-build-from-source).
>
> Each release ships four archives per architecture:
> `pingwaf-linux-{amd64,arm64}.tar.xz` / `.tar.gz` for the control plane and
> all-in-one modes, and `pingwaf-agent-linux-{amd64,arm64}.tar.xz` / `.tar.gz`
> for the agent-only build (no PostgreSQL, REST API or embedded dashboard),
> each with a `.sha256` sidecar. `install.sh --mode agent` prefers the
> `pingwaf-agent` asset and installs it as `/usr/local/bin/pingwaf-agent`
> (`pingwaf-agent.service`). On an existing agent install, re-running the
> installer updates only the `[agent]` keys you pass explicitly in
> `/etc/pingwaf/pingwaf.toml` (that file wins over `PINGWAF_*`, and the unit
> carries no duplicate), keeping a timestamped backup; a release
> that predates it falls back to the full binary with a warning. The installer
> prefers the `.tar.xz` asset, skips straight to `.tar.gz` when `xz(1)` is not
> installed locally, and falls back on a failed download.
>
> The binaries are linked against the glibc of the runner that builds them
> (Ubuntu 24.04, **glibc 2.39**), so they run on Debian 12+/Ubuntu 24.04+ and
> equivalent. On older distributions (CentOS/RHEL 8, Debian 11, Ubuntu 22.04,
> Alpine, ...) `install.sh` stops with the loader's error — build from source
> there, or run the container image.

> **Manual tarball install.** The archive contains `pingwaf`, `pingwaf.service`
> and `pingwaf.toml` **inside** `pingwaf-linux-<arch>/`; copy the unit from
> there, not from the top of the extraction directory. Use `PINGWAF_DB_URL`
> (not `PINGWAF_DATABASE_URL`) for a non-default database, set
> `PINGWAF_SECRET_KEY` and an admin password before the first start, and keep
> the unit's `WorkingDirectory=/var/lib/pingwaf` so the relative
> `cache_dir = "./data/cache"` is writable under `ProtectSystem=strict`. A
> manual unit has no memory probe; pin `MIMALLOC_ARENA_RESERVE` on hosts below
> 4 GiB. The full checklist is in [quick-start Path 3](./quick-start.md#path-3-binary--systemd-production).

### 2. Set Up PostgreSQL

```bash
# Install PostgreSQL (use the distribution's default package; Ubuntu 26.04
# ships postgresql-18 and has no postgresql-16. Anything 14+ works.)
sudo apt install postgresql

# Create user and database (idempotent: skip an existing role/database)
sudo -u postgres createuser pingwaf 2>/dev/null || true
sudo -u postgres createdb -O pingwaf pingwaf 2>/dev/null || true
sudo -u postgres psql -c "ALTER USER pingwaf PASSWORD 'your-secure-password';"
```

### 3. Configure

Settings come from three places, each overriding the one before it: a TOML
file, `PINGWAF_*` environment variables, and command-line flags.

1. **A TOML file** — `--config /etc/pingwaf/pingwaf.toml` (or
   `PINGWAF_CONFIG`). `install.sh` writes one and points the unit at it; its
   `[server]` and `[agent]` tables hold the settings listed in the
   [Configuration Reference](#configuration-reference). The installer also
   bakes `admin_email`, `admin_password` and `tls_sans` (from the matching
   install options) into the file, and creates it owned by `pingwaf:pingwaf`
   with mode `640` — the seeded admin password and the JWT secret it contains
   are not world-readable.
2. **Environment variables** — set as `Environment=` lines in the systemd unit
   (a drop-in override with `sudo systemctl edit pingwaf` survives package
   upgrades).
3. **Command-line flags** — in the unit's `ExecStart`, e.g.
   `pingwaf all-in-one --tls-enabled false`.

```ini
[Service]
Environment=PINGWAF_CONFIG=/etc/pingwaf/pingwaf.toml
# Environment variables remain useful for the settings the file cannot
# express, and for secrets you would rather not put in a file:
Environment=PINGWAF_ALLOW_REGISTRATION=false
Environment=PINGWAF_DB_MAX_CONNECTIONS=40
Environment=RUST_LOG=info,sqlx=warn
```

The settings with no file key — `allow_registration`, `db_max_connections`,
`metric_retention_days`, `cors_origins`, the passkey settings and the
`PINGWAF_ES_*` Elasticsearch block — are listed under
[Environment-only settings](#server-configuration-server).

There is no reload signal: edit the file or the drop-in and
`sudo systemctl restart pingwaf`.

### 4. Start the Service

```bash
sudo systemctl enable --now pingwaf
sudo systemctl status pingwaf

# View logs
sudo journalctl -u pingwaf -f
```

## Configuration Reference

The `[server]` and `[agent]` tables of the configuration file
(`--config` / `PINGWAF_CONFIG`) take the long-flag names with dashes written as
underscores (`--admin-addr` → `admin_addr`); list values are TOML arrays. The
`all-in-one` mode reads both tables, `server` and `agent` only their own. A key
that is not recognised is reported and ignored, and a setting given both in the
file and in the environment is taken from the environment.

### Server Configuration (`[server]`)

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `db_url` | string | `postgres://pingwaf:pingwaf@localhost:5432/pingwaf` | PostgreSQL DSN |
| `admin_addr` | string | `0.0.0.0:9080` (CLI, installer and example config alike) | REST API + dashboard address; restrict it with a firewall or a reverse proxy |
| `grpc_addr` | string | `0.0.0.0:9090` (CLI default; the installer and example config use `127.0.0.1:9090` until `--listen-public`) | gRPC control plane address |
| `grpc_tls_mode` | string | `tls` | `tls` (default) or `off`. With `tls` and no certificate configured, a self-signed CA + server certificate is generated on first boot and reused after |
| `grpc_tls_cert` | string | — | PEM certificate for the gRPC listener; takes priority over the self-signed pair (requires `grpc_tls_key`) |
| `grpc_tls_key` | string | — | PEM private key for `grpc_tls_cert` |
| `public_host` | string | — | Host the dashboard is reached under from the outside (optionally with port); pinned into the HTTP-to-HTTPS redirect instead of the client's `Host` header |
| `jwt_secret` | string | `change-me-in-production` | JWT signing secret (required; 32 random bytes recommended, server floor 16 chars) |
| `admin_email` | string | `admin@pingwaf.local` | Seeded admin email |
| `admin_password` | string | `pingwaf123` | Seeded admin password. Only used to seed an empty `users` table; the compose file sets no default and the installers generate a random password when unset |
| `serve_frontend` | bool | `true` | Serve the embedded dashboard SPA for non-API routes |
| `tls_enabled` | bool | `true` | Serve the REST API + dashboard over HTTPS; cleartext requests get a `308` redirect, health probes are exempt. Turn off behind a TLS-terminating proxy |
| `tls_sans` | string[] | hostname, `localhost`, `127.0.0.1` | Subject alternative names of the generated self-signed certificate |

The rest of the server settings are **environment-only** — they have no flag
and therefore no file key:

| Environment Variable | Default | Description |
|----------------------|---------|-------------|
| `PINGWAF_JWT_EXPIRATION_HOURS` | `12` | Access token lifetime |
| `PINGWAF_ALLOW_REGISTRATION` | `true` (code); `docker-compose.yml` sets `false` | Allow new user signups |
| `PINGWAF_HEARTBEAT_INTERVAL` | `15` | Heartbeat interval handed to agents at registration. The agent-side `--heartbeat-interval-secs` flag reads the same variable and falls back to `30` |
| `PINGWAF_DB_MAX_CONNECTIONS` | `20` | Connection pool maximum |
| `PINGWAF_METRIC_RETENTION_DAYS` | `7` | Days of edge-metric samples kept in `agent_metrics` |
| `PINGWAF_CORS_ORIGINS` | `[]` (same-origin only) | Allowed CORS origins, comma-separated. With no value the API sends **no** CORS headers, so only same-origin callers work |
| `PINGWAF_SECRET_KEY` | unset | Instance key for sealed secrets (notification channels, AI provider key, backup-target credentials). Set 32 random bytes; unset = clear text. No online rotation — see [`SECURITY.md`](../SECURITY.md#credentials-and-secrets) |
| `PINGWAF_PASSKEY_ENABLED` | `true` | Offer passkey (WebAuthn) registration and login. Passkeys need a secure context: HTTPS under a hostname, or `localhost` |
| `PINGWAF_PASSKEY_RP_NAME` | `PingWAF` | Relying party name shown by the authenticator |
| `PINGWAF_PASSKEY_RP_ID` | derived from the request | Relying party ID, i.e. the effective domain |
| `PINGWAF_PASSKEY_ORIGIN` | derived from the request | Origin the dashboard is served from |
| `PINGWAF_PASSKEY_TRUST_FORWARDED_PROTO` | `false` | Believe `X-Forwarded-Proto` when deriving the origin |
| `PINGWAF_MALLOC_PURGE_DELAY_MS` | derived from memory ceiling | Allocator purge delay in **milliseconds** (mimalloc `mi_option_purge_delay`). Overrides the automatic bucketing (<1 GB → 0, 1-4 GB → 250, 4-16 GB → 500, >16 GB → 1000); `-1` disables purging. The standard `MIMALLOC_PURGE_DELAY` is honoured as a fallback |

> **Passkeys need a secure context.** WebAuthn only works over HTTPS under a
> hostname (or `localhost`), never plain `http://` or a bare IP. Behind a
> TLS-terminating proxy set `PINGWAF_PASSKEY_ORIGIN` to the public origin, and
> `PINGWAF_PASSKEY_TRUST_FORWARDED_PROTO=true` only when that proxy is the one
> terminating TLS. Ceremony state lives in the database, so several replicas can
> serve one ceremony without sticky sessions.

The refresh-token lifetime (30 days), the minimum pool size (`1`) and the
server-side log batch size (`500`) are compiled-in constants with no override.

### Agent Configuration (`[agent]`)

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `server_url` | string | `http://localhost:9090` (`--server-url`); the compiled-in struct default is `https://localhost:9090` and applies when the key is absent from the file | Control plane gRPC URL |
| `server_ca_cert` | string | — | PEM file with the CA that signed the control plane's TLS certificate (required for private CAs; bundled roots are used otherwise) |
| `server_tls_name` | string | — | Name used for SNI and certificate verification when the control plane's certificate carries no SAN for the host in `server_url`. Requires 0.32.17 or newer; on older binaries, point the SAN name at the control plane via `/etc/hosts` instead |
| `api_key` | string | `""` | Agent authentication key (empty = auto-register over loopback in all-in-one) |
| `cache_dir` | string | `./data/cache` (`--cache-dir`); the compiled-in struct default is `/var/lib/pingwaf/cache` and applies when the key is absent from the file | Local rule cache directory. On Unix the directory is `0700` and the snapshot files are `0600` (an older world-readable cache is tightened on load): the snapshot carries site TLS private keys, OIDC secrets and ACME credentials |
| `fail_open` | bool | `true` | Local fallback while disconnected: `true` keeps serving with the last synced rules; `false` answers 503 for hosts without synced rules. The per-site failover policy (console → site → basic) and the control-plane-wide default (settings → defense) take precedence once synced |
| `heartbeat_interval_secs` | int | `30` | Fallback heartbeat frequency, used only when the control plane hands no interval down at registration |
| `log_batch_size` | int | `100` | Log entries before flush |
| `log_flush_interval_secs` | int | `5` | Max time between flushes |
| `max_body_log_size` | int | `8192` | Max body bytes to log per direction (request and response), 64 KiB ceiling, `0` disables body capture. Bodies held for logging are capped at 32 MiB for the in-flight requests and 32 MiB for the log queue: above the default, a busy node logs some entries without their body, marked truncated |
| `metrics_ship_interval_secs` | int | `30` | Edge-metrics ship interval; `0` disables shipping |
| `auto_upgrade` | bool | `false` | Install the control plane's build automatically when the versions differ. Replaces the binary and restarts the process — see [Agent Self-Upgrade](#agent-self-upgrade) |
| `upgrade_url` | string | — | Control plane REST base (`https://cp.example.com:9080`) used only by the upgrade endpoints. Auto-upgrade stays disabled while this is unset |

Environment variables: `PINGWAF_SERVER_URL`, `PINGWAF_SERVER_CA_CERT`,
`PINGWAF_SERVER_TLS_NAME`, `PINGWAF_API_KEY`,
`PINGWAF_CACHE_DIR`, `PINGWAF_FAIL_OPEN`, `PINGWAF_HEARTBEAT_INTERVAL`,
`PINGWAF_METRICS_SHIP_INTERVAL`, `PINGWAF_AUTO_UPGRADE`,
`PINGWAF_UPGRADE_URL`. The three log settings and the reconnect
backoff constants (`1 s` doubling to `60 s`) have no environment variable.

In **agent mode the configuration file wins over these environment
variables** (command-line flags still win) — `install.sh` keeps `server_url`,
`api_key` and `cache_dir` in `/etc/pingwaf/pingwaf.toml` only and no longer
mirrors them into the systemd unit's `Environment=` lines, so an operator who
edits the file and restarts gets exactly what they wrote. When a file value
overrides a still-present environment variable the agent logs a warning with
the secret masked, so a legacy unit cannot silently shadow the file. Server
and all-in-one modes keep the opposite order (environment beats file).

### Environment Variables

Every setting can be given as an environment variable. The ones with a file key
are listed against it below; the dash in a table entry means the setting has no
`--flag` and is environment-only (or compiled in).

| Variable | File key |
|----------|-----------|
| `PINGWAF_CONFIG` | — (names the configuration file itself) |
| `PINGWAF_DB_URL` | `server.db_url` |
| `PINGWAF_ADMIN_ADDR` | `server.admin_addr` |
| `PINGWAF_HTTP_ADDR` | `server.admin_addr` (alias) |
| `DATABASE_URL` | `server.db_url` (used only when `PINGWAF_DB_URL` is unset) |
| `PINGWAF_GRPC_ADDR` | `server.grpc_addr` |
| `PINGWAF_JWT_SECRET` | `server.jwt_secret` |
| `PINGWAF_ADMIN_EMAIL` | `server.admin_email` |
| `PINGWAF_ADMIN_PASSWORD` | `server.admin_password` |
| `PINGWAF_SERVE_FRONTEND` | `server.serve_frontend` |
| `PINGWAF_JWT_EXPIRATION_HOURS` | — |
| `PINGWAF_ALLOW_REGISTRATION` | — |
| `PINGWAF_HEARTBEAT_INTERVAL` | `server.heartbeat_interval_seconds` (agents follow the value the control plane hands them; `agent.heartbeat_interval_secs` is only a fallback) |
| `PINGWAF_DB_MAX_CONNECTIONS` | — |
| `PINGWAF_METRIC_RETENTION_DAYS` | — |
| `PINGWAF_TLS_ENABLED` | `server.tls_enabled` |
| `PINGWAF_TLS_SANS` | `server.tls_sans` (comma-separated) |
| `PINGWAF_CORS_ORIGINS` | — (comma-separated) |
| `PINGWAF_CONTROL_PLANE_PEEK_TIMEOUT_SECS` | — (control-plane listener; default `5`, clamped `1`–`120`) |
| `PINGWAF_CONTROL_PLANE_HANDSHAKE_TIMEOUT_SECS` | — (control-plane listener; default `10`, clamped `1`–`120`) |
| `PINGWAF_PASSKEY_ENABLED` | — |
| `PINGWAF_PASSKEY_RP_NAME` | — |
| `PINGWAF_PASSKEY_RP_ID` | — |
| `PINGWAF_PASSKEY_ORIGIN` | — |
| `PINGWAF_PASSKEY_TRUST_FORWARDED_PROTO` | — |
| `PINGWAF_MODE` | CLI mode (`all-in-one`, `server`, `agent`) |
| `PINGWAF_SERVER_URL` | `agent.server_url` |
| `PINGWAF_SERVER_CA_CERT` | `agent.server_ca_cert` |
| `PINGWAF_SERVER_TLS_NAME` | `agent.server_tls_name` |
| `PINGWAF_API_KEY` | `agent.api_key` |
| `PINGWAF_CACHE_DIR` | `agent.cache_dir` |
| `PINGWAF_FAIL_OPEN` | `agent.fail_open` |
| `PINGWAF_LISTEN_HTTP_ADDR` | — (data-plane `:80` listener; default `[::]:80`, automatic `0.0.0.0:80` fallback) |
| `PINGWAF_LISTEN_HTTPS_ADDR` | — (data-plane `:443` listener; default `[::]:443`, automatic `0.0.0.0:443` fallback) |
| `PINGWAF_METRICS_SHIP_INTERVAL` | `agent.metrics_ship_interval_secs` |
| `PINGWAF_ES_ENABLED` | — Elasticsearch shipper toggle |
| `PINGWAF_ES_URLS` | — Elasticsearch URLs (comma-separated) |
| `PINGWAF_ES_INDEX_PREFIX` | — ES index prefix |
| `PINGWAF_ES_USERNAME` | — ES basic auth username |
| `PINGWAF_ES_PASSWORD` | — ES basic auth password |
| `PINGWAF_ES_API_KEY` | — ES API key |
| `PINGWAF_ES_MAX_BODY_SIZE` | — ES body truncation limit |
| `PINGWAF_ES_BUFFER_DIR` | — ES on-disk buffer directory |
| `RUST_LOG` | Tracing log filter (default `info,sqlx=warn`) |

## Deployment Modes

### All-in-One Mode

Runs both control plane and data plane in a single process. Best for single-server setups.

```bash
pingwaf all-in-one --db-url "postgres://..." --admin-addr 0.0.0.0:9080
```

### Distributed Mode

Separate control plane server and multiple edge agents.

**Control Plane (server):**
```bash
pingwaf server --db-url "postgres://..." --admin-addr 0.0.0.0:9080 --grpc-addr 0.0.0.0:9090
```

**Edge Agent:**
```bash
pingwaf agent --server-url "https://control-plane:9090" --server-ca-cert /var/lib/pingwaf/data/grpc-ca.pem --api-key "your-agent-key"
```

**Tunnel connector (optional, for services inside a private network):**

```bash
# on a host inside the private network; the token comes from
# Agents -> Tunnels -> Create tunnel
pingwaf-tunnel pair --config /etc/pingwaf/tunnel.toml \
  --token "<pairing-token>" --server-url "https://control-plane:9090" \
  --server-ca-cert /etc/pingwaf/tunnel-ca.pem
pingwaf-tunnel run --config /etc/pingwaf/tunnel.toml
```

The connector dials out only: it never needs an inbound port. On the edge side,
enable the listener and give that edge a public ingress address:

```toml
[agent]
tunnel_addr = "0.0.0.0:9443"
```

then set **Agents → [node] → Tunnel ingress** to `edge1.example.com:9443`.
Bind an origin pool to the tunnel in the site's origin pool settings; its node
addresses are then resolved by the connector inside the private network. See
[user-guide.md](user-guide.md#tunnels-internal-services-without-an-inbound-port).

In the compose deployment the listener is off unless the variable reaches the
container — compose only passes the environment a service declares, so add
`PINGWAF_TUNNEL_ADDR=0.0.0.0:9443` to the `pingwaf` service's `environment:`
block (the shipped file already forwards `${PINGWAF_TUNNEL_ADDR}` and publishes
`${PINGWAF_TUNNEL_BIND:-127.0.0.1}:9443:9443`), set
`PINGWAF_TUNNEL_BIND=0.0.0.0` when the connectors are on other hosts, and open
`9443` in the firewall and the cloud security group. A value in `.env` alone is
never passed through.

A connector host with **no direct egress** can reach both the control plane and
the edges through a SOCKS5 proxy: add `socks5_proxy = "10.0.0.1:1080"` (plus
`socks5_username` / `socks5_password` when the proxy requires them) to
`tunnel.toml`, or pass `--socks5-proxy` to `install.sh --mode tunnel`. Target
names are resolved by the proxy, the handshake is bounded, and a dial never
falls back to a direct connection.

The gRPC listener serves TLS by default, so `server_url` should be `https://`
with the CA that signed the control plane's certificate in
`server_ca_cert` (self-signed control plane: download the CA from
**Settings → Control plane HTTPS**). If the certificate's SANs do not include
the host agents dial, set `server_tls_name` to a SAN it does carry — the
parameter requires 0.32.17 or newer; on older binaries, point the SAN name at
the control plane in `/etc/hosts` instead.

Create the enrollment key from the console's **Nodes → Add node** flow (or
`POST /api/v1/agents/enroll`); it returns the install command. A generic
**Settings → API Keys** key is also accepted, but both are unbound: an agent
receives **every site owned by the key's owner**, not just one site. The
control plane's own dashboard site (typically `waf.<domain>` with upstream
`127.0.0.1:9080`) is included; on an edge that loopback has no dashboard and
the site returns 502. There is no per-node site binding yet, so use a
dedicated control plane or remove sites an edge must not run.

For ACME, the control plane must be **0.32.40 or newer**: that release syncs
the PEMs issued by its embedded data plane into `site_certificates` and pushes
them to the agents. An older control plane sends `acme_email` without a PEM,
so each agent starts its own HTTP-01 order; the CA validation request reaches
the host that owns port 80 (usually the control plane), not the agent, and the
order fails with `acme_failed`. If the control plane cannot be upgraded yet,
set `PINGAP_DISABLE_ACME=true` in the agent unit and upload certificates
instead. An agent that really is the HTTP-01 endpoint for its domains can
still issue locally, but a remote agent behind the control plane cannot.

The installer copies `--server-ca-cert` to `/etc/pingwaf/grpc-ca.pem` and
writes that durable path into the config/unit; the source file may live in a
temporary directory. The console's generated command does not include the CA
or `--server-tls-name`, so add them when the control plane uses its
self-signed certificate.

The log, certificate-event and metric shipping streams authenticate with the
agent token sent as `x-agent-token` gRPC metadata. **In distributed
deployments, upgrade the control plane first, then the agents** (see
[Rolling Upgrade](#rolling-upgrade-distributed)): a new server rejects shipping
streams that carry no token, while an old server simply ignores the metadata.

## Downstream Timeouts & Keep-Alive

These are per-listener settings in the `[servers.<name>]` tables of the data
plane's configuration file (the `conf/servers.toml` shipped with the repo
documents every key; `h2_idle_timeout` and the `tcp_*` keys sit in the same
table). They govern the client-facing side of the proxy:

| Key | Default | Meaning |
|-----|---------|---------|
| `downstream_read_timeout` | unset | Max gap between reads from the client. Unset, a slow client can hold a connection open indefinitely — the startup log warns when it is missing |
| `downstream_write_timeout` | unset | Max gap between writes to the client |
| `downstream_connection_timeout` | unset | **Read-timeout cap for the connection phase.** Pingora 0.9 has no first-read-only knob, so the effective read timeout is `min(this, downstream_read_timeout)`. It can only *tighten* the read timeout, never extend it; a value larger than the read timeout is ignored |
| `keepalive_request_limit` | unset (pingora default: no limit) | Maximum requests served on one downstream keep-alive connection before pingora closes it |
| `h2_idle_timeout` | unset (never) | Close an idle HTTP/2 client connection after this long |
| `tcp_idle` / `tcp_interval` / `tcp_probe_count` | unset | TCP keepalive probes on client connections |

`downstream_connection_timeout` and `keepalive_request_limit` exist because
pingora 0.9 has no independent HTTP/1.1 idle timeout: without them, a client
that opens a connection, completes the handshake and then goes quiet holds a
worker connection for as long as the read timeout (or forever, if read timeout
is unset).

Generated public listeners (`:80` / `:443`) default to 60s read/write, a
120s HTTP/2 idle timeout and a 1000-request keep-alive limit when the
configuration leaves these keys unset — an explicit value always wins, and
internal/generic listeners keep Pingora's own "unset" behaviour. The proxy
also applies a 60s request-phase fallback when a generic listener leaves
these keys unset; the initial request-line/header read still follows the
listener setting, so configure these keys explicitly for untrusted traffic. A site can override the
post-routing body/response budget under **Site → Basic → Downstream timeout**
(1–3600 seconds, default 60). The override applies after the site's Location
matches, so the request line/header phase still uses the listener value; on an
HTTP/1.1 keep-alive connection the listener value is restored before the next
request's headers are read. On HTTP/2 the write timeout is stored per stream, so the site value applies to
that stream's response; Pingora's H2 read-timeout setter is a no-op and H2
reads remain under Pingora's own connection handling.

Recommendations (starting points, not defaults):

- Set `downstream_read_timeout` (e.g. `60s`) and
  `downstream_write_timeout` (e.g. `60s`) unless a workload genuinely needs
  longer gaps.
- Set `downstream_connection_timeout` **below** the read timeout (it is a
  cap, not an independent budget) — most
  legitimate TLS handshakes finish in well under a second even on mobile
  networks, so `5s`–`10s` comfortably separates stalled handshakes from slow
  clients. Note the effective first-read timeout is the **shorter** of the
  two, so a connection timeout larger than the read timeout has no effect.
- Set `keepalive_request_limit` to a finite value (e.g. `1000`) to bound
  connection age; behind a load balancer that reuses connections
  aggressively, a lower value spreads traffic across workers more evenly.

## Upstream Host Override

By default the proxy forwards the client's `Host` header to the origin
unchanged. A location can override which virtual host the origin sees with
`upstream_host` on the location (`LocationConf.upstream_host`, configurable
through the location form / `PUT /api/v1/sites/{site_id}` locations):

- Value is a **bare host name** — no scheme, no path, no spaces (validation
  rejects `http://…`, `/…` and anything over 253 characters). A port is not
  part of the Host the origin needs for virtual-host selection either; the
  upstream address determines where to connect, `upstream_host` only rewrites
  the header.
- Unset (the default) keeps the client's `Host` — historic behaviour.
- An explicit value **wins over** any `Host` entry in the location's
  `proxy_set_headers`, so "which virtual host the origin sees" has one
  obvious knob instead of competing header rewrites.

Typical use: a pool of origins behind one address that route by
`Host` (virtual hosts, multi-tenant origins) — set `upstream_host` to the
name the origin expects instead of asking every client to send it.

## Origin gzip (`upstream_accept_encoding`)

By default the proxy injects `Accept-Encoding: gzip` into requests it sends
to origins when the **client sent no `Accept-Encoding`**. The switch is the
`[basic] upstream_accept_encoding` key of the generated proxy configuration
(default `gzip`; set it to `""` to disable the injection, and no other value
is accepted — validation rejects anything but `gzip` and the empty string).

The injection is skipped when the client explicitly said `identity`, and for
messages that cannot carry an encoded body: `HEAD`, requests with a `Range`
header, and `Upgrade`/`CONNECT` exchanges (the WebSocket key header covers
the HTTP/2 handshake).

What happens to a compressed origin response depends on the client:

- **The client supports gzip** — the origin's compressed response is passed
  through untouched.
- **The client cannot decode gzip** — the proxy decompresses the body and
  serves the plaintext: `Content-Encoding` and `Content-Length` are removed,
  the response becomes chunked, a strong `ETag` is weakened,
  `Content-MD5`/`Digest` are dropped and `Vary` gains `Accept-Encoding`.
- Decompression is capped at **8 MiB** of plaintext (compression-bomb
  protection). Over the cap, or on a decompression failure, the original
  compressed bytes are passed through as-is and a warning is logged.
- Decompressed responses carry `Vary: Accept-Encoding`; a cache whose
  `vary_headers` allowlist omits `accept-encoding` will treat them as
  uncacheable.

Two interactions worth knowing:

- The decompression runs **before** body-rewriting plugins, so a `sub_filter`
  in the chain sees plaintext and behaves as if the origin never compressed.
- The `compression` plugin does not compress a second time: it only triggers
  for clients that advertise the encoding.

## Release & Build Constraints

Facts worth knowing before you build, ship or debug a release — the
reasons live in the sources this section points at.

### tokio is pinned to `=1.53.3`

The workspace still pins tokio to exactly `1.53.3`
(`[workspace.dependencies]` in the root `Cargo.toml`, comment included)
because the tokio upgrade has not been validated on its own. The original
reason is gone: tokio 1.54.0 added `assert!(capacity > 0)` to
`BufReader::with_capacity` / `BufStream::with_capacity`, and pingora 0.9.0
constructed both with a hardcoded `(0, 0)`, which panicked on the **first
accepted connection** of the data plane
([cloudflare/pingora#1059](https://github.com/cloudflare/pingora/issues/1059)).
**Pingora 0.9.1 contains that fix**, so raising the pin to `>= 1.54` is now
a normal dependency change: do it in a dedicated commit, run the data-plane
tests, and remove the matching note in `.github/dependabot.yml`.

### CI shape

`.github/workflows/ci.yml` runs the test matrix on two toolchains
(`1.96.0` MSRV and stable) with nextest (doctests go through a separate
`cargo test --doc` step, which nextest does not execute). Docs-only PRs
skip the heavy rust/web jobs but never the static `docs` job; a
`web-dist` job builds the dashboard once and a `release-binary` job on
`main` pushes builds the shipped binary outside the test matrix. Release
images/manifests are built by `release.yml` on a `v*` tag, with per-arch
jobs pinned to `ubuntu-24.04` / `ubuntu-24.04-arm` runners so both
architectures build on the same distro.

## Small-Memory Instances

On instances with less than 2 GiB of RAM (especially all-in-one deployments
that share the box with PostgreSQL):

- **Cap the container's memory.** Prevent a single process from squeezing out
  the database on the same host:
  `docker run --memory=1g ...` (the same flag works with podman), or in
  Compose:

  ```yaml
  services:
    pingwaf:
      mem_limit: 1g
  ```

- **The allocator returns freed memory to the OS.** PingWAF links
  [mimalloc](https://github.com/microsoft/mimalloc) as its global allocator on
  every platform (previously only musl builds used it; glibc builds went
  through the system allocator, whose per-thread arenas could pin tens of
  megabytes of RSS). Freed pages are purged back to the kernel instead of
  lingering until a restart, and the purge delay now **derives from the memory
  ceiling at startup** (cgroup limit, then host total):

  | Memory ceiling | Purge delay |
  | --- | --- |
  | < 1 GB | 0 ms (return pages immediately) |
  | 1-4 GB | 250 ms |
  | 4-16 GB | 500 ms |
  | > 16 GB | 1000 ms |

  Small hosts return pages aggressively (a little more CPU for a materially
  lower RSS); large hosts keep mimalloc's lazier default. Pin an explicit
  value with `PINGWAF_MALLOC_PURGE_DELAY_MS` (milliseconds, `-1` = never
  purge); the standard `MIMALLOC_PURGE_DELAY` is honoured as a fallback. The
  effective value is logged at startup
  (`pingwaf: allocator retention purge_delay=...`).
- **Set transparent hugepages to `madvise`** on the host kernel
  (`/sys/kernel/mm/transparent_hugepage/enabled`); `always` lets the
  hugepage backing inflate resident memory.
- **Swap is a buffer, not a substitute.** Configuring swap absorbs slow growth,
  but a sudden burst allocates faster than the kernel can swap out, so the OOM
  killer still fires first — keep the memory cap.

To observe actual memory usage, call `GET /api/v1/system/metrics` — the
`memory` object carries the process RSS, the host totals and the cgroup
figures the dashboard reads; see
[System observation](./profiling.md#get-apiv1systemmetrics).

## SSL/TLS Configuration

### Automatic (ACME / Let's Encrypt)

Configure through the dashboard:
1. Add a site with your domain
2. Go to **SSL** tab
3. Select "Let's Encrypt" and choose challenge type:
   - **HTTP-01**: Requires port 80 accessible
   - **DNS-01**: Supports Cloudflare, AliDNS, Huawei DNS, Tencent DNS

Certificates are automatically renewed before expiry.

In **all-in-one** mode the embedded data plane is the ACME issuer. The control
plane mirrors the PEMs it issues (from the data plane's `acme_state.toml`) into
the ACME-managed `site_certificates` rows every 30 seconds and pushes the
updated bundle to every agent, so an edge agent whose port 80 points at the
control plane does not have to issue (and fail) its own HTTP-01 order. Set
`PINGWAF_ACME_STATE` to override the file path. A remote agent that is itself
the HTTP-01 endpoint can still issue locally; use **DNS-01** when it is not the
instance owning the domain's port 80.

### Manual Certificates

Upload PEM-encoded certificate and key through the dashboard or API:

```bash
curl -k -X POST https://localhost:9080/api/v1/sites/{site_id}/certificates \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "domain": "example.com",
    "cert_pem": "-----BEGIN CERTIFICATE-----\n...",
    "key_pem": "-----BEGIN PRIVATE KEY-----\n..."
  }'
```

### Origin Certificate Verification

When a site's origin pool has an SNI hostname, traffic to the origin goes over
HTTPS. Whether that origin's certificate is validated is a separate decision from the
certificate the proxy serves to clients. The effective value resolves through a
three-level cascade — the first level that is set wins:

1. **Per pool** — an origin pool's own `verify_cert` (origin pool dialog).
   `true` forces validation, `false` forces acceptance, and leaving it unset
   falls through to the site default.
2. **Per site** — the site's `origin_verify_cert` (origin certificate check
   card on the site's SSL page, or `origin_verify_cert` on
   `PUT /api/v1/sites/{site_id}`). `true`/`false` pins a default for every
   pool of the site; `null` (no site preference) falls through to the global
   default.
3. **Global default** — **Settings → Origin TLS verification** in the
   dashboard, or `PUT /api/v1/settings/origin`. When neither the pool nor the
   site sets a value, the data plane treats the origin certificate as **must
   verify** (strict): self-signed and internal-PKI origins need `false` here
   (or at site/pool level) to keep working. The value reaches the agents
   through the next rule bundle, so no restart is needed.

Accepting an unvalidated origin certificate means an attacker who can
intercept the connection to the origin can impersonate it. Prefer issuing the
origin a certificate from a CA the proxy trusts (a private CA works: upload it
as the pool's `ca` file) and enabling verification, rather than disabling it.

When the deployment does not state a value at any of the three levels, the
agent's generated configuration now states **strict verification**
(`origin_verify_cert = true`) instead of leaving the setting unset — see the
[upgrade notes](#upgrade-procedure) for the 502 this can produce on
self-signed-origin deployments.

**`$host` SNI depends on this switch staying on.** An origin pool may set its
SNI to `$host`, which sends each request's client `Host` as the TLS SNI — the
client then effectively chooses the name the origin certificate must cover,
and origin certificate verification is the only backstop against an
intercepted origin. A pool that pins `verify_cert: false` together with
`sni = "$host"` is rejected by the API
(`400 "$host requires origin certificate verification"`); leaving
`verify_cert` unset follows the cascade, so turning verification off at site
or global level removes the same guarantee for every `$host` pool. Keep the
effective cascade value at verification-on wherever `$host` pools are used.

### Control-Plane Certificate (dashboard)

The dashboard and REST API are served over HTTPS with a certificate that is
managed from the dashboard itself — no files to edit:

1. **First boot** generates a self-signed pair (EC P-256) covering the
   configured `tls_sans` plus the machine hostname, `localhost` and `127.0.0.1`.
2. **Settings → Control plane HTTPS** shows what is being served (subject,
   SANs, fingerprint, expiry) and lets you download it, upload a real
   certificate + key, or regenerate the self-signed pair.
3. Uploads are validated first — a mismatched key or a broken chain is rejected
   with `400` and the previous certificate keeps serving — and take effect
   immediately, without a restart.

`PINGWAF_TLS_ENABLED=false` (or `tls_enabled = false`) switches the port back
to plain HTTP, which is the right setting behind a reverse proxy that
terminates TLS. Passkeys require a secure origin, so with plain HTTP they are
unavailable unless the proxy publishes an HTTPS origin and
`PINGWAF_PASSKEY_TRUST_FORWARDED_PROTO=true` is set.

### gRPC Control-Plane TLS

The gRPC listener agents connect to (default `0.0.0.0:9090`) serves **TLS by
default**. With no certificate configured, the first boot generates a
self-signed CA plus server certificate (SANs: `localhost`, the hostname and
the listener IPs), persists it in `instance_settings` and writes the PEM files
to the data directory — the agent then only needs that CA to verify the
server. The generated pair survives restarts; deleting it makes the next boot
issue a fresh one (agents must be re-pointed at the new CA).

For a publicly trusted certificate (e.g. issued for the control plane's
hostname), point the server at a PEM certificate and key — explicit
certificates take priority over the self-signed pair:

```bash
pingwaf server \
  --grpc-tls-cert /etc/pingwaf/certs/grpc.pem \
  --grpc-tls-key /etc/pingwaf/certs/grpc-key.pem
```

Or set `grpc_tls_cert` / `grpc_tls_key` in `pingwaf.toml`, or the
`PINGWAF_GRPC_TLS_CERT` / `PINGWAF_GRPC_TLS_KEY` environment variables. Both
paths must be present; a mismatch or unreadable file aborts startup instead of
falling back to plaintext.

`grpc_tls_mode = "off"` (or `PINGWAF_GRPC_TLS_MODE=off`) reverts to plaintext
h2c. The server logs a warning at startup and `GET /version` reports
`grpc_tls: false`, which the console surfaces as a degraded-mode banner —
acceptable for loopback all-in-one deployments, not for agents across
untrusted networks.

Agents switch to `https://` server URLs and either trust the bundled webpki
roots (publicly trusted certificate) or load the signing CA — the generated
self-signed CA when the server auto-generates:

```bash
pingwaf agent \
  --server-url "https://control-plane.example.com:9090" \
  --server-ca-cert /var/lib/pingwaf/data/grpc-ca.pem \
  --api-key "your-agent-key"
```

The certificate must carry a subject alternative name for whatever agents
connect to: a DNS name for domain URLs, an IP SAN for `https://<ip>:9090`, and
`127.0.0.1` for all-in-one mode — which switches its embedded agent to
`https://127.0.0.1` automatically and wires up the generated CA, so a default
all-in-one boots fully encrypted with no extra configuration.

The API for all of this is documented in
[`docs/api.md` → Control-plane certificate](./api.md#control-plane-certificate).

### Agent Can't Reach the Control Plane

**Symptoms.** The agent's journal shows a repeating
`Connection to control plane failed` with one of:

- `GoAway(b"", FRAME_SIZE_ERROR, Library)` — a plaintext h2 preface hitting a
  TLS port (e.g. an `http://` server URL against the default TLS listener);
- `UnknownIssuer` — the server's certificate is signed by a CA the agent does
  not trust (typically a self-signed control plane with no `server_ca_cert`);
- `NotValidForName` — the certificate has no SAN for the host the agent dials.

Besides `error` and `delay_ms`, the failure log now carries a `hint="..."`
field, derived from the error chain, that names the most likely
misconfiguration:

| `hint` (abridged) | Meaning | Remedy |
|-------------------|---------|--------|
| `server_url uses http:// but ... serves TLS` | plaintext `http://` against the TLS gRPC port | switch to `https://` and set `server_ca_cert` |
| `certificate is not trusted` | the signing CA is not trusted by the agent | download the control plane certificate (Settings → Control plane HTTPS) and set `agent.server_ca_cert` |
| `certificate does not name the host` | no SAN for the host in `server_url` | set `agent.server_tls_name` to a SAN the certificate carries |
| unreachable variants (connection refused / tcp connect error / timed out / connection reset) | the control plane cannot be reached at all | check `server_url`, port 9090 reachability and the firewall |

An empty `hint` means the error was not recognized as a known configuration
problem — the original `error` and backoff `delay_ms` fields are unchanged
either way.

**Diagnose:**

```bash
# Is 9090 actually TLS, and does the certificate match the host?
openssl s_client -connect <host>:9090 -servername <host> -alpn h2

# Which SAN does the certificate carry (compare against server_url)?
openssl s_client -connect <host>:9090 -servername <host> -verify_hostname <host> </dev/null

# Is a legacy unit still exporting PINGWAF_* overrides? Agent mode gives the
# configuration file precedence over the environment, but the warning in the
# journal names the shadowed variable — check what the unit carries:
systemctl show pingwaf -p Environment
```

`install.sh` prints the same three-part guidance when it sees a non-loopback
`http://` server URL — the symptom to expect in the journal, the fix
(`--server-url https://<host>:9090 --server-ca-cert <pem>`, plus
`--server-tls-name <SAN>` when the certificate does not name the host) and the
`openssl s_client -alpn h2` verification command. It is a **warning only**: the
installer does not block the install, so a plaintext agent URL can still end up
in the configuration file.

**Fix:**

1. Use `https://` in `server_url` (the gRPC listener serves TLS by default)
   and set `server_ca_cert` to the CA that signed the control plane's
   certificate (self-signed control plane: **Settings → Control plane HTTPS**).
2. If the certificate has no SAN for the URL host, set `server_tls_name` to a
   SAN it does carry (requires 0.32.17 or newer; on older binaries, point the
   SAN name at the control plane via `/etc/hosts` as a transition).
3. Remove stale `PINGWAF_*` overrides from the unit (`systemctl edit pingwaf`)
   and restart: `sudo systemctl restart pingwaf`.

Do **not** work around this with `grpc_tls_mode = "off"` across an untrusted
network — that sends agent traffic and credentials in plaintext.

## Firewall & Port Requirements

| Port | Protocol | Purpose | Required |
|------|----------|---------|----------|
| 80 | TCP | HTTP traffic / ACME HTTP-01 | Yes (for proxied sites) |
| 443 | TCP | HTTPS traffic | Yes (for proxied sites) |
| 9080 | TCP | Admin dashboard + REST API (HTTPS) | Yes |
| 9090 | TCP | gRPC control plane (agents and tunnel connectors) | Distributed mode |
| 9443 | TCP | Tunnel listener (`agent.tunnel_addr`) | Only on edges that terminate tunnels |
| 5432 | TCP | PostgreSQL | Internal only |

```bash
# UFW example — restrict the management plane to trusted sources.
sudo ufw allow 80/tcp
sudo ufw allow 443/tcp
sudo ufw allow from 203.0.113.10 to any port 9080 proto tcp  # admin/VPN only
sudo ufw allow from 203.0.113.0/24 to any port 9090 proto tcp # agent nodes only
sudo ufw allow 9443/tcp                                       # tunnel connectors (see below)
```

`9443` is the tunnel listener and is **off by default**: only an edge that an
operator explicitly made a tunnel ingress accepts connector sessions
(`agent.tunnel_addr`, or `--tunnel-addr 0.0.0.0:9443`). It must be reachable
from wherever the connectors run — that is the whole point of the tunnel, and
the only port a private-network host needs to reach. The connectors themselves
need **outbound** 9090 (control plane) and 9443 (ingress edges) and nothing
inbound.

`9080` is the authenticated admin console, not a public API. Prefer binding it
to a private interface or loopback and reaching it through a VPN/SSH tunnel
instead of publishing it on `0.0.0.0`. `9090` should likewise be reachable only
from agent hosts. The console's TLS listener deliberately caps protocol
detection at 5s and TLS handshake at 10s, keeps at most 128 pre-handshakes in
flight, closes excess connections, caps active HTTP connections at 512 with a
60s idle read/write deadline, and reserves a small loopback pool for local
health probes; it does not rely on the kernel accept queue for backpressure.
The Compose healthcheck uses `curl --connect-timeout 2 --max-time 3`; use the
same bounded probe in an external watchdog rather than an unbounded `curl`.

### Login Rate Limiting

The control plane rate-limits its credential endpoints (`login`, passkey
login, `register`, token `refresh`, password change) per source address:
10 login attempts/min, 5 registrations/hour, 30 refreshes/min, 10 password
changes/min. The limit is keyed by the real TCP peer address — forwarding
headers are never trusted — so a proxy in front of the dashboard must not be
shared by other clients that would otherwise be throttled together. Behind a
large NAT exit (offices, campus networks) all users share one address and can
exhaust the login budget with legitimate traffic; give the dashboard a
dedicated ingress address in that case. Exceeding a limit answers `429` with
a `Retry-After` header. These token buckets are counted **in-process**: the
counters are not shared across control-plane instances, so in a multi-replica
deployment each replica enforces the limits independently and the effective
budget per source address is the configured threshold multiplied by the
number of replicas — scale the thresholds up accordingly.

## Security Checklist

A hardening pass before exposing the control plane beyond localhost:

- **Change the default administrator.** First boot seeds
  `admin@pingwaf.local` / `pingwaf123` — the server logs a warning while that
  password is active, and the dashboard forces a password change on first
  login, but do not leave the seed in place. `pingwaf user add-admin` can
  create administrators before boot instead.
- **Set `PINGWAF_SECRET_KEY`.** 32 random bytes (`openssl rand -hex 32`); it
  seals notification-channel secrets, the AI provider key and backup-target
  credentials. Without it those fields are stored in clear text. Back it up
  separately and read the rotation note in
  [`SECURITY.md`](../SECURITY.md#credentials-and-secrets) before changing it —
  there is no online rotation.
- **Protect the secrets file.** `.env` (DSN, `PINGWAF_SECRET_KEY`, JWT secret,
  admin password) must be `chmod 600` and out of version control; prefer a
  secret manager (systemd credentials, Kubernetes Secret, Docker secret).
- **Give the control plane a real JWT secret.** A placeholder secret (the
  `change-me-…` default or the compose value) makes the server generate and
  persist a random one on first boot, which is safe but rotates whenever the
  persisted copy is lost; set `PINGWAF_JWT_SECRET` explicitly when tokens
  must survive re-provisioning or be shared across instances.
- **Restrict CORS.** With no `PINGWAF_CORS_ORIGINS` the API sends no CORS
  headers at all, so only same-origin callers work — the safe default when the
  embedded dashboard is the only client. When a split-origin client must call
  the API, list its exact origin(s) explicitly; never use a wildcard.
- **Keep gRPC TLS on for cross-network agents.** It is the default: a
  self-signed CA + server certificate is generated on first boot and agents
  verify against it. Replace it with a publicly trusted certificate where
  possible; only loopback all-in-one deployments may consider
  `grpc_tls_mode = "off"`, which logs a warning and shows a degraded banner.
- **Keep PostgreSQL off public interfaces.** The compose file binds 5432 to
  loopback by default; set `POSTGRES_BIND=0.0.0.0` only when another host
  genuinely needs the database, and then with a strong password and
  firewall rules in front of it.
- **Choose `fail_open` deliberately.** Disconnected behaviour is resolved per
  host: the site's failover policy (console → site → basic settings) wins,
  then the control-plane-wide default (settings → defense), then the agent's
  local `--fail-open` flag. `true` (default at every layer) keeps sites
  serving with the last synced rules while the agent is disconnected;
  `false` refuses traffic (`503`) for hosts without synced rules, trading
  availability for guaranteeing the WAF sees every request. With a closed
  policy, remember the window between agent start and first sync refuses
  traffic for all hosts.
- **Verify origin certificates where you can.** The console defaults
  `Settings → Origin` to off, which accepts any certificate from an HTTPS
  origin and therefore allows an on-path attacker to impersonate it. Turn it
  on once origins present a certificate the proxy can chain — a private CA
  works, uploaded as the pool's `ca` — and keep per-pool overrides for the
  few origins that cannot.
- **Treat cache rules as shared by default.** A response is only cached when
  the origin allows it, but a cache rule toggled on with a long TTL will
  serve one user's response to the next unless the origin marks it
  personalised (`private`, `no-store`, `Cache-Control: no-cache`) or sends
  `Vary` on the request dimensions. Responses carrying `Set-Cookie` and
  requests carrying `Authorization` are excluded unless the origin opts in
  explicitly, so origins that authenticate through a header other than
  `Authorization` rely on you marking those responses correctly.
- **Session revocation semantics.** Accounts carry a token version;
  changing a password, disabling an account, or changing its role bumps the
  version and immediately invalidates that account's access and refresh
  tokens. Password changes by the user do the same for their own sessions, and
  so does `pingwaf user reset-password` (the forced-change flag, if the account
  carried one, is left as it is).

## Backup & Recovery

### Database Backup

```bash
# Full backup
pg_dump -U pingwaf -d pingwaf -F c -f pingwaf_backup_$(date +%Y%m%d).dump

# Restore
pg_restore -U pingwaf -d pingwaf --clean pingwaf_backup_20240101.dump
```

> **The database is a secret store.** `site_certificates` holds certificate
> **private keys**, `site_basic_auth` holds credentials in plaintext,
> `site_oidc` holds cookie secrets, and clones copy all of that material to
> every site they were created from — one compromised backup exposes the
> secrets of an entire site family. Encrypt backups at rest and restrict
> read access like any other secret store.

### Configuration Backup

```bash
tar -czf pingwaf_config_backup.tar.gz /etc/pingwaf/ /var/lib/pingwaf/certs/
```

### Automated Backups (cron)

```cron
# Daily at 2:00 AM
0 2 * * * pg_dump -U pingwaf -d pingwaf -F c -f /backups/pingwaf_$(date +\%Y\%m\%d).dump
# Remove backups older than 30 days
0 3 * * * find /backups -name "pingwaf_*.dump" -mtime +30 -delete
```

## Maintenance CLI (break-glass)

`pingwaf` carries four maintenance command families that talk **straight to
PostgreSQL** instead of the REST API, so they keep working when the control
plane is down, has not been seeded yet, or its own IP allowlist has locked the
operator out — which is exactly when they are needed. Each one connects with
`--db-url` (env `PINGWAF_DB_URL`, default
`postgres://pingwaf:pingwaf@localhost:5432/pingwaf`), does its work and exits.
Run `pingwaf <command> --help` for the exact options of any of them — plain
`pingwaf --help` answers with the `pingap`-compatible options of the shared
binary, not with this subcommand tree.

> **Complete reference:** every maintenance command, its options, the danger
> notes (`--force`, CLI rollback semantics) and incident runbooks live in the
> [CLI handbook](./cli.md) ([中文](./zh/cli.md)). The table below is only a
> summary — keep that page as the source of truth for details.

| Command | Purpose |
| --- | --- |
| `pingwaf user list` · `add-admin --email <addr> [--name <name>] [--password <pw>]` · `reset-password --email <addr> [--password <pw>]` | Dashboard accounts. An omitted password is generated and printed once; `reset-password` also revokes that account's sessions. |
| `pingwaf mode observe` · `mode enforce` · `mode status` | The global observation mode: detect everything, block nothing. |
| `pingwaf security status` · `security allowlist on\|off [--force]` | The 9080 self-protection switches. `--force` enables the allowlist even when it is empty, which refuses every API caller. |
| `pingwaf config history` · `config show` · `config rollback` | The configuration version history (see below). |

### `pingwaf config`

```bash
pingwaf config history [--site <uuid|global>] [--limit <n>]   # newest first, default 20
pingwaf config show <version> [--json]                        # metadata + snapshot summary
pingwaf config rollback <version> [--yes]                     # prompts unless --yes
```

`history` lists recorded versions newest first and filters them to one site
(its UUID) or to the deployment-wide settings (`--site global`); `--limit`
caps how many are printed (default 20). `show` prints one version's metadata
plus a snapshot summary, or the whole snapshot as JSON with `--json`.
`rollback` restores a version and records the restore as a new version
attributed to `cli`, so rolling back a rollback works and the history stays
linear.

**A CLI rollback does not notify a running control plane.** It writes the rows
directly to PostgreSQL, so agents pick the restored configuration up on their
next full sync or restart — use the console rollback
(`POST /config-versions/{id}/rollback`, see [docs/api.md](./api.md)) when the
change has to reach the fleet immediately.

## Upgrade Procedure

Take a backup before every upgrade, and pin the exact version you deploy —
`:latest` makes an upgrade an unauditable drift and leaves you without a known
rollback target.

> **Console listen address.** New installs bind the console on `0.0.0.0:9080`.
> An upgrade never rewrites an existing configuration, so an installation
> created before that change keeps `admin_addr = "127.0.0.1:9080"` (and compose
> keeps the old `.env` value) and stays reachable only from the host itself.
> Set `admin_addr = "0.0.0.0:9080"` — or `PINGWAF_ADMIN_BIND=0.0.0.0` in
> `.env` — when you want the new behavior, then restrict the port with a
> firewall or a reverse proxy.

### Docker

```bash
cd /path/to/compose            # the directory holding docker-compose.yml and .env
# 1) Snapshot: database, environment (secrets) and the compose file.
docker compose exec -T postgres pg_dump -U pingwaf -d pingwaf -Fc > pre-upgrade.dump
cp .env pre-upgrade.env && chmod 600 pre-upgrade.env
cp docker-compose.yml pre-upgrade-compose.yml
# 2) Pin the new version (edit PINGWAF_TAG in .env) and recreate only the app.
docker compose pull pingwaf
docker compose up -d pingwaf
# 3) Verify.
docker compose ps
curl -sk -o /dev/null -w '%{http_code}\n' https://localhost:9080/healthz   # expect 200
```

### Binary

```bash
# Download and install new version
./install.sh --version NEW_VERSION

# Restart service
sudo systemctl restart pingwaf
```

Database migrations run automatically on startup.

### Rollback

A failed upgrade is recovered by going back to the previous version, not by
forward-fixing in place:

```bash
# Docker: put the previous tag back in .env, then recreate the app.
#   PINGWAF_TAG=<previous-version>
docker compose up -d pingwaf
# Binary: reinstall the previous version and restart.
./install.sh --version PREVIOUS_VERSION && sudo systemctl restart pingwaf
```

Migrations are **forward-only**: the previous binary may refuse to start against
a database a newer release has already migrated. If that happens, restore the
`pre-upgrade.dump` taken above (and the matching `.env`) before starting the old
version. Keep the dump until the new version has served traffic for a full day.

> **Installer upgrades keep your secrets and volumes.** The compose installer
> reuses the administrator password and `PINGWAF_SECRET_KEY` from the existing
> `.env` on `--force`, and never deletes `postgres-data` / `pingwaf-data` /
> `pingwaf-certs`; the binary installer keeps `/etc/pingwaf/pingwaf.env` (the
> instance key). If you need to reclaim host disk, the only volume-pruning path
> is `scripts/sweep-host-storage.sh --prune-volumes`, which removes unused
> **non-PingWAF** volumes only — names starting `waf_` are never touched.
>
> A compose file downloaded from an **older release tag** does not carry the
> install hardening (loopback binds, `PINGWAF_SECRET_KEY` pass-through, no
> default admin password). The installer warns about it; upgrade to a release
> whose compose file includes those changes rather than assuming the
> guarantees apply.

> The control plane and its embedded agent are one process in `all-in-one` mode,
> so an upgrade briefly interrupts the data plane (seconds). Distributed
> deployments upgrade the control plane first, then let agents pick up the new
> bundle; see [Agent Self-Upgrade](#agent-self-upgrade).

### Agent Self-Upgrade

A distributed deployment has two moving parts, and the console's fleet view now
shows both. To let the edges follow the control plane by themselves:

```toml
[agent]
auto_upgrade = true
upgrade_url = "https://cp.example.com:9080"   # the REST API, not the gRPC port
```

or `PINGWAF_AUTO_UPGRADE=true PINGWAF_UPGRADE_URL=https://cp.example.com:9080`.

What happens: every 5 minutes (first check after 1 minute) the agent asks
`/api/v1/agent-upgrade/manifest` what this control plane is running. If the
version differs it downloads **the control plane's own binary** — PingWAF is one
binary whose role comes from `PINGWAF_MODE`, so that is the artifact to install
— verifies `size` and `sha256` against the manifest, keeps the current binary as
`<exe>.prev`, then re-execs itself. In-flight connections drop at that moment;
that is the cost of an in-place restart, and the reason this is opt-in.

Properties worth knowing before turning it on:

- It only trusts the control plane that authenticated the agent: the artifact is
  served over the same API, with the same token, and there is no release feed.
- A digest or size mismatch aborts before anything is replaced, and a failed
  download leaves the running build untouched.
- Rollback is manual and local: stop the service, `mv <exe>.prev <exe>`, start.
- The certificate must cover the host in `upgrade_url` — the agent verifies it
  with the same CA (`server_ca_cert` / the injected all-in-one PEM) the gRPC
  channel uses. Point `upgrade_url` at a name in the certificate, not at the raw
  IP of a self-signed control plane.
- **Containers and all-in-one are out of scope.** A container's binary lives in
  an image layer, so the replacement is lost on the next `docker compose up`;
  update containers by pulling a new tag. In all-in-one the embedded agent
  shares the control plane's process, so upgrading would restart the control
  plane and the flag is ignored there by construction.

> ⚠️ **Do not upgrade with `--no-systemd` on an older unit.** Older versions of
> this document suggested `./install.sh --version NEW_VERSION --no-systemd` to
> skip touching the unit file. That skips the rewrite, so a unit installed
> before the `CAP_NET_BIND_SERVICE` change keeps running without it — and the
> unprivileged `pingwaf` user then fails to bind 80/443 with
> `Permission denied` (`bind() failed on [::]:443`). Two fixes:
>
> 1. Preferred: upgrade **without** `--no-systemd` so the script refreshes the
>    unit file with the capability included. This **rewrites**
>    `/etc/systemd/system/pingwaf.service`, so local edits made directly in that
>    file are lost. Keep customisations in a drop-in
>    (`sudo systemctl edit pingwaf`) instead — the rewrite leaves
>    `pingwaf.service.d/*.conf` alone.
> 2. If you must keep `--no-systemd`, the script detects the outdated unit (it
>    reads both the unit file and its drop-ins) and prints a drop-in; apply it
>    with `sudo systemctl edit pingwaf`:
>
>    ```ini
>    [Service]
>    AmbientCapabilities=CAP_NET_BIND_SERVICE
>    CapabilityBoundingSet=CAP_NET_BIND_SERVICE
>    ```
>
> Either way, verify afterwards:
>
> ```bash
> systemctl show pingwaf -p AmbientCapabilities
> # AmbientCapabilities=CAP_NET_BIND_SERVICE
> ```

Upgrading to the release that adds the origin certificate switch (migration
52) resets `verify_cert` on every origin pool that had it stored as `false` to
"unset". Under the old two-state field, `false` meant "not configured"; it now
means "accept any certificate", so leaving those rows would have pinned them
to the lenient behaviour and ignored the new global switch. The pools fall
back to the global default, which that migration seeds as `false` as well —
behaviour is unchanged on upgrade. Review **Settings → Origin** afterwards and
turn verification on where the origins can support it.

Upgrading to 0.27.12 fills `waf_settings.body_stage_timeout_secs` with 30
(the default) on every site; the migration has no way to tell a backfilled 30
from an operator-chosen 30, and both behave identically. If you want a shorter
body-withholding window, set 1–29 in the site's WAF settings.

Migration 58 adds two columns to site routes: `strip_prefix`
(`NOT NULL DEFAULT FALSE`) and `rewrite_path` (nullable). Both default to
off, so after upgrading every route keeps forwarding the request path to the
origin unchanged — behaviour is identical. If a backend lives under a
stripped prefix (a route on `/api` whose origins serve `/users`), edit that
route afterwards and enable **Strip path prefix** (`strip_prefix: true`, so
`/api/users` is forwarded as `/users`) or set a fixed **Rewrite path**
(`rewrite_path`), both of which were previously impossible.

Migration 59 adds `site_routes.upstream_host` (`TEXT NULL`) and migration 60
adds `sites.response_replace` (`JSONB NULL`). Both columns default to `NULL`
and nothing backfills them, so after upgrading every route keeps forwarding
the client's `Host` header and no site performs response-body replacement —
behaviour is identical until you set the new fields (route
[Upstream Host](#upstream-host-override) override; site
[response replacement](user-guide.md#response-body-replacement)).

**Cache storage default changed: unset `directory` is now a file cache.**
Earlier releases made the cache plugin's `directory` key default to the
in-memory backend. It now defaults to `/tmp/pingwaf-cache` — a file-backed
cache. This affects every configuration that never set the key:

- To keep the in-memory backend exactly as before, set the key explicitly:
  `directory = ""` (or `directory = "memory://…"`).
- If you accept the file cache, prefer a persistent path — inside a
  container `/tmp` is usually a tmpfs mount, where the cache consumes
  **memory** (it counts against the container's memory limit) and vanishes
  with the container. Mount a volume at the cache directory or point
  `directory` at a real path, e.g. `directory = "/var/lib/pingwaf/cache"`.
- Whichever backend you run, the module attribution table now reports the
  cache's disk footprint separately (`disk_bytes` on the `cache` row of
  `GET /api/v1/system/profile`); memory attribution counts only the index
  plus the read buffers of objects being streamed out — see
  [Profiling](profiling.md#memory-vs-disk-in-the-cache-module).

The System observation UI moved: the card on the settings page is gone, and
the full view (overview, module attribution, top methods, control-plane
restart history) now lives on the dedicated `/system` page — admin-only,
reachable from the account dropdown in the top-right corner ("System
observation"). The standalone memory-detail card was removed afterwards: the
overview cards and pills already carry RSS / peak / available / usage, so the
per-field breakdown was a second copy of the same numbers. The underlying
endpoints (`GET /api/v1/system/metrics`, `GET /api/v1/system/profile`) are
unchanged.

**Certificate sharing (migrations 85–86) and the legacy inline certificate
cleanup.** Migration 85 creates `site_certificate_links` and backfills a link
row for every certificate a site has already selected (from
`site_ssl.certificate_id`); migration 86 creates `site_path_exceptions`. Both
are additive — no existing column changes shape and no row is deleted.

The behavioural change needs one decision before you upgrade. Sites that
predate the certificate table still carry their certificate material directly
on the `site_ssl` row (`cert_pem` / `key_pem`). With this release:

- Saving a site's TLS settings **with a selected certificate clears that
  inline copy** — the selected certificate is now unambiguously what the edge
  serves. The old behaviour preferred the inline PEM, which made "switch
  certificate" look like it did nothing.
- Until the settings are saved once, the inline copy keeps serving (fail-open
  on upgrade), so simply upgrading changes nothing.
- ACME write-backs now target the certificate row the site reports
  (`SiteStatus.certificate_id`) instead of whichever pending row belonged to
  the site, so shared certificates are no longer updated on the wrong row;
  agents that report no certificate id keep the old matching behaviour.

> ⚠️ **If you may need to roll back**, back up the `site_ssl` table before
> upgrading (or before the first TLS settings save afterwards):
> `pg_dump -t site_ssl <database> > site_ssl_backup.sql`. Once the inline PEM
> copy is cleared and you downgrade, the old server would again prefer the
> inline copy — which is then empty.

**Sharing certificates between sites** is the point of migration 85: link a
certificate owned by one site to another site (console: the site's SSL
settings certificate picker; API: `POST /sites/{id}/certificate-links`). Share
only to sites you trust — a linked site serves the same hostnames. Unlinking
is refused while the certificate is the site's current TLS selection; clear or
replace the selection first.

**Migrations 87/88: error-page scope registration and heartbeat indexes.**
Migration 88 (`m20240101_000088_heartbeat_indexes`) adds the indexes the
heartbeat retention sweeps and liveness queries need. It also fixes a
packaging miss: migration 87 (`m20240101_000087_error_page_scope`, the
error-page `scope` migration) existed but was never registered in the
migration runner, so it now runs on the next startup of installations that
never received it. Both are additive; migrations run automatically on
startup.

**Migration 90 makes `site_certificates.site_id` nullable** so a certificate
can be uploaded before it is bound to a site — and then serve several (see
[unbound certificates](./api.md#unbound-certificates)). It is additive:
existing rows keep their site and behave exactly as before. The `down`
migration refuses to run while unbound rows exist
(`cannot restore NOT NULL: unbind-free certificates exist; bind or delete
them first`) rather than deleting them — a rollback must not destroy PEM
material you uploaded and have not bound yet. Bind or delete any unbound
certificates before rolling back past this release.

**Origin verification defaults to strict when unset — self-signed origins
return 502.** The agent's generated configuration now states
`origin_verify_cert = true` (verify) when the deployment does not state a
value, instead of leaving it unset (which the proxy read as "do not
verify"). A deployment whose origins present self-signed or private-CA
certificates **and** whose global origin-verification switch was never saved
will see HTTPS-origin requests fail with `502` after upgrading. Fix it at
whichever level fits: the deployment-wide switch
(**Settings → Origin TLS verification**, or
`PUT /api/v1/settings/origin`), the site's `origin_verify_cert` (site SSL
page, origin certificate check card), or the per-pool `verify_cert` — an
explicit value at any level wins over the new default.

**Retries changed.** The proxy's retry ceiling is now an explicit **3**
(Pingora's built-in default was 16), and **non-idempotent requests are never
retried**: only `GET`, `HEAD`, `OPTIONS` and `TRACE` consume a retry, so a
failed connection on a `POST`/`PUT`/`PATCH`/`DELETE` fails immediately
instead of being replayed and possibly duplicating a side effect.

**Cloning copies sensitive configuration.** Site clones now copy
certificates (with private keys), Basic Auth credentials, the OIDC cookie
secret and visitor grants — see [docs/api.md](./api.md) for the exact list
and the console's acknowledgement flow. Anyone who can call the clone API
(or use the console's Clone action) can duplicate that material onto a site
they can create; review write access before upgrading.

### Rolling Upgrade (Distributed)

1. Upgrade the control plane server first
2. Verify agents reconnect successfully
3. Upgrade agents one at a time

## Troubleshooting

### Service won't start

```bash
# Check logs
sudo journalctl -u pingwaf --no-pager -n 50

# Common issues:
# - PostgreSQL not running: systemctl status postgresql
# - Port already in use: ss -tlnp | grep -E '9080|9090'
# - Invalid config: pingwaf all-in-one --db-url "..." (run interactively)
```

### Database connection failed

```bash
# Test connection
psql "postgres://pingwaf:password@localhost:5432/pingwaf" -c "SELECT 1"

# Check pg_hba.conf allows connections
# Ensure the user has CREATEDB privilege for migrations
```

### Agent not connecting

1. Verify the control plane gRPC port (9090) is accessible
2. Check the API key is valid
3. Review agent logs: `RUST_LOG=debug pingwaf agent --server-url ...`
4. Ensure clocks are synchronized (NTP)

`wrong version number` on port **443** is not a control-plane connection
error: it means a client spoke plaintext HTTP to the TLS listener (a scanner
or a misconfigured client). It is expected noise on a public edge; filter or
rate-limit it in the log collector rather than disabling the listener's error
target. Control-plane TLS problems are the `UnknownIssuer` / `NotValidForName`
family covered under [Agent Can't Reach the Control Plane](#agent-cant-reach-the-control-plane).

If `/etc/pingwaf/pingwaf.toml` already exists, `install.sh --server-url …`
and `--api-key …` do **not** rewrite it: in agent mode the file wins over the
unit's environment. The effective URL is the one in the startup log
(`server_url=…`); edit `[agent]` in the file and restart when it differs from
what the installer was given.

TLS handshake errors (`GoAway … FRAME_SIZE_ERROR`, `UnknownIssuer`,
`NotValidForName`) have a dedicated walkthrough under
[Agent Can't Reach the Control Plane](#agent-cant-reach-the-control-plane).

### High memory usage

- Reduce `db_max_connections`
- Lower `log_batch_size`
- Check if cache disk quota is configured
- Call `GET /api/v1/system/metrics`
  ([System observation](./profiling.md#get-apiv1systemmetrics)) to see whether
  it is process RSS or the host that is under pressure

### Error pages & debugging

When the edge cannot serve a request through the upstream (502/503/504, …),
the built-in error page shows a fixed, category-level message only:

- `Upstream Timeout` — upstream read/write/connect timeout
- `Upstream Unavailable` — connection refused / unreachable / TLS handshake failure
- `Upstream Connection Error` — upstream read/write failure
- `Upstream Protocol Error` — HTTP/2 protocol violation
- `Internal Server Error` — anything else

The page also renders the `request_id`, and the response carries the error
class in the `X-Pingap-EType` header (only pingora's built-in error types are
echoed; a custom type reported by a third-party plugin is collapsed to
`internal`, so a plugin-supplied string never reaches the response). It **never** shows the internal error
chain, plugin names or upstream addresses — the original error goes to the
server log only. To debug, take the `request_id` from the page and grep the
server log with it:

```bash
grep "<request_id>" /var/log/pingwaf/server.log
```

The log line holds the original error text, the error type and the site, which
is enough to tell a slow origin from a dead one without exposing internals to
visitors.

### Health check endpoint

```bash
curl http://localhost:9080/healthz
# {"status":"ok","database":"up"}
```
