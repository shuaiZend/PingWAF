//! Input normalization: URL decoding, HTML entity decoding, escape-sequence
//! decoding, path collapsing, query string and cookie parsing.
//!
//! Everything the detection stages look at flows through here first so that
//! signature matching and libinjection always see attacker-decoded payloads
//! rather than their obfuscated wire form.
//!
//! The normalized view *borrows* the request's headers and body instead of
//! copying them — the wire-format data already lives in [`RequestData`]
//! upstream, and duplicating it per request was the engine's largest
//! avoidable allocation cost.

pub mod html;
pub mod path;
pub mod url;

use std::borrow::Cow;

use crate::normalize::html::decode_entities;
use crate::normalize::path::normalize as normalize_path;
use crate::normalize::url::multi_decode;

/// Where a particular decoded value came from. Used for logging and for
/// per-source weight tuning in the rule engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueSource {
    QueryParam,
    Header,
    Cookie,
    Body,
    Path,
}

impl ValueSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ValueSource::QueryParam => "query",
            ValueSource::Header => "header",
            ValueSource::Cookie => "cookie",
            ValueSource::Body => "body",
            ValueSource::Path => "path",
        }
    }
}

/// A single normalized field of the request, paired with its decoded form.
#[derive(Debug, Clone)]
pub struct DecodedValue {
    pub source: ValueSource,
    /// Parameter / header / cookie name (empty for the path itself).
    pub name: String,
    /// Value after URL-decoding, entity decoding and (optionally) escape
    /// decoding.
    pub decoded: String,
    /// `true` when the value is a discrete input field (query param, cookie,
    /// header, form member, JSON member) rather than an opaque body blob.
    /// Typed fields are the classic injection surface and carry full
    /// severity; bulk blobs (analytics JSON dumps, pasted HTML) often
    /// contain the same strings benignly, so the engine weights blob hits
    /// as weak signals.
    pub field: bool,
}

/// Fully normalized view of an inbound HTTP request. Borrows the request's
/// headers and body; only decoded/parsed material is owned.
#[derive(Debug, Clone)]
pub struct NormalizedRequest<'a> {
    /// Uppercased method — borrowed when the wire form was already uppercase
    /// (the common case), owned otherwise.
    pub method: Cow<'a, str>,
    pub path: String,
    /// Path plus `?query`, with the path portion normalized.
    pub full_uri: String,
    pub query_params: Vec<(String, String)>,
    /// The request's own header list, borrowed verbatim. Header lookups are
    /// ASCII case-insensitive.
    pub headers: &'a [(String, String)],
    pub body: Option<&'a [u8]>,
    pub cookies: Vec<(String, String)>,
    /// Flat list of every decoded value the detectors should scan.
    pub decoded_values: Vec<DecodedValue>,
}

impl NormalizedRequest<'_> {
    /// Body decoded as UTF-8 when possible. A borrow, not a copy — the
    /// previous `Option<String>` field cloned the entire body on every
    /// request that carried one.
    pub fn body_str(&self) -> Option<&str> {
        self.body.and_then(|b| std::str::from_utf8(b).ok())
    }

    /// Case-insensitive header lookup; returns the first match.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Host header (falls back to `:authority` for HTTP/2 captures).
    pub fn host(&self) -> &str {
        self.header("host")
            .or_else(|| self.header(":authority"))
            .unwrap_or("")
    }

    /// User-Agent header (empty when absent).
    pub fn user_agent(&self) -> &str {
        self.header("user-agent").unwrap_or("")
    }
}

/// Headers whose value should never be inspected as user-controlled data.
/// Skipping these keeps the signature engine from firing on, e.g., the
/// `Cookie` header itself when we already inspect its parsed components.
const SKIP_HEADERS: &[&str] = &[
    "cookie",
    "authorization",
    "proxy-authorization",
    "content-length",
    "transfer-encoding",
];

