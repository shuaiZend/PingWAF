//! Input normalization: URL decoding, HTML entity decoding, path collapsing,
//! query string and cookie parsing.
//!
//! Everything the detection stages look at flows through here first so that
//! signature matching and libinjection always see attacker-decoded payloads
//! rather than their obfuscated wire form.

pub mod html;
pub mod path;
pub mod url;

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
    /// Wire-format value as the client sent it.
    pub original: String,
    /// Value after URL-decoding (multi-pass) and HTML entity decoding.
    pub decoded: String,
}

/// Fully normalized view of an inbound HTTP request.
#[derive(Debug, Clone)]
pub struct NormalizedRequest {
    pub method: String,
    pub path: String,
    /// Path plus `?query`, with the path portion normalized.
    pub full_uri: String,
    pub query_params: Vec<(String, String)>,
    /// Lower-cased header names paired with their original-cased values.
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    /// Body decoded as UTF-8 when possible (used for body-aware rules).
    pub body_str: Option<String>,
    pub cookies: Vec<(String, String)>,
    /// Flat list of every decoded value the detectors should scan.
    pub decoded_values: Vec<DecodedValue>,
}

impl NormalizedRequest {
    /// Case-insensitive header lookup; returns the first match.
    pub fn header(&self, name: &str) -> Option<&str> {
        let needle = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| *k == needle)
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
#[allow(clippy::too_many_arguments)]
pub fn normalize_request(
    method: &str,
    raw_path: &str,
    raw_query: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
    max_decode_layers: usize,
) -> NormalizedRequest {
    let decoded_path_raw = multi_decode(raw_path, max_decode_layers);
    let normalized_path = normalize_path(&decoded_path_raw);

    let mut decoded_values = Vec::with_capacity(8);
    decoded_values.push(DecodedValue {
        source: ValueSource::Path,
        name: String::new(),
        original: raw_path.to_string(),
        decoded: decoded_path_raw.clone(),
    });

    // NOTE: the raw query is split on `&`/`;` *before* decoding so that an
    // encoded `%26` inside a value is not mistaken for a separator. Each
    // key/value is then multi-decoded individually inside `parse_query`.
    let query_params =
        parse_query(raw_query, max_decode_layers, &mut decoded_values);
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

    // Lower-case header names once so downstream lookups stay cheap.
    let normalized_headers: Vec<(String, String)> = headers
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
        .collect();

    let mut cookies = Vec::new();
    for (k, v) in &normalized_headers {
        if k == "cookie" {
            parse_cookies(
                v,
                max_decode_layers,
                &mut cookies,
                &mut decoded_values,
            );
            continue;
        }
        if SKIP_HEADERS.contains(&k.as_str()) {
            continue;
        }
        let decoded = decode_value(v, max_decode_layers);
        decoded_values.push(DecodedValue {
            source: ValueSource::Header,
            name: k.clone(),
            original: v.clone(),
            decoded,
        });
    }

    let body_owned = body.map(<[u8]>::to_vec);
    let body_str = body_owned
        .as_ref()
        .and_then(|b| String::from_utf8(b.clone()).ok());

    if let Some(text) = body_str.as_ref() {
        // Form-encoded bodies are split into parameters so each value is
        // inspected separately; anything else is scanned as a single blob.
        let ctype = normalized_headers
            .iter()
            .find(|(k, _)| k == "content-type")
            .map(|(_, v)| v.to_ascii_lowercase())
            .unwrap_or_default();
        if ctype.contains("application/x-www-form-urlencoded") {
            parse_form_body(text, max_decode_layers, &mut decoded_values);
        } else {
            let decoded = decode_value(text, max_decode_layers);
            decoded_values.push(DecodedValue {
                source: ValueSource::Body,
                name: String::new(),
                original: text.clone(),
                decoded,
            });
        }
    }

    NormalizedRequest {
        method: method.to_ascii_uppercase(),
        path: normalized_path,
        full_uri,
        query_params,
        headers: normalized_headers,
        body: body_owned,
        body_str,
        cookies,
        decoded_values,
    }
}

/// Run URL multi-decoding plus HTML entity decoding, returning a stable form
/// safe for signature matching.
pub fn decode_value(input: &str, max_decode_layers: usize) -> String {
    let url_decoded = multi_decode(input, max_decode_layers);
    if url_decoded.contains('&') {
        decode_entities(&url_decoded)
    } else {
        url_decoded
    }
}

/// Split a query string on `&`/`;` and decode each key/value pair, pushing
/// every value into `decoded_values` for downstream scanning.
fn parse_query(
    query: &str,
    max_decode_layers: usize,
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
        let val_decoded = decode_value(v, max_decode_layers);
        out.push((key_decoded.clone(), val_decoded.clone()));
        decoded_values.push(DecodedValue {
            source: ValueSource::QueryParam,
            name: key_decoded,
            original: v.to_string(),
            decoded: val_decoded,
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
        let val_decoded = decode_value(v, max_decode_layers);
        decoded_values.push(DecodedValue {
            source: ValueSource::Body,
            name: key_decoded,
            original: v.to_string(),
            decoded: val_decoded,
        });
    }
}

/// Parse a `Cookie:` header value (`a=b; c=d`) and decode each pair.
fn parse_cookies(
    header: &str,
    max_decode_layers: usize,
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
        let val_decoded = decode_value(v, max_decode_layers);
        cookies.push((key_decoded.clone(), val_decoded.clone()));
        decoded_values.push(DecodedValue {
            source: ValueSource::Cookie,
            name: key_decoded,
            original: v.to_string(),
            decoded: val_decoded,
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
        let req = normalize_request("GET", "/", "", &headers, None, 3);
        assert!(!req
            .decoded_values
            .iter()
            .any(|v| v.source == ValueSource::Header
                && v.name == "authorization"));
    }
}
