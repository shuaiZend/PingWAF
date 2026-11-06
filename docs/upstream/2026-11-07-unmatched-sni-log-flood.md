# One `ERROR` log line per unmatched TLS SNI (unauthenticated log flood)

## Summary

Every TLS handshake whose SNI is not in the certificate table writes an
`ERROR` line ("no match certificate") with the attacker-controlled SNI. No
deduplication, sampling or rate limit is applied, and the code runs on the
handshake path, so any unauthenticated client can generate unbounded log
volume with a single loop.

## Affected versions

Tested against `vicanso/pingap` `main` at `cfdf5f76` (crate version `0.16.1`).

## Reproduction

```bash
for i in $(seq 1 1000); do
  openssl s_client -connect host:443 -servername "probe-$i.example.org" </dev/null >/dev/null 2>&1
done
grep -c 'no match certificate' <pingap log>
```

## Actual

1000 handshakes produce 1000 `ERROR` lines, each carrying the random SNI
string (up to 255 bytes). A TLS scanner or a misconfigured client does this
continuously.

## Expected

Unmatched SNI is normal Internet background noise. Log at `DEBUG`, or keep a
single aggregated metric (`unmatched_sni_total`) and rate-limit any message
(e.g. once per SNI per minute, with the SNI truncated). Certificate-table
failures that indicate a real problem (for example an unreadable self-signed
fallback) should stay at `ERROR`.

## Impact

- Disk exhaustion: log volume is attacker-controlled and can fill the logs
  partition. If access/audit logs share that filesystem (the common
  single-host layout), they are lost as well — deployment-dependent, not
  measured here.
- SIEM/syslog pipelines and alerting are flooded, hiding real incidents.
- The SNI string is reflected into log fields, so a crafted value can forge
  additional key/value content in line-oriented collectors.

## Root cause

`pingap-certificate/src/dynamic_certificate.rs`, `GlobalCertificate::select()`
(`upstream/main` around lines 494–499):

```rust
let pair = self.provider.get_pair(sni);
if pair.is_empty() {
    error!(target: LOG_TARGET, sni, "no match certificate");
}
```

It is called from the handshake callback
(`certificate_callback()` around line 703 and `resolve()` around line 794).

## Suggested fix

```rust
if pair.is_empty() {
    // Scanners probe random SNI names; one ERROR per handshake is a
    // log-flood lever. Keep the signal, drop the flood.
    debug!(target: LOG_TARGET, sni, "no match certificate");
}
```

If operators still want visibility, add a counter metric and a per-SNI
rate-limited `warn!`. Do **not** demote the `get self signed cert fail` branch
below it — that one indicates a broken fallback and must stay loud.

## Regression test

Open N handshakes with unknown SNI and assert the emitted log count is bounded
(or that the level is `DEBUG`); add a companion test that an unmatched SNI
still completes the handshake with the default certificate.

## References

- `upstream/main:pingap-certificate/src/dynamic_certificate.rs` — `select()`,
  `certificate_callback()`, `resolve()`
- Local workaround: `0a50036f` in the PingWAF workspace demotes the line to
  `debug!`.