/// Decode and normalize an entire request.
///
/// `max_decode_layers` caps how many URL-decoding passes are applied to defend
/// against double / triple-encoded payloads without opening a CPU DoS.
/// `decode_escapes` additionally resolves `\xHH` / `\uHHHH` sequences — a
/// Strict-level extra pass, off by default to keep the Normal hot path lean.
#[allow(clippy::too_many_arguments)]
pub fn normalize_request<'a>(
    method: &'a str,
    raw_path: &str,
    raw_query: &str,
    headers: &'a [(String, String)],
    body: Option<&'a [u8]>,
    max_decode_layers: usize,
    decode_escapes: bool,
) -> NormalizedRequest<'a> {
    let decoded_path_raw = multi_decode(raw_path, max_decode_layers);
    let normalized_path = normalize_path(&decoded_path_raw);

    let mut decoded_values = Vec::with_capacity(8);
    decoded_values.push(DecodedValue {
        source: ValueSource::Path,
        name: String::new(),
        decoded: decoded_path_raw.clone(),
        field: true,
    });

    // NOTE: the raw query is split on `&` *before* decoding so that an
    // encoded `%26` inside a value is not mistaken for a separator. Each
    // key/value is then multi-decoded individually inside `parse_query`.
    let query_params = parse_query(
        raw_query,
        max_decode_layers,
        decode_escapes,
        &mut decoded_values,
    );
    let full_uri = if query_params.is_empty() {
        normalized_path.clone()
    } else {
        let qs: Vec<String> = query_params
            .iter()
            .map(|(k, v)| {
                if v.is_empty() {
                    k.clone()
                } else {
                    format!("{}={}", k, v)
                }
            })
            .collect();
        format!("{}?{}", normalized_path, qs.join("&"))
    };

    // The method only needs re-allocating when the wire form was not already
    // uppercase; proxy stacks hand us uppercase methods ~always.
    let method = if method.bytes().any(|b| b.is_ascii_lowercase()) {
        Cow::Owned(method.to_ascii_uppercase())
    } else {
        Cow::Borrowed(method)
    };

    let mut cookies = Vec::new();
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("cookie") {
            parse_cookies(
                v,
                max_decode_layers,
                decode_escapes,
                &mut cookies,
                &mut decoded_values,
            );
            continue;
        }
        if SKIP_HEADERS.iter().any(|s| k.eq_ignore_ascii_case(s)) {
            continue;
        }
        let decoded = decode_value(v, max_decode_layers, decode_escapes);
        decoded_values.push(DecodedValue {
            source: ValueSource::Header,
            name: k.clone(),
            decoded,
            field: true,
        });
    }

    if let Some(body_bytes) = body {
        // Form-encoded bodies are split into parameters so each value is
        // inspected separately; anything else is scanned as a single blob.
        let ctype = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v.to_ascii_lowercase())
            .unwrap_or_default();
        if let Ok(text) = std::str::from_utf8(body_bytes) {
            if ctype.contains("application/x-www-form-urlencoded") {
                parse_form_body(
                    text,
                    max_decode_layers,
                    decode_escapes,
                    &mut decoded_values,
                );
            } else if ctype.contains("json") || looks_like_json(text) {
                // Structured bodies: keep the raw blob (bulk content can be
                // benign), then add every string member as a *field* so the
                // detectors see typed inputs — with JSON escape handling
                // (`\u003c`) resolved by the parser itself.
                let decoded =
                    decode_value(text, max_decode_layers, decode_escapes);
                decoded_values.push(DecodedValue {
                    source: ValueSource::Body,
                    name: String::new(),
                    decoded,
                    field: false,
                });
                unpack_json(
                    text,
                    max_decode_layers,
                    decode_escapes,
                    &mut decoded_values,
                );
            } else {
                let decoded =
                    decode_value(text, max_decode_layers, decode_escapes);
                decoded_values.push(DecodedValue {
                    source: ValueSource::Body,
                    name: String::new(),
                    decoded,
                    field: false,
                });
            }
        }
    }

    NormalizedRequest {
        method,
        path: normalized_path,
        full_uri,
        query_params,
        headers,
        body,
        cookies,
        decoded_values,
    }
}

