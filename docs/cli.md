# PingWAF CLI — Administrator Handbook

The `pingwaf` binary is more than the service process: it carries a set of
maintenance subcommands that talk **straight to PostgreSQL** instead of the
REST API. This handbook covers the run modes, the shared options and every
maintenance command, with runbooks for the situations where the web console
cannot help.

## When to use the CLI

The web console is an HTTP client of the control plane on `:9080`. It stops
being an option when the control plane itself is the problem. The CLI keeps
working in exactly those situations:

- **The control plane is down** (crash loop, failed migration, wrong
  configuration) and you need to inspect or repair state.
- **The API IP allowlist locked you out** — every source address you can reach
  `:9080` from is refused.
- **You forgot the administrator password** and cannot sign in (or the
  registered e-mail is wrong).
- **You need to roll back a configuration change** and the console rollback
  cannot be reached to push it.

Run `pingwaf <command> --help` for the exact options of any subcommand. Plain
`pingwaf --help` answers with the `pingap`-compatible options of the shared
binary, not with this subcommand tree.

## Run modes

Three subcommands start a long-running process; they are what the systemd
unit's `ExecStart` invokes:

| Mode | Command | Role |
| --- | --- | --- |
| Control plane | `pingwaf server` | REST API + gRPC control plane + embedded dashboard. Does not proxy traffic. |
| Data plane | `pingwaf agent` | Connects to a remote control plane, enforces its rules, proxies traffic on :80/:443. |
| All-in-one | `pingwaf all-in-one` | Both of the above in one process (the agent talks to the local server over loopback). |

The mode can also be selected with the `PINGWAF_MODE` environment variable
instead of a subcommand, and `pingwaf --version` (long banner) / `pingwaf -V`
(bare number) print the version and exit. A systemd install runs
`ExecStart=/usr/local/bin/pingwaf <mode>` (the mode `install.sh` was asked
for) with `PINGWAF_CONFIG=/etc/pingwaf/pingwaf.toml` set in the unit;
`systemctl restart pingwaf` applies any configuration change, and
`journalctl -u pingwaf -e` shows the service logs.

### The agent-only build (`pingwaf-agent`)

Releases also ship `pingwaf-agent`, built from the same source with the
control plane compiled out (`--bin pingwaf-agent --features edge`). It is the
data plane and nothing else: no PostgreSQL, no REST API, no embedded
dashboard, so an edge node downloads and maps far less code. `install.sh
--mode agent` fetches that archive and installs it under its own name —
`/usr/local/bin/pingwaf-agent`, unit `pingwaf-agent.service`, still reading
`/etc/pingwaf/pingwaf.toml` — so an edge node does not carry a binary called
`pingwaf` that looks like it could serve the console. `--version` marks the
build with `edge`. Both spellings work:

```bash
pingwaf-agent agent --server-url https://control-plane:9090 --api-key KEY
pingwaf-agent --server-url https://control-plane:9090 --api-key KEY
```

The `agent` subcommand is accepted because a control-plane-driven upgrade
(`agent.auto_upgrade`) replaces the file with the control plane's all-in-one
build, which still needs it. That upgrade follows the control plane's
executable, so it trades the smaller build for the full one; the release-feed
upgrade below installs the agent-only artifact instead. The agent-only build
has no `server`/`all-in-one`/maintenance subcommands; run those from the full
`pingwaf` binary.

### Version and upgrade

Both binaries answer `version` and `upgrade` (the agent used to have no way to
ask for either):

```bash
pingwaf-agent version            # version, build commit, TLS backend, target
pingwaf-agent version --check    # …and the newest published release
pingwaf-agent upgrade --check    # is there a newer release?
pingwaf-agent upgrade            # install it and restart pingwaf-agent.service
pingwaf upgrade --version 0.33.4 --no-restart   # the full binary, pinned version
```

`upgrade` downloads `pingwaf-agent-linux-{amd64,arm64}.tar.gz` (or
`pingwaf-linux-…` for the full binary) from the GitHub release, verifies the
`.sha256` sidecar **before** extracting, asks the extracted binary for its
version before replacing anything, keeps the current file as `<binary>.prev`,
and stages the replacement next to the target so an interrupted upgrade leaves
the old binary in place. It needs `curl` (the installer already requires it) and
write access to the target directory — run it with `sudo` on a normal install.
`--repo owner/name` points it at a fork or mirror; `--binary PATH` replaces a
file other than the running one; `--no-restart` skips the systemd restart.

This is the release-feed path: it installs the **agent-only** build for
`pingwaf-agent`. The opt-in `agent.auto_upgrade` remains the fleet path — it
follows whatever build the control plane is serving, which is the full binary.

