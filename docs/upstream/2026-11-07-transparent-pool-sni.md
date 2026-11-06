# Transparent pool passes the raw `Host` header as TLS SNI

## Summary

When an upstream pool is `transparent` with TLS enabled and `sni = "$host"`,
the SNI sent to the origin is the client's raw `Host` header: it is not
lowercased, not validated, and an IP literal is passed through. The static
pool path applies three checks (`sni_for()`) to the same configuration value,
so one setting has two different meanings.

## Affected versions

Tested against `vicanso/pingap` `main` at `cfdf5f76` (crate version `0.16.1`).
The static path has had the guard for several releases, so the split is old.

## Reproduction

1. Start a TLS origin whose certificate/SNI routing only knows
   `backend.example.com`.
2. Configure `upstream { discovery = "transparent", tls = true, sni = "$host" }`.
3. Send requests with different `Host` values:

```bash
curl -k --resolve x:443:127.0.0.1 -H 'Host: BACKEND.example.com' https://x/
curl -k --resolve x:443:127.0.0.1 -H 'Host: 10.0.0.5'            https://x/
```

## Actual

Capture the ClientHello: the first request carries
`SNI = BACKEND.example.com` (mixed case preserved); the second carries
`SNI = 10.0.0.5`.

## Expected

- A host name is lowercased before it becomes SNI.
- An IP literal (or a bracketed IPv6 literal) is not a legal
  `HostName` in SNI; the request should be rejected or sent without SNI,
  exactly as the static path does.

## Impact

- RFC 6066 §3 forbids IP literals in `HostName`. Strict TLS stacks (and
  nginx-style SNI certificate selection) are **expected** to abort such a
  handshake with `unrecognized_name`, which surfaces as a 502; that endpoint
  behaviour is inferred from the RFC and the origin's SNI handling, not
  reproduced in this report.
- Case differences are **expected** to split pingora's connection pool
  (`Example.com` and `example.com` are different peer keys), doubling
  handshakes and socket memory for the same backend. The pooling key is
  code-level reasoning, not a measurement here.
- Operators cannot reason about `sni = "$host"` because the same value behaves
  differently per discovery mode (this part is directly observable in code).

## Root cause

`pingap-upstream/src/upstream.rs`, `Upstream::new_transparent_peer()`
(`upstream/main` around lines 1786–1796):

```rust
let addr = resolve_host(host, port, self.ipv4_only).await?;
let sni = if self.sni == "$host" {
    host_name(host).to_string()          // raw Host: no checks, no lowercase
} else {
    self.sni.clone()
};
```

The static path in the same file (`sni_for()`, around lines 1743–1760) already
does it correctly:

```rust
let host = pingap_core::get_host(session.req_header())?;
if host.is_empty()
    || host.starts_with('[')
    || host.parse::<std::net::IpAddr>().is_ok()
{ return None; }
Some(host.to_ascii_lowercase())
```

## Suggested fix

Reuse `sni_for()` from the transparent path (it has the `Session` available), or
inline the same three checks plus `to_ascii_lowercase()`. Treat an unusable
host the same way the static path does (reject the request) so both discovery
modes agree.

## Regression tests

A table test over transparent + `sni = "$host"`:

| `Host` | expected SNI |
|---|---|
| `Backend.Example.COM:443` | `backend.example.com` |
| `10.0.0.5` | none / request rejected |
| `[2001:db8::1]` | none / request rejected |
| empty | none / request rejected |

Also assert that two different casings produce the **same** peer group key.

## References

- `upstream/main:pingap-upstream/src/upstream.rs` — `sni_for()` and
  `new_transparent_peer()`
- RFC 6066 §3 (`HostName` must be a DNS name, not an IP literal)
- Local workaround: `cd5899ed` in the PingWAF workspace normalises transparent
  SNI through the same helper.