/// Run URL multi-decoding plus HTML entity decoding, returning a stable form
/// safe for signature matching. With `decode_escapes`, `\xHH` / `\uHHHH`
/// sequences are resolved as well.
pub fn decode_value(
    input: &str,
    max_decode_layers: usize,
    decode_escapes: bool,
) -> String {
    let url_decoded = multi_decode(input, max_decode_layers);
    let out = if url_decoded.contains('&') {
        decode_entities(&url_decoded)
    } else {
        url_decoded
    };
    if decode_escapes {
        decode_escapes_owned(&out)
    } else {
        out
    }
}

/// `decode_value` with form-encoding semantics: a bare `+` means space, the
/// way every mainstream runtime (nginx, PHP, Tomcat, …) parses query strings
/// and `application/x-www-form-urlencoded` bodies. Without it a payload like
/// `' OR 1=1--` sent as `%27+OR+1%3D1--` keeps its `+` separators and every
/// whitespace-sensitive detector (fingerprinting, regexes) sees one token and
/// walks past. Literal `+` stays reachable via `%2B`, and header / cookie /
/// path values use plain [`decode_value`] because there `+` is a literal.
fn decode_value_form(
    input: &str,
    max_decode_layers: usize,
    decode_escapes: bool,
) -> String {
    if input.contains('+') {
        return decode_value(
            &input.replace('+', " "),
            max_decode_layers,
            decode_escapes,
        );
    }
    decode_value(input, max_decode_layers, decode_escapes)
}

/// Resolve `\xHH` and `\uHHHH` escape sequences. Returns the input untouched
/// (borrowed) when it contains no escape marker, so the Normal-level path
/// never allocates for this.
pub fn decode_escapes(input: &str) -> Cow<'_, str> {
    let has_marker = input.contains("\\x")
        || input.contains("\\X")
        || input.contains("\\u")
        || input.contains("\\U");
    if !has_marker {
        return Cow::Borrowed(input);
    }
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 1 < bytes.len()
            && matches!(bytes[i + 1], b'x' | b'X' | b'u' | b'U')
        {
            let width = if matches!(bytes[i + 1], b'x' | b'X') {
                2
            } else {
                4
            };
            let hex_end = (i + 2 + width).min(bytes.len());
            if hex_end == i + 2 + width
                && bytes[i + 2..hex_end].iter().all(|b| b.is_ascii_hexdigit())
            {
                let n = u32::from_str_radix(&input[i + 2..hex_end], 16)
                    .unwrap_or(0);
                if let Some(c) = char::from_u32(n) {
                    out.push(c);
                    i = hex_end;
                    continue;
                }
            }
        }
        // Copy one full UTF-8 scalar (not one byte) so multi-byte sequences
        // pass through verbatim.
        let ch_len = input[i..].chars().next().map_or(1, char::len_utf8);
        out.push_str(&input[i..i + ch_len]);
        i += ch_len;
    }
    Cow::Owned(out)
}

fn decode_escapes_owned(input: &str) -> String {
    match decode_escapes(input) {
        Cow::Borrowed(s) => s.to_string(),
        Cow::Owned(s) => s,
    }
}

/// Split a query string on `&` and decode each key/value pair, pushing
/// every value into `decoded_values` for downstream scanning.
fn parse_query(
    query: &str,
    max_decode_layers: usize,
    decode_escapes: bool,
    decoded_values: &mut Vec<DecodedValue>,
) -> Vec<(String, String)> {
    if query.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(4);
    // Split on `&` only. `;` is *not* treated as a separator: doing so would
    // cut classic command-injection payloads (`cmd=;cat /etc/passwd`) into an
    // empty value plus a name-only fragment, hiding the attack from the
    // detectors.
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        let key_decoded = multi_decode(k, max_decode_layers);
        let val_decoded =
            decode_value_form(v, max_decode_layers, decode_escapes);
        out.push((key_decoded.clone(), val_decoded.clone()));
        decoded_values.push(DecodedValue {
            source: ValueSource::QueryParam,
            name: key_decoded,
            decoded: val_decoded,
            field: true,
        });
    }
    out
}