An edge installed before the binary was renamed still has `/usr/local/bin/pingwaf`
and `pingwaf.service`. Re-run `install.sh --mode agent` once: it installs
`pingwaf-agent`, writes `pingwaf-agent.service`, and retires the old agent unit
(stopped, disabled and removed) so the two cannot fight over the ports and the
config file.

**In agent mode the config file is authoritative** (`[agent]` in
`/etc/pingwaf/pingwaf.toml` wins over `PINGWAF_*`), so the systemd unit carries
no second copy of the connection settings — a duplicate was inert at best and,
when it disagreed with the file, `systemctl cat` reported a URL the agent never
dialed. Re-running the installer updates only the keys you pass explicitly
(`--server-url`, `--api-key`, `--server-ca-cert`, `--server-tls-name`): each is
written into `[agent]` (a `pingwaf.toml.bak-<timestamp>` copy is kept first) and
the plan summary shows the effective URL, marking it "from the existing config
file" when the file wins.

> An agent-only node is currently **unbound**: it receives every site owned by
> the API key's owner, not one site. The control plane must be **0.32.40 or
> newer** when it is the ACME issuer, so issued PEMs are synced and pushed;
> otherwise each agent starts its own HTTP-01 order and fails behind the
> control plane. See [deployment.md → Edge Agent](deployment.md#edge-agent)
> for the CA, `server_tls_name`, and `PINGAP_DISABLE_ACME` details.

## Global options

`server` and `all-in-one` accept these options (the `agent` has its own set,
listed after the table). Every option also reads a `PINGWAF_*` environment
variable.

| Option | Environment variable | Default | Purpose |
| --- | --- | --- | --- |
| `--db-url` | `PINGWAF_DB_URL` | `postgres://pingwaf:pingwaf@localhost:5432/pingwaf` | PostgreSQL connection string |
| `--db-url-file` | `PINGWAF_DB_URL_FILE` | — | Read the PostgreSQL connection string from a file instead (trimmed; empty/unreadable aborts with exit code 1; **wins over `--db-url`**). Accepted by `server`, `all-in-one` and every maintenance command |
| `--admin-addr` | `PINGWAF_ADMIN_ADDR` | `0.0.0.0:9080` | HTTP admin/API listen address |
| `--grpc-addr` | `PINGWAF_GRPC_ADDR` | `0.0.0.0:9090` | gRPC listen (server/all-in-one) or connect (agent) address |
| `--grpc-tls-cert` | `PINGWAF_GRPC_TLS_CERT` | — | PEM certificate chain for the gRPC control plane; agents then connect via `https://` |
| `--grpc-tls-key` | `PINGWAF_GRPC_TLS_KEY` | — | PEM private key for `--grpc-tls-cert` |
| `--grpc-tls-mode` | `PINGWAF_GRPC_TLS_MODE` | `tls` | `tls` serves the configured certificate or, absent one, the self-signed control-plane certificate; `off` serves plaintext (degraded) |
| `--tls-enabled` | `PINGWAF_TLS_ENABLED` | `true` | Serve the dashboard and REST API over TLS; a self-signed certificate is generated on first boot. Set `false` behind a TLS-terminating proxy |
| `--tls-sans` | `PINGWAF_TLS_SANS` | *(empty)* | Comma-separated SANs (DNS names and IP addresses) of the generated self-signed certificate |
| `--public-host` | `PINGWAF_PUBLIC_HOST` | *(unset)* | Host (optionally with port) the dashboard is reached under; pinned into the HTTP→HTTPS redirect target instead of the client's `Host` header |
| `--config` | `PINGWAF_CONFIG` | — | TOML configuration file; its `[server]` and `[agent]` tables hold the same settings |

Server-only options (`server` / `all-in-one`): `--jwt-secret`
(`PINGWAF_JWT_SECRET`, default `change-me-in-production`, **use 32 random
bytes**: `openssl rand -hex 32`; the server only enforces a 16-character
minimum, and a placeholder value is replaced by a random secret persisted on
first boot), `--admin-email` (`PINGWAF_ADMIN_EMAIL`, default
`admin@pingwaf.local`), `--admin-password` (`PINGWAF_ADMIN_PASSWORD`, default
`pingwaf123` — the compose file and the installers require an explicit password
or generate a random one) and `--serve-frontend` (`PINGWAF_SERVE_FRONTEND`, default
`true`).

`--admin-email` / `--admin-password` only **seed** the first administrator when
the `users` table is empty, and that account is created with a forced password
change. Changing the flags later does not rotate an existing account — use
`pingwaf user reset-password` (which also revokes every session).

`PINGWAF_SECRET_KEY` has no flag and is read from the environment only. It
seals notification-channel secrets, the AI provider API key and backup-target
credentials at rest; set it to 32 random bytes (`openssl rand -hex 32`), because
without it those fields are stored in clear text (only a warning is logged).
Back the key up separately — there is no online rotation. See
[`SECURITY.md`](../SECURITY.md#credentials-and-secrets).

Agent-only options (`agent` / the agent half of `all-in-one`): `--server-url`
(`PINGWAF_SERVER_URL`, default `http://localhost:9090`), `--server-ca-cert`
(`PINGWAF_SERVER_CA_CERT`), `--api-key` (`PINGWAF_API_KEY`), `--cache-dir`
(`PINGWAF_CACHE_DIR`, default `./data/cache`), `--fail-open`
(`PINGWAF_FAIL_OPEN`, default `true`), `--heartbeat-interval-secs`
(`PINGWAF_HEARTBEAT_INTERVAL`, default `30`), `--metrics-ship-interval`
(`PINGWAF_METRICS_SHIP_INTERVAL`, default `30`), plus `--log-batch-size`,
`--log-flush-interval-secs` and `--max-body-log-size` (command line only).

The connector can reach both the control plane and the edge ingresses through a
SOCKS5 proxy, for a host with no direct egress: `--socks5-proxy <host:port>`
(`PINGWAF_TUNNEL_SOCKS5_PROXY`), plus `--socks5-username` /
`--socks5-password` (prefer `PINGWAF_TUNNEL_SOCKS5_PASSWORD` over the flag, which
`ps` can read). The same keys live in `/etc/pingwaf/tunnel.toml` as
`socks5_proxy`, `socks5_username` and `socks5_password`. Target names are
resolved by the proxy (SOCKS5 ATYP 3), and the handshake has its own timeout.

The tunnel listener is an agent/all-in-one option as well: `--tunnel-addr`
(`PINGWAF_TUNNEL_ADDR`, default empty = off) binds the address a
`pingwaf-tunnel` connector dials. Only an edge that terminates tunnels sets it;
see [user-guide.md](user-guide.md#tunnels-internal-services-without-an-inbound-port).

The standalone connector is its own binary, `pingwaf-tunnel`:

```
pingwaf-tunnel pair   --token <pairing-token> --server-url https://cp:9090 [--config PATH]
pingwaf-tunnel run    [--config PATH]
pingwaf-tunnel status [--config PATH]
```

`pair` redeems the one-time token (from **Agents → Tunnels**) for a long-lived
credential and writes `/etc/pingwaf/tunnel.toml` (`0600`); `run` serves until
SIGTERM (or until the tunnel is revoked); `status` prints the stored
configuration plus the live state `run` publishes to
`/var/lib/pingwaf/tunnel/status.json` — the connected ingress endpoints, the
targets in force, the SOCKS5 proxy when one is configured, and the last
revocation reason. `status` never opens a control-plane stream: doing so would
take the session slot of the connector that is actually serving traffic.

`pair` and `run` take the same global flags; anything not on the command line is
read from the config file:

| Flag | Env | Meaning |
| --- | --- | --- |
| `--config PATH` | — | `tunnel.toml` (default `/etc/pingwaf/tunnel.toml`) |
| `--token`, `--token-file` | `PINGWAF_TUNNEL_TOKEN`, `PINGWAF_TUNNEL_TOKEN_FILE` | the one-time pairing token |
| `--server-url` | `PINGWAF_TUNNEL_SERVER_URL` | control-plane gRPC URL (`https://…:9090`) |
| `--server-ca-cert` | `PINGWAF_TUNNEL_SERVER_CA_CERT` | CA bundle for a self-signed control plane |
| `--server-tls-name` | `PINGWAF_TUNNEL_SERVER_TLS_NAME` | override the TLS server name |
| `--ingress` | `PINGWAF_TUNNEL_INGRESS` | pin edge endpoints (`host:port[=sha256],…`) |
| `--allowed-targets` | `PINGWAF_TUNNEL_ALLOWED_TARGETS` | local allowlist, intersected with the policy |
| `--socks5-proxy`, `--socks5-username`, `--socks5-password` | `PINGWAF_TUNNEL_SOCKS5_*` | egress through a SOCKS5 proxy |

The config file carries the same keys in snake_case (`server_url`,
`server_ca_cert`, `server_tls_name`, `ingress`, `allowed_targets`,
`socks5_proxy`, `socks5_username`, `socks5_password`, plus the `tunnel_id`,
`credential` and `session_token` that `pair` writes). A malformed value is an
error, never a silent fallback to a direct dial.

### Passing secrets safely

`--db-url`, `--api-key`, `--jwt-secret` and `--admin-password` are visible to
every local user in `ps` and are recorded in shell history. For the two
highest-value ones the CLI can read the value from a file instead:

| Flag | Environment variable | Reads |
| --- | --- | --- |
| `--db-url-file` | `PINGWAF_DB_URL_FILE` | PostgreSQL connection string (`server`, `all-in-one`, and every maintenance command) |
| `--api-key-file` | `PINGWAF_API_KEY_FILE` | Agent enrollment API key (`agent`, `all-in-one`) |

The file content is trimmed (surrounding whitespace and the trailing newline)
and used as the whole value; an unreadable or **empty** file aborts with exit
code 1 instead of silently configuring an empty secret. When both an inline
value and the file are given, **the file wins**. Passing `--db-url` or
`--api-key` inline without the matching file flag prints a warning that the
value is visible in the process list.

For scripts and CI, write the secret to a `0600` file (or use the environment
variable / systemd `EnvironmentFile=` — the host installer writes
`PINGWAF_SECRET_KEY` to `/etc/pingwaf/pingwaf.env` for exactly this reason) and
pass `--db-url-file` / `--api-key-file`. `--jwt-secret` and `--admin-password`
have no file variant yet: use `PINGWAF_JWT_SECRET` / `PINGWAF_ADMIN_PASSWORD`,
and run the installer with `--dry-run` first so the resolved plan — not the
secret — is what gets logged.

### Configuration precedence

Settings are read from the TOML file first, then from the `PINGWAF_*`
environment variables, then from the command line — a later channel overrides
an earlier one. An argument repeated on the command line wins over the value
injected from the file, so
`pingwaf all-in-one --config /etc/pingwaf/pingwaf.toml --tls-enabled false`
always disables TLS regardless of what the file says.

## Host installer (`install.sh`)

The release installer (not the binary itself) writes the unit, the config and
the instance secrets. Its security-relevant behaviour:

- **Release checksum** — the release asset's `.sha256` sidecar is downloaded and
  verified; a missing, malformed or mismatching checksum aborts the install.
- **Administrator password** — when `--admin-password` / `PINGWAF_ADMIN_PASSWORD`
  is empty, a random password is generated and printed **once** at the end.
  There is no built-in default password.
- **`PINGWAF_SECRET_KEY`** — generated when unset and written to
  `/etc/pingwaf/pingwaf.env` (mode `0600`, owned by `pingwaf`); the shipped unit
  reads it via `EnvironmentFile=-/etc/pingwaf/pingwaf.env`. An existing key is
  kept on upgrade — rotating it silently would make sealed channel/AI/backup
  credentials unreadable.
- **Listen addresses** — the dashboard (`9080`) binds **`0.0.0.0`** so a fresh
  install is reachable; restrict it with a firewall or a TLS reverse proxy and
  change the administrator password at the first sign-in. gRPC (`9090`) binds
  **`127.0.0.1`**; `--listen-public` (`PINGWAF_LISTEN_PUBLIC=1`) opens it on
  `0.0.0.0`, which only remote agents need.

See [deployment.md → Manual Installation](./deployment.md#manual-installation-binary--systemd)
for the full option table.

## Maintenance commands

All maintenance subcommands take `--db-url` (env `PINGWAF_DB_URL`, default
`postgres://pingwaf:pingwaf@localhost:5432/pingwaf`), connect to PostgreSQL
directly, do their work and exit. A **running** control plane notices their
writes within a few seconds, but — unlike the console — the CLI does not push
anything to the agents; see the `config rollback` caveat below.

### `pingwaf user` — dashboard accounts

| Command | Purpose |
| --- | --- |
| `pingwaf user list` | List dashboard accounts. |
| `pingwaf user add-admin --email <addr> [--name <name>] [--password <pw>]` | Create an administrator account. When `--password` is omitted a random one is generated and **printed once**. |
| `pingwaf user reset-password --email <addr> [--password <pw>]` | Set a new password. An omitted password is generated and printed once. The reset bumps the account's token version, so every existing session of that account is revoked — a leaked session does not survive the reset. |

### `pingwaf mode` — observation mode

| Command | Purpose |
| --- | --- |
| `pingwaf mode status` | Print the current observation mode. |
| `pingwaf mode observe` | Turn observation mode on: every detection runs, nothing is blocked. |
| `pingwaf mode enforce` | Turn observation mode off: protections enforce again. |

Use `observe` while tuning rules on live traffic; switch back with `enforce`
once the false positives are handled. `mode status` tells you which of the two
is active without trusting anyone's memory.

### `pingwaf security` — control-plane self-protection

| Command | Purpose |
| --- | --- |
| `pingwaf security status` | Print the `:9080` protection settings and the effective allowlist. |
| `pingwaf security allowlist on\|off [--force]` | Turn the API IP allowlist on or off — the break-glass switch when the console is unreachable. |

```bash
pingwaf security allowlist on           # enable; refused while the list is empty
pingwaf security allowlist off          # disable
pingwaf security allowlist on --force   # enable even when empty — see the warning
```

> ⚠️ **`allowlist on --force` is a footgun.** `--force` enables the allowlist
> even when it is empty, and an empty allowlist **refuses every API caller** —
> the console, the dashboard and any scripting against `:9080` all get locked
> out, and you have to come back to this CLI (or the database) to undo it.
> Turn the allowlist on without `--force` first, and add your own addresses
> before relying on it.

### `pingwaf config` — configuration version history

```bash
pingwaf config history [--site <uuid|global>] [--limit <n>]   # newest first, default 20
pingwaf config show <version> [--json]                        # metadata + snapshot summary
pingwaf config rollback <version> [--yes]                     # prompts unless --yes
```

- `history` lists recorded versions newest first. `--site` filters to one
  site (its UUID) or to the deployment-wide settings (`global`); `--limit`
  caps how many are printed (default 20).
- `show` prints one version's metadata plus a snapshot summary, or the whole
  snapshot as JSON with `--json`.
- `rollback` restores a version and records the restore as a new version
  attributed to `cli`, so rolling back a rollback works and the history stays
  linear. It prompts for confirmation unless `--yes` is given.

> ⚠️ **A CLI rollback does not notify a running control plane.** It writes the
> rows directly to PostgreSQL; the agents pick the restored configuration up
> on their next full sync or restart. When the change has to reach the fleet
> immediately, use the console rollback (`POST /config-versions/{id}/rollback`,
> see [docs/api.md](./api.md)) — it pushes to the agents right away.

## Typical incident runbooks

### Forgot the administrator password

```bash
pingwaf user reset-password --email admin@pingwaf.local
# → prints a generated password once
pingwaf user reset-password --email admin@pingwaf.local --password 'S3cret-New-Pass'
```

All previous sessions of the account are revoked by the reset; sign in with
the new password and change it in the console afterwards if it was printed.

### Locked out by the API allowlist

```bash
pingwaf security status          # confirm the allowlist is what blocks you
pingwaf security allowlist off   # disable it, sign in, fix the list, re-enable
```

If the allowlist was enabled with `--force` while empty, `allowlist off` is
the only way back in short of editing the database directly.

### Temporarily stop blocking (observation mode)

```bash
pingwaf mode status
pingwaf mode observe    # detect everything, block nothing
pingwaf mode enforce    # restore enforcement
```

### Roll back a bad configuration change

```bash
pingwaf config history --limit 10          # find the version to restore
pingwaf config show 42                     # inspect it before restoring
pingwaf config rollback 42                 # prompts; --yes skips the prompt
sudo systemctl restart pingwaf             # make the running process pick it up now
```

Remember that the CLI rollback reaches agents only on their next full sync or
restart; restarting the service (or using the console rollback when reachable)
propagates it immediately.

### Service will not start or logs look wrong

```bash
journalctl -u pingwaf -e        # last log lines of the unit
systemctl status pingwaf
```

### Binding 80/443 fails with "Permission denied"

The unit runs as the unprivileged `pingwaf` user; binding privileged ports
works only while the unit carries `CAP_NET_BIND_SERVICE`. Check it:

```bash
systemctl show pingwaf -p AmbientCapabilities
# AmbientCapabilities=CAP_NET_BIND_SERVICE
```

If the line is empty (a unit installed by an older `install.sh` run), follow
the `systemctl edit pingwaf` drop-in snippet the installer prints on upgrade —
see the [upgrade section of the deployment guide](./deployment.md#upgrade-procedure).

## Differences from the web console

| Aspect | CLI | Web console |
| --- | --- | --- |
| How it changes state | Writes directly to PostgreSQL | REST API on `:9080`, subject to the allowlist |
| Control plane down | Keeps working | Unavailable |
| Agents | Does not push anything; agents sync later | Pushes immediately (e.g. console rollback) |
| Authentication | Database credentials (`--db-url`) | Dashboard session / JWT |
| Scope | Single-instance maintenance | Full fleet management |

The two are complementary: manage the fleet through the console, and keep this
CLI for the moments the console is the thing that broke.
