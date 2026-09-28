//! Fetching and parsing IP group subscription sources.
//!
//! Two source styles are understood:
//! - Plain text: one IP or CIDR per line, `#` starts a comment.
//! - JSON: either Cloudflare's `/client/v4/ips` document
//!   (`result.ipv4_cidrs` / `result.ipv6_cidrs`, or the newer
//!   `result.items[].ip_prefix` list) or any JSON array of strings.
//!
//! Parsing never panics on foreign payloads: entries that do not parse as an
//! IP/CIDR are skipped, and an empty result is reported to the caller so a
//! broken upstream cannot wipe an existing group.

use serde_json::Value;

/// Builds the HTTP client used for subscription fetches.
pub fn subscription_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("PingWAF/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default()
}

/// Downloads a subscription source and returns its validated IP ranges.
pub async fn fetch_subscription_ranges(
    client: &reqwest::Client,
    url: &str,
) -> Result<Vec<String>, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|err| format!("request failed: {err}"))?;
    if !response.status().is_success() {
        return Err(format!("source answered with HTTP {}", response.status()));
    }
    let body = response
        .text()
        .await
        .map_err(|err| format!("failed to read the response body: {err}"))?;
    parse_subscription(&body)
}

/// Parses a subscription body, auto-detecting JSON and plain-text layouts.
pub fn parse_subscription(body: &str) -> Result<Vec<String>, String> {
    let trimmed = body.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        match serde_json::from_str::<Value>(trimmed) {
            Ok(json) => return json_ranges(&json),
            Err(_) => {
                return Err(
                    "source looks like JSON but does not parse".to_string()
                )
            },
        }
    }
    let ranges = text_ranges(body);
    if ranges.is_empty() {
        return Err("source returned no IP ranges".to_string());
    }
    Ok(ranges)
}

/// Extracts ranges from a JSON document.
fn json_ranges(json: &Value) -> Result<Vec<String>, String> {
    let mut raw: Vec<String> = Vec::new();

    // Plain JSON arrays of strings.
    if let Some(items) = json.as_array() {
        raw.extend(items.iter().filter_map(Value::as_str).map(str::to_string));
    }

    // Cloudflare /client/v4/ips: `result.ipv4_cidrs` / `result.ipv6_cidrs`.
    let result = json.get("result").unwrap_or(json);
    if let Some(cidrs) = result.get("ipv4_cidrs").and_then(Value::as_array) {
        raw.extend(cidrs.iter().filter_map(Value::as_str).map(str::to_string));
    }
    if let Some(cidrs) = result.get("ipv6_cidrs").and_then(Value::as_array) {
        raw.extend(cidrs.iter().filter_map(Value::as_str).map(str::to_string));
    }
    // Newer Cloudflare layout: `result.items[].ip_prefix`.
    if let Some(items) = result.get("items").and_then(Value::as_array) {
        raw.extend(items.iter().filter_map(|item| {
            item.get("ip_prefix")
                .or_else(|| item.get("ip"))
                .and_then(Value::as_str)
                .map(str::to_string)
        }));
    }

    let ranges: Vec<String> = raw
        .into_iter()
        .filter_map(|entry| valid_range(&entry))
        .collect();
    if ranges.is_empty() {
        return Err("no IP ranges found in the JSON source".to_string());
    }
    Ok(ranges)
}

/// One IP or CIDR per line; blank lines and `#` comments are ignored.
fn text_ranges(body: &str) -> Vec<String> {
    body.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(valid_range)
        .collect()
}

/// Accepts a bare IP or a `pfx.len` CIDR (including IPv6 zone-free notation).
fn valid_range(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let base = trimmed.split('/').next().unwrap_or(trimmed);
    if base.parse::<std::net::IpAddr>().is_err() {
        return None;
    }
    Some(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_text_lists() {
        let body = "# comment\n173.245.48.0/20\n\n10.0.0.1\nnot-an-ip\n2400:cb00::/32\n";
        let ranges = parse_subscription(body).unwrap();
        assert_eq!(
            ranges,
            vec!["173.245.48.0/20", "10.0.0.1", "2400:cb00::/32"]
        );
    }

    #[test]
    fn parses_cloudflare_ips_document() {
        let body = r#"{
            "success": true,
            "result": {
                "etag": "abc",
                "ipv4_cidrs": ["173.245.48.0/20", "103.21.244.0/22"],
                "ipv6_cidrs": ["2400:cb00::/32"]
            }
        }"#;
        let ranges = parse_subscription(body).unwrap();
        assert_eq!(
            ranges,
            vec!["173.245.48.0/20", "103.21.244.0/22", "2400:cb00::/32"]
        );
    }

    #[test]
    fn parses_cloudflare_items_document() {
        let body = r#"{
            "result": { "items": [
                { "ip_prefix": "173.245.48.0/20" },
                { "ip_prefix": "2400:cb00::/32" }
            ]}
        }"#;
        let ranges = parse_subscription(body).unwrap();
        assert_eq!(ranges, vec!["173.245.48.0/20", "2400:cb00::/32"]);
    }

    #[test]
    fn parses_json_array_of_strings() {
        let ranges = parse_subscription(r#"["10.0.0.0/8", "junk"]"#).unwrap();
        assert_eq!(ranges, vec!["10.0.0.0/8"]);
    }

    #[test]
    fn rejects_empty_and_broken_sources() {
        assert!(parse_subscription("").is_err());
        assert!(parse_subscription("# only comments\n").is_err());
        assert!(parse_subscription("{not json").is_err());
        assert!(parse_subscription(r#"{"result": {}}"#).is_err());
    }
}