/// Parse a `application/x-www-form-urlencoded` body. Same shape as
/// [`parse_query`] but tags every value with [`ValueSource::Body`] so the
/// engine can weight body hits differently from query-string hits.
fn parse_form_body(
    body: &str,
    max_decode_layers: usize,
    decode_escapes: bool,
    decoded_values: &mut Vec<DecodedValue>,
) {
    if body.is_empty() {
        return;
    }
    // See `parse_query` — `;` must stay part of the value.
    for pair in body.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        let key_decoded = multi_decode(k, max_decode_layers);
        let val_decoded =
            decode_value_form(v, max_decode_layers, decode_escapes);
        // A form field whose value is itself serialized JSON (APIs that
        // embed `param={"add":5,"delete":0}`) is unpacked in place — the
        // `"key":value` container shape trips libinjection's quote-keyword
        // fingerprint on completely benign payloads.
        if !val_decoded.is_empty()
            && unpack_string_json(
                &val_decoded,
                0,
                &key_decoded,
                max_decode_layers,
                decode_escapes,
                decoded_values,
            )
        {
            continue;
        }
        decoded_values.push(DecodedValue {
            source: ValueSource::Body,
            name: key_decoded,
            decoded: val_decoded,
            field: true,
        });
    }
}

/// Upper bounds that keep JSON unpacking a bounded operation regardless of
/// what the client sends: recursion depth, per-value byte length and the
/// total number of extracted members. The depth budget covers a nested
/// serialized-JSON string (telemetry beacons embed JSON-in-JSON three to
/// four levels deep), so the walk may descend into a string member's own
/// object tree as well.
const JSON_MAX_DEPTH: usize = 6;
const JSON_MAX_VALUE_LEN: usize = 4096;
const JSON_MAX_VALUES: usize = 64;

/// A JSON-looking body: JSON content type or an object/array opening brace
/// after leading whitespace. The sniff only *enables* the parse — a parse
/// failure falls back to blob-only scanning.
fn looks_like_json(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with('{') || trimmed.starts_with('[')
}

/// Recursively extract string members of a JSON body as typed fields. The
/// parser resolves JSON escape sequences (`\u003c`, `\/`) for us, closing
/// the escape-obfuscation gap without turning on the Strict escape pass.
/// Bounded by [`JSON_MAX_DEPTH`], [`JSON_MAX_VALUE_LEN`] and
/// [`JSON_MAX_VALUES`]; a parse failure simply yields nothing.
fn unpack_json(
    body: &str,
    max_decode_layers: usize,
    decode_escapes: bool,
    decoded_values: &mut Vec<DecodedValue>,
) {
    if body.len() > JSON_MAX_VALUE_LEN * 64 {
        return;
    }
    let Ok(root) = serde_json::from_str::<serde_json::Value>(body) else {
        return;
    };
    let mut count = 0usize;
    walk_json(
        &root,
        0,
        String::new(),
        max_decode_layers,
        decode_escapes,
        decoded_values,
        &mut count,
    );
}

/// Parse a string value that is itself serialized JSON and unpack its
/// members into `decoded_values` under `path`. Returns `true` when the value
/// was replaced by its unpacked members — the caller must then *not* push
/// the container string itself (the `"key":value` shape trips libinjection's
/// quote-keyword fingerprint on benign structured payloads). Depth shares
/// the [`JSON_MAX_DEPTH`] budget of the surrounding walk.
fn unpack_string_json(
    s: &str,
    depth: usize,
    path: &str,
    max_decode_layers: usize,
    decode_escapes: bool,
    decoded_values: &mut Vec<DecodedValue>,
) -> bool {
    if depth >= JSON_MAX_DEPTH
        || s.is_empty()
        || s.len() > JSON_MAX_VALUE_LEN
        || !looks_like_json(s)
    {
        return false;
    }
    let Ok(nested) = serde_json::from_str::<serde_json::Value>(s) else {
        return false;
    };
    let before = decoded_values.len();
    let mut count = 0usize;
    walk_json(
        &nested,
        depth + 1,
        path.to_string(),
        max_decode_layers,
        decode_escapes,
        decoded_values,
        &mut count,
    );
    // Replace the container only when members were actually extracted —
    // a value the depth budget rejected stays put rather than vanishing.
    decoded_values.len() > before
}

