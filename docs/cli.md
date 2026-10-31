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

## Global options

`server` and `all-in-one` accept these options (the `agent` has its own set,
listed after the table). Every option also reads a `PINGWAF_*` environment
variable.

| Option | Environment variable | Default | Purpose |
| --- | --- | --- | --- |
| `--db-url` | `PINGWAF_DB_URL` | `postgres://pingwaf:pingwaf@localhost:5432/pingwaf` | PostgreSQL connection string |
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
(`PINGWAF_JWT_SECRET`, default `change-me-in-production`, must be ≥ 16
characters), `--admin-email` (`PINGWAF_ADMIN_EMAIL`, default
`admin@pingwaf.local`), `--admin-password` (`PINGWAF_ADMIN_PASSWORD`, default
`pingwaf123`) and `--serve-frontend` (`PINGWAF_SERVE_FRONTEND`, default
`true`).

Agent-only options (`agent` / the agent half of `all-in-one`): `--server-url`
(`PINGWAF_SERVER_URL`, default `http://localhost:9090`), `--server-ca-cert`
(`PINGWAF_SERVER_CA_CERT`), `--api-key` (`PINGWAF_API_KEY`), `--cache-dir`
(`PINGWAF_CACHE_DIR`, default `./data/cache`), `--fail-open`
(`PINGWAF_FAIL_OPEN`, default `true`), `--heartbeat-interval-secs`
(`PINGWAF_HEARTBEAT_INTERVAL`, default `30`), `--metrics-ship-interval`
(`PINGWAF_METRICS_SHIP_INTERVAL`, default `30`), plus `--log-batch-size`,
`--log-flush-interval-secs` and `--max-body-log-size` (command line only).

### Configuration precedence

Settings are read from the TOML file first, then from the `PINGWAF_*`
environment variables, then from the command line — a later channel overrides
an earlier one. An argument repeated on the command line wins over the value
injected from the file, so
`pingwaf all-in-one --config /etc/pingwaf/pingwaf.toml --tls-enabled false`
always disables TLS regardless of what the file says.

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
