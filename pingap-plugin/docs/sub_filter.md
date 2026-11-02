# sub_filter

Search-and-replace inside the response body, in the spirit of nginx's
`sub_filter` and the `subs_filter` module. Useful for rewriting absolute URLs,
injecting a script tag, or patching an upstream you cannot change.

- **Step:** `response` and `response_body`
- **Registered as:** `sub_filter`

## Configuration

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `category` | string | — | Must be `sub_filter`. |
| `filters` | string[] | `[]` | Substitution rules; see the syntax below. |
| `path` | string | — | Regex on the request path. Unset means every path. |
| `status_codes` | string | — | Comma-separated status codes to apply to, e.g. `"200,201"`. Unset means all. |

## Rule syntax

```
sub_filter  '<literal>' '<replacement>' [flags]
subs_filter '<regex>'   '<replacement>' [flags]
```

| Flag | Meaning |
| --- | --- |
| `g` | Replace every occurrence instead of only the first |
| `i` | Case-insensitive (`subs_filter` only) |

`subs_filter` replacements use the [`regex`] crate's syntax, so capture groups
are referenced as `$1`, `$2` or `${name}`.

## Examples

```toml
[plugins.rewriteLinks]
category = "sub_filter"
path = "^/docs"
status_codes = "200"
filters = [
    "sub_filter  'http://old.example.com' 'https://new.example.com' g",
    "subs_filter '<title>(.*?)</title>' '<title>$1 — Docs</title>' i",
    "sub_filter  '</head>' '<script src=\"/analytics.js\"></script></head>'",
]

[locations.docs]
upstream = "docs"
path = "/docs"
plugins = ["rewriteLinks"]
```

Filters run in the order they are listed, each operating on the output of the
previous one.

## Behaviour

When the plugin applies, it removes `Content-Length`, switches the response to
`Transfer-Encoding: chunked`, buffers the whole body, applies the filters at end
of stream and emits the result.

### Compressed responses

The plugin understands `Content-Encoding: gzip` (and `x-gzip`),
`Content-Encoding: deflate` (zlib wrapper; raw deflate is attempted as a
fallback), `Content-Encoding: br` (brotli) and `Content-Encoding: zstd`. For
those responses it decompresses the body, runs the filters on the plaintext,
and recompresses with the same algorithm before emitting it:

- `Content-Encoding` is kept (the body is recompressed the same way it
  arrived), `Content-Length` is removed and the response is sent as
  `Transfer-Encoding: chunked`.
- A strong `ETag` is weakened to `W/…` because it described the origin body;
  `Content-MD5` and `Digest` are removed for the same reason; `Vary` gains
  `Accept-Encoding` (the recompressed body differs from what the origin sent
  for other `Accept-Encoding` values).
- **Cap: 8 MiB on the decompressed plaintext** (a few MiB of gzip can expand
  to gigabytes, so capping the raw bytes alone would not stop a compression
  bomb). The read is bounded — each decoder is capped at the limit while
  inflating — so an oversized stream is abandoned mid-decompression instead
  of being fully inflated into memory first. Above the cap, when
  decompression fails or when recompression fails, the original compressed
  bytes are passed through as-is with their `Content-Encoding` kept.
- **No rule matched:** the origin's compressed bytes are emitted verbatim —
  the body is byte-identical and is not recompressed. The response was still
  taken over, so the header normalisation above (chunked, weak `ETag`,
  `Vary: Accept-Encoding`, …) applies as usual.

Encodings the plugin cannot round-trip — multi-value values such as
`gzip, br`, or anything unknown — leave the response completely
untouched (body *and* headers), and a debug line is logged.

### Proxy-level decompression runs first

The proxy may decompress an origin's gzip response **before** the plugin
chain runs: it injects `Accept-Encoding: gzip` on behalf of clients that sent
no `Accept-Encoding` (`[basic] upstream_accept_encoding`, on by default) and
undoes the encoding for those clients. In that case `sub_filter` sees the
plaintext, whatever the response header still said — the proxy's
decoded-response flag makes the plugin take the plaintext branch, so the body
is never decompressed twice.

## Usage notes

- **The entire response body is buffered in memory** before substitution. Scope
  the plugin with `path` and `status_codes` and keep it away from large files or
  streaming endpoints.
- Even when no rule matches, a response the plugin has already taken over is
  converted to chunked transfer encoding and its `ETag` weakened. This is a
  known trade-off of the streaming design: the final length is only known once
  the end of stream is reached.
- A rule that fails to parse is a startup error, so `pingap -t` catches quoting
  mistakes. Patterns and replacements must be wrapped in single quotes and cannot
  themselves contain a single quote.
- Replacement happens on raw bytes, so a match that straddles a multi-byte UTF-8
  boundary in a regex pattern is handled by the regex engine, but literal
  patterns must be given exactly as they appear in the body.

[`regex`]: https://docs.rs/regex/latest/regex/#syntax