#[allow(clippy::too_many_arguments)]
fn walk_json(
    value: &serde_json::Value,
    depth: usize,
    path: String,
    max_decode_layers: usize,
    decode_escapes: bool,
    decoded_values: &mut Vec<DecodedValue>,
    count: &mut usize,
) {
    if *count >= JSON_MAX_VALUES {
        return;
    }
    match value {
        serde_json::Value::String(s) => {
            if s.is_empty() || s.len() > JSON_MAX_VALUE_LEN {
                return;
            }
            // A string member that is itself serialized JSON (nested
            // telemetry payloads, icons, embedded config blobs) is unpacked
            // in place instead of being scanned as a container — see
            // [`unpack_string_json`].
            if unpack_string_json(
                s,
                depth,
                &path,
                max_decode_layers,
                decode_escapes,
                decoded_values,
            ) {
                *count += 1;
                return;
            }
            let decoded = decode_value(s, max_decode_layers, decode_escapes);
            decoded_values.push(DecodedValue {
                source: ValueSource::Body,
                name: path,
                decoded,
                field: true,
            });
            *count += 1;
        },
        serde_json::Value::Array(items) => {
            if depth >= JSON_MAX_DEPTH {
                return;
            }
            for (i, item) in items.iter().enumerate() {
                walk_json(
                    item,
                    depth + 1,
                    format!("{path}[{i}]"),
                    max_decode_layers,
                    decode_escapes,
                    decoded_values,
                    count,
                );
            }
        },
        serde_json::Value::Object(map) => {
            if depth >= JSON_MAX_DEPTH {
                return;
            }
            for (k, v) in map {
                let child = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                walk_json(
                    v,
                    depth + 1,
                    child,
                    max_decode_layers,
                    decode_escapes,
                    decoded_values,
                    count,
                );
            }
        },
        // Scalars become scannable strings so a nested container made only
        // of numbers/booleans still yields members (keeping the
        // container-replacement rule honest). Scalar values carry no
        // injection payload on their own.
        serde_json::Value::Number(n) => {
            if *count >= JSON_MAX_VALUES {
                return;
            }
            decoded_values.push(DecodedValue {
                source: ValueSource::Body,
                name: path,
                decoded: n.to_string(),
                field: true,
            });
            *count += 1;
        },
        serde_json::Value::Bool(b) => {
            if *count >= JSON_MAX_VALUES {
                return;
            }
            decoded_values.push(DecodedValue {
                source: ValueSource::Body,
                name: path,
                decoded: b.to_string(),
                field: true,
            });
            *count += 1;
        },
        _ => {},
    }
}

