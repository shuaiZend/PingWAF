//! Webhook senders: WeCom (企业微信), DingTalk (钉钉) and a generic JSON
//! webhook.
//!
//! Both chat platforms receive a Markdown-formatted message; DingTalk
//! additionally supports the "signing key" secret mode, where every request
//! must carry a timestamp plus an HMAC-SHA256 signature of `"{timestamp}\n
//! {secret}"`, base64-encoded and URL-escaped.

use base64::Engine as _;
use hmac::digest::KeyInit as _;
use hmac::Mac;
use serde_json::{Value, json};

use super::AlertEvent;

/// Markdown content is hard-capped by both platforms; keep under it.
const MAX_CONTENT_BYTES: usize = 4000;

/// Renders the alert as Markdown text shared by the chat platforms.
fn markdown(event: &AlertEvent) -> String {
    let details = event
        .details
        .as_ref()
        .map(|details| {
            serde_json::to_string_pretty(details).unwrap_or_default()
        })
        .unwrap_or_default();
    let mut body = format!(
        "**{}**\n\n{}\n\nseverity: `{}`\ntime: `{}`",
        event.title,
        event.message,
        event.severity,
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
    );
    if !details.is_empty() && details != "null" {
        body.push_str(&format!("\n\n```\n{details}\n```"));
    }
    truncate_chars(&body, MAX_CONTENT_BYTES)
}

/// Cuts on a character boundary so multi-byte text never panics.
fn truncate_chars(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n…", &text[..end])
}

/// POSTs a JSON payload, mapping transport errors and non-2xx replies into a
/// readable message.
async fn post_json(
    http: &reqwest::Client,
    url: &str,
    payload: &Value,
) -> Result<(), String> {
    let response = http
        .post(url)
        .header("Content-Type", "application/json")
        .json(payload)
        .send()
        .await
        .map_err(|err| format!("request failed: {err}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        // Keep the reply short: chat APIs echo long HTML error pages.
        let body = truncate_chars(body.trim(), 200);
        return Err(format!("webhook replied {status}: {body}"));
    }
    Ok(())
}

/// WeCom group-bot webhook: `{"msgtype":"markdown","markdown":{...}}`.
pub async fn send_wecom(
    http: &reqwest::Client,
    config: &Value,
    event: &AlertEvent,
) -> Result<(), String> {
    let url = config
        .get("url")
        .and_then(Value::as_str)
        .filter(|url| !url.trim().is_empty())
        .ok_or_else(|| "missing 'url' in the channel config".to_string())?;
    let payload = json!({
        "msgtype": "markdown",
        "markdown": { "content": markdown(event) },
    });
    post_json(http, url.trim(), &payload).await
}

/// DingTalk custom-bot webhook, with optional HMAC-SHA256 signing.
pub async fn send_dingtalk(
    http: &reqwest::Client,
    config: &Value,
    event: &AlertEvent,
) -> Result<(), String> {
    let mut url = config
        .get("url")
        .and_then(Value::as_str)
        .filter(|url| !url.trim().is_empty())
        .ok_or_else(|| "missing 'url' in the channel config".to_string())?
        .trim()
        .to_string();

    if let Some(secret) = config
        .get("secret")
        .and_then(Value::as_str)
        .filter(|secret| !secret.trim().is_empty())
    {
        let signed = sign_url(&url, secret.trim())
            .map_err(|err| format!("cannot sign the webhook URL: {err}"))?;
        url = signed;
    }

    let payload = json!({
        "msgtype": "markdown",
        "markdown": {
            "title": event.title,
            "text": markdown(event),
        },
    });
    post_json(http, &url, &payload).await
}

/// Appends `timestamp` and `sign` query parameters to a DingTalk webhook URL.
pub fn sign_url(url: &str, secret: &str) -> Result<String, String> {
    let timestamp = chrono::Utc::now().timestamp_millis();
    let string_to_sign = format!("{timestamp}\n{secret}");

    type HmacSha256 = hmac::Hmac<sha2::Sha256>;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|err| err.to_string())?;
    hmac::digest::Update::update(&mut mac, string_to_sign.as_bytes());
    let signature = mac.finalize().into_bytes();
    let encoded =
        base64::engine::general_purpose::STANDARD.encode(signature);

    let separator = if url.contains('?') { '&' } else { '?' };
    Ok(format!(
        "{url}{separator}timestamp={timestamp}&sign={}",
        urlencode(&encoded)
    ))
}

/// Percent-encodes everything outside the RFC 3986 unreserved set.
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~' => out.push(byte as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// Generic webhook: the alert is POSTed as JSON, optionally with a
/// `X-PingWAF-Token` header for simple shared-secret protection.
pub async fn send_generic(
    http: &reqwest::Client,
    config: &Value,
    event: &AlertEvent,
) -> Result<(), String> {
    let url = config
        .get("url")
        .and_then(Value::as_str)
        .filter(|url| !url.trim().is_empty())
        .ok_or_else(|| "missing 'url' in the channel config".to_string())?;

    let mut request = http
        .post(url.trim())
        .header("Content-Type", "application/json")
        .header("User-Agent", "PingWAF/notifications");
    if let Some(token) = config
        .get("secret_token")
        .and_then(Value::as_str)
        .filter(|token| !token.trim().is_empty())
    {
        request = request.header("X-PingWAF-Token", token.trim());
    }

    let payload = json!({
        "event": event.event_type,
        "severity": event.severity,
        "title": event.title,
        "message": event.message,
        "details": event.details,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });
    let response = request
        .json(&payload)
        .send()
        .await
        .map_err(|err| format!("request failed: {err}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("webhook replied {status}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_respects_char_boundaries() {
        let text = "告警信息".repeat(2000);
        let cut = truncate_chars(&text, 100);
        assert!(cut.len() <= 110);
        assert!(cut.ends_with('…'));
        // No panic is the real assertion; the cut is valid UTF-8 by type.
    }

    #[test]
    fn truncation_keeps_short_text() {
        assert_eq!(truncate_chars("hello", 100), "hello");
    }

    #[test]
    fn signing_appends_timestamp_and_sign() {
        let signed = sign_url("https://oapi.dingtalk.com/robot/send?access_token=x", "SEC123").unwrap();
        assert!(signed.starts_with(
            "https://oapi.dingtalk.com/robot/send?access_token=x&timestamp="
        ));
        assert!(signed.contains("&sign="));
        // The signature is base64, percent-encoded for the query string.
        let sign = signed.rsplit("sign=").next().unwrap_or_default();
        assert!(!sign.contains('=') || sign.contains('%'));
    }

    #[test]
    fn signing_reuses_the_existing_query_separator() {
        let signed = sign_url("https://example.com/hook", "s").unwrap();
        assert!(signed.contains("/hook?timestamp="));
    }
}
