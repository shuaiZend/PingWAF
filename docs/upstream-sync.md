# Upstream sync analysis (2026-11-07)

How far PingWAF's vendored `pingap-*` crates are from `vicanso/pingap`, what
the upstream has fixed since the closest release, and what to port next. The
short version: **the two trees share no git ancestry**, so an upstream merge is
impossible — every sync is a manual port. This page records the baseline and
the backlog so the next sync starts from evidence instead of guesswork.

## Versions

| Component | PingWAF | Upstream | Note |
|---|---|---|---|
| Pingora | `0.9.1` (this branch) | `0.9.1` (2026-11-03) | patch bump; see the changelog entry |
| pingap crates | `0.32.17` (workspace version) | `0.16.1` / `main@cfdf5f76` | upstream keeps its own version line |

`git merge-base HEAD upstream/main` returns no commit: the histories are
unrelated (the workspace was re-imported, not forked). `git merge upstream/main`
would replay ~2,000 commits of unrelated work and is **never** the right move.

## Baseline

The local tree is not a snapshot of any single upstream tag. The closest
content match is **`v0.15.0` (`8270a1eb`)**, measured over `pingap-*`:

| Upstream ref | files changed | insertions | deletions | modified-file lines |
|---|---|---|---|---|
| **`v0.15.0`** | 120 | +35,637 | −4,973 | **14,735** (smallest) |
| `v0.14.3` | 99 | +37,970 | −1,631 | 15,020 |
| `v0.16.0` | 200 | +36,327 | −57,744 | 63,354 |
| `v0.16.1` | 208 | +36,431 | −62,246 | 65,211 |
| `main` | 223 | +36,642 | −67,136 | 69,666 |

The last column counts insertions+deletions on files that exist in **both**
trees (`--diff-filter=M`): added/deleted files are excluded so a module
rewrite on one side does not dominate the score. `v0.16.x` rewrote the plugin
base, error handling and logging (hence the −57k deletions), so `v0.15.0` is
the last release where a file-by-file comparison is meaningful; `v0.14.3` is
a close second. The local tree also carries code from later upstream work
(`error_page.rs`) and lacks some `v0.15.0` files (`ja4.rs`, `first_round.rs`),
which is why the baseline is "closest", not "ancestor".

Refresh it with:

```bash
git fetch upstream '+refs/tags/v0.1*:refs/upstream-tags/v0.1*'
git diff --shortstat refs/upstream-tags/v0.15.0 HEAD -- 'pingap-*'
git diff --diff-filter=M --numstat refs/upstream-tags/v0.15.0 HEAD -- 'pingap-*' \
  | awk '{a+=$1; d+=$2} END {print a+d " modified-file lines"}'
git log --oneline refs/upstream-tags/v0.15.0..upstream/main -- 'pingap-*'
```

(`refs/upstream-tags/*` avoids the local release tags, which reuse the same
`v0.x.y` names.)

## Port backlog

`v0.15.0..upstream/main` contains 69 commits touching `pingap-*`
(24 fix / 18 feat / 15 test / 4 refactor / 3 perf / 2 docs / 2 chore / 1 ci).
The rows below are the ones that change behaviour we actually run. "Local"
says whether the protection already exists here.

### P0 — security / request routing

| Upstream | What it fixes | Local | Action |
|---|---|---|---|
| `fd6fe1a3` | Match locations against a **normalised path** (`/%61dmin`, `//admin`, `/public/../admin` no longer bypass `/admin` plugins) | **missing** (no `normalize_path`) | port first; add the upstream unit matrix |
| `cc2e68a0` | 16 audit findings: admin auth bypass, cache keyed without host, directory traversal, ACME token leak, spoofable client IP behind trusted proxies, unhealthy backends still 503 | mostly present (cache key includes host: `pingap-proxy/src/server.rs:2261`), rest needs per-item review | review item by item; port the gaps |
| `44e75ed5`, `22d4196a` | Client IP spoofing via non-ASCII XFF; cache key loses host after h2 rewrite; ipv4-mapped prefixes matched as IPv4 | implementation differs (own `ProxyTrust`) | audit `ProxyTrust` against these cases |
| `2d895b21` | Merge split `Cookie` headers; reject targets that are neither path nor URL | missing | port |

### P1 — stability / correctness

| Upstream | What it fixes | Local | Action |
|---|---|---|---|
| `0f7a981b` | Pingora 0.9's brotli stream never finishes (`br` bodies rejected by strict decoders) | missing (uses pingora `ResponseCompression` directly) | port `BrotliTail` with the 0.9.1 upgrade |
| `b29acc17` | ACME: timeout every CA/DNS call (one hung request stops the renewal service); accept `ready` orders; zone from registrable domain; self-signed size cap | timeouts missing | port the timeout wrapper at least |
| `e034c140` | Two inflight limits on one location must both count; per-instance body handlers; JWT `nbf`; redirect port | missing (`ctx.state.guard` is single) | port when touching `limit.rs` / `jwt.rs` |

### P2 — hardening and features

`e3aeab27`, `56ee9ed9`, `8aa7f853`, `4b402362`, `dc2a540e` add option validation
and reload hardening; `8082ab84` rejects duplicate plugin categories. Feature
work (`maintenance`, `uri_block`, `mirror`, `request_headers`,
`bandwidth_limit`, JA4, `hmac_auth`, unix socket, PROXY protocol to upstream,
`strict_sni` 421) is **not ported** and is tracked in `UPSTREAM.md`; porting is
a product decision, not a sync obligation.

## Upstream bugs found in this pass

Three defects that exist in `upstream/main` and that this project already
works around are written up for upstream submission under
[`docs/upstream/`](upstream/README.md):

1. transparent-pool `sni = "$host"` uses the raw Host as SNI (IP literals and
   mixed case included);
2. a `ERROR` log line per unmatched-SNI handshake (unauthenticated log flood);
3. `X-Pingap-EType` / `{{error_type}}` echo internal `Custom` error names.

## Recommended order

1. `fd6fe1a3` path normalisation (P0, small, testable).
2. `BrotliTail` from `0f7a981b` (user-visible `br` correctness).
3. `b29acc17` ACME timeouts (availability).
4. `2d895b21` cookie merge + routable-target check.
5. Re-run this analysis after every upstream release; keep the backlog table
   current instead of tracking it in chat.

Each port should land with the upstream test where one exists, plus a local
regression test, and must not change existing `pingap-*` public signatures
without a matching workspace update.