/// Parse a `Cookie:` header value (`a=b; c=d`) and decode each pair.
fn parse_cookies(
    header: &str,
    max_decode_layers: usize,
    decode_escapes: bool,
    cookies: &mut Vec<(String, String)>,
    decoded_values: &mut Vec<DecodedValue>,
) {
    for pair in header.split(';') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (k.trim(), v.trim()),
            None => (pair, ""),
        };
        let key_decoded = multi_decode(k, max_decode_layers);
        let val_decoded = decode_value(v, max_decode_layers, decode_escapes);
        cookies.push((key_decoded.clone(), val_decoded.clone()));
        decoded_values.push(DecodedValue {
            source: ValueSource::Cookie,
            name: key_decoded,
            decoded: val_decoded,
            field: true,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_query_and_cookies() {
        let headers = vec![
            ("User-Agent".to_string(), "curl/8.0".to_string()),
            ("Cookie".to_string(), "a=1; b=hello%20world".to_string()),
        ];
        let req = normalize_request(
            "get",
            "/foo/./bar",
            "x=1&y=hello%20world",
            &headers,
            None,
            3,
            false,
        );
        assert_eq!(req.method, "GET");
        assert_eq!(req.path, "/foo/bar");
        assert_eq!(req.query_params.len(), 2);
        assert_eq!(
            req.cookies,
            vec![
                ("a".to_string(), "1".to_string()),
                ("b".to_string(), "hello world".to_string()),
            ]
        );
        // path + 2 query + UA + 2 cookies
        assert_eq!(req.decoded_values.len(), 6);
    }

    #[test]
    fn double_encoded_payloads_are_decoded() {
        let req = normalize_request(
            "GET",
            "/x",
            "p=%252e%252e%252f%252e%252e%252fetc%252fpasswd",
            &[],
            None,
            3,
            false,
        );
        let v = req
            .decoded_values
            .iter()
            .find(|v| v.name == "p")
            .expect("param p");
        assert_eq!(v.decoded, "../../etc/passwd");
    }

    #[test]
    fn skips_authorization_header() {
        let headers =
            vec![("Authorization".to_string(), "Bearer xyz".to_string())];
        let req = normalize_request("GET", "/", "", &headers, None, 3, false);
        assert!(!req
            .decoded_values
            .iter()
            .any(|v| v.source == ValueSource::Header
                && v.name == "authorization"));
    }

    #[test]
    fn headers_are_borrowed_not_copied() {
        let headers = vec![("X-Custom".to_string(), "probe".to_string())];
        let body = b"payload".to_vec();
        let req =
            normalize_request("GET", "/", "", &headers, Some(&body), 3, false);
        assert_eq!(req.headers.len(), 1);
        assert_eq!(req.body, Some(body.as_slice()));
        assert_eq!(req.body_str(), Some("payload"));
    }

    #[test]
    fn method_borrowed_when_uppercase() {
        let req = normalize_request("GET", "/", "", &[], None, 3, false);
        assert!(matches!(req.method, Cow::Borrowed("GET")));
        let req = normalize_request("get", "/", "", &[], None, 3, false);
        assert!(matches!(req.method, Cow::Owned(_)));
        assert_eq!(req.method, "GET");
    }

    #[test]
    fn escapes_decoded_only_when_enabled() {
        let headers = vec![];
        let req = normalize_request(
            "GET",
            "/x",
            r"p=%5Cx3cscript%3E",
            &headers,
            None,
            3,
            false,
        );
        let v = req
            .decoded_values
            .iter()
            .find(|v| v.name == "p")
            .expect("param p");
        assert_eq!(v.decoded, r"\x3cscript>");
        assert!(!v.decoded.contains("<script"));

        let req = normalize_request(
            "GET",
            "/x",
            r"p=%5Cx3cscript%3E",
            &headers,
            None,
            3,
            true,
        );
        let v = req
            .decoded_values
            .iter()
            .find(|v| v.name == "p")
            .expect("param p");
        assert_eq!(v.decoded, "<script>");
    }

    #[test]
    fn unicode_escape_decoded() {
        assert_eq!(decode_escapes(r"a\u003cb"), "a<b");
        assert_eq!(decode_escapes("no escapes"), "no escapes");
        // Incomplete / invalid escapes stay verbatim.
        assert_eq!(decode_escapes(r"\xzz"), r"\xzz");
        assert_eq!(decode_escapes(r"\uD83D"), r"\uD83D");
    }

    #[test]
    fn plus_is_space_in_query_and_form_body() {
        let req = normalize_request(
            "GET",
            "/",
            "id=%27+OR+1%3D1--",
            &[],
            None,
            3,
            false,
        );
        let v = req
            .decoded_values
            .iter()
            .find(|v| v.name == "id")
            .expect("param id");
        assert_eq!(v.decoded, "' OR 1=1--");

        let headers = vec![(
            "content-type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        )];
        let req = normalize_request(
            "POST",
            "/",
            "",
            &headers,
            Some(b"q=waitfor+delay+%270:0:10%27".as_slice()),
            3,
            false,
        );
        let v = req
            .decoded_values
            .iter()
            .find(|v| v.name == "q")
            .expect("body param q");
        assert_eq!(v.decoded, "waitfor delay '0:0:10'");
    }

    #[test]
    fn plus_stays_literal_in_headers_and_encoded_plus() {
        let headers =
            vec![("User-Agent".to_string(), "C++_runtime".to_string())];
        let req =
            normalize_request("GET", "/a+b", "k=%2B", &headers, None, 3, false);
        let ua = req
            .decoded_values
            .iter()
            .find(|v| v.name.eq_ignore_ascii_case("user-agent"))
            .expect("ua value");
        assert_eq!(ua.decoded, "C++_runtime");
        let v = req
            .decoded_values
            .iter()
            .find(|v| v.name == "k")
            .expect("param k");
        assert_eq!(v.decoded, "+");
    }
}
