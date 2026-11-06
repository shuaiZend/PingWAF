# `X-Pingap-EType` and `{{error_type}}` echo internal error names

## Summary

When a proxied request fails, pingap writes the value of
`pingora::ErrorType::as_str()` into the `X-Pingap-EType` response header and
into the `{{error_type}}` template variable. For `ErrorType::Custom(_)` and
`CustomCode(_, _)` that string is arbitrary internal text chosen by the code
that raised the error, so plugin/module names can leak to unauthenticated
clients.

## Affected versions

Tested against `vicanso/pingap` `main` at `cfdf5f76` (crate version `0.16.1`).
Currently the only `Custom` value in tree is `"UpstreamStatus"`
(`pingap-core/src/lib.rs:74`), so today's leak is limited to that identifier;
the mechanism will expose any future custom name.

## Reproduction

1. Raise a `Custom` error from a plugin, e.g.
   `Error::explain(ErrorType::Custom("SecretPlugin"), "boom")`.
2. Make a request that reaches `fail_to_proxy`.

## Actual

```http
X-Pingap-EType: SecretPlugin
```

and the error page renders `{{error_type}}` = `SecretPlugin`.

## Expected

Clients see a stable public classification (`internal`, `upstream_error`,
`timeout`, …) and the detailed type stays in server-side logs.

## Impact

Information disclosure to unauthenticated clients: internal module/plugin
vocabulary and the shape of the failure path. Low severity today, but it turns
any future `Custom("<plugin name>")` into a leak without a code review of this
path.

## Root cause

`pingap-proxy/src/server.rs`, `Server::fail_to_proxy()`
(`upstream/main` around lines 2795, 2915–2919 and 2932):

```rust
let error_type = e.etype().as_str();          // Custom(_) => arbitrary text
...
let content = self.error_template.render(pkg_version, message, error_type);
...
let _ = resp.insert_header("X-Pingap-EType", error_type);
```

## Suggested fix

Map custom types to a fixed public value before they reach the header or the
template:

```rust
fn public_etype(etype: &pingora::ErrorType) -> &'static str {
    match etype {
        pingora::ErrorType::Custom(_) | pingora::ErrorType::CustomCode(..) => "internal",
        other => other.as_str(),
    }
}
```

Use it for both the header and `{{error_type}}`; keep `etype().as_str()` in the
server log line where it is needed for debugging.

## Regression test

- `Custom("InternalPluginName")` and `CustomCode("SecretUpstream", 599)` →
  header value `internal`.
- Built-in types (`ConnectError`, `ReadTimedout`, `HTTPStatus(502)`) are
  preserved verbatim.

## References

- `upstream/main:pingap-proxy/src/server.rs` — `fail_to_proxy()`
- `upstream/main:pingap-proxy/src/error_template.rs` — HTML/JSON escaping
  (the value is escaped, so this is disclosure, not XSS)
- Local workaround: `c124012b` in the PingWAF workspace adds the mapping.
