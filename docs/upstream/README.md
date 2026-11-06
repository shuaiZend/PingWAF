# Upstream bug reports

Findings against **`vicanso/pingap`** that were confirmed while maintaining the
vendored `pingap-*` crates in this workspace. Each report is self-contained and
written for submission as a GitHub issue (English, code references pinned to
`upstream/main` at the time of writing).

| Report | Severity | Upstream status |
|---|---|---|
| [Transparent pool uses the raw Host as SNI](2026-11-07-transparent-pool-sni.md) | P2 (interop / connection-pool waste) | not yet filed |
| [Unmatched SNI logs one ERROR per handshake](2026-11-07-unmatched-sni-log-flood.md) | P2 (log flood) | not yet filed |
| [`X-Pingap-EType` leaks internal error names](2026-11-07-x-pingap-etype-leak.md) | P3 (information disclosure) | not yet filed |

## Using these reports

- Reproduce against a current `upstream/main` first; the line numbers are a
  snapshot and may move.
- Submit one issue per report. Suggested labels: `bug`; add `security` only
  where the report says the impact is security-relevant.
- The reports describe defects, not exploits; they intentionally omit any
  site-specific hostnames, addresses or customer data.
- If upstream has already fixed a finding, close the corresponding file here
  instead of submitting a duplicate.

## How these were found

The workspace's `pingap-*` crates are a long-lived manual port rather than a
git fork (no common ancestor with upstream). During the 2026-11 sync review we
compared local fixes with `upstream/main`; only findings that still reproduce
there are kept. The comparison method and the port backlog live in
[`docs/upstream-sync.md`](../upstream-sync.md).
