//! Telegram Bot API delivery for control-plane alerts.

use serde_json::{json, Value};

use super::AlertEvent;

fn token(config: &Value) -> Result<&str, String> {
    let value = config
        .get("bot_token")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();

    let valid = value.split_once(':').is_some_and(|(id, secret)| {
        !id.is_empty()
            && id.bytes().all(|b| b.is_ascii_digit())
            && !secret.is_empty()
            && secret
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    });

    if !valid {
        return Err("config.bot_token is not a valid Telegram bot token".into());
    }

    Ok(value)
}

fn chat_id(config: &Value) -> Result<&str, String> {
    let value = config
        .get("chat_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();

    let valid = value.parse::<i64>().is_ok_and(|id| id != 0)
        || value.strip_prefix('@').is_some_and(|name| {
            !name.is_empty()
                && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        });

    if !valid {
        return Err(
            "config.chat_id must be a numeric chat ID or @channel username"
                .into(),
        );
    }

    Ok(value)
}

fn thread_id(config: &Value) -> Result<Option<i32>, String> {
    match config.get("message_thread_id") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
        Some(value) => {
            let id = value
                .as_i64()
                .and_then(|n| i32::try_from(n).ok())
                .or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()));

            id.filter(|n| *n > 0).map(Some).ok_or_else(|| {
                "config.message_thread_id must be a positive integer".into()
            })
        },
    }
}

pub fn validate_config(config: &Value) -> Result<(), String> {
    token(config)?;
    chat_id(config)?;
    thread_id(config)?;
    Ok(())
}

fn truncate_message(text: &str, max_units: usize) -> String {
    let mut result = String::new();
    let mut units = 0;

    for ch in text.chars() {
        if units + ch.len_utf16() > max_units.saturating_sub(1) {
            result.push('…');
            return result;
        }

        units += ch.len_utf16();
        result.push(ch);
    }

    result
}

fn payload(config: &Value, event: &AlertEvent) -> Result<Value, String> {
    let indicator = match event.severity {
        "critical" => "🔴",
        "warning" => "🟠",
        "info" => "ℹ️",
        _ => "🔔",
    };

    let text = format!(
        "{} PingWAF — {}\n\n{}\n\n\
         Severity: {}\n\
         Event: {}\n\
         Time: {}",
        indicator,
        truncate_message(&event.title, 400),
        truncate_message(&event.message, 3000),
        event.severity,
        truncate_message(&event.event_type, 100),
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
    );

    // Plain text prevents alert content from being interpreted as markup.
    let mut body = json!({
        "chat_id": chat_id(config)?,
        "text": truncate_message(&text, 4096),
        "link_preview_options": {
            "is_disabled": true
        }
    });

    if let Some(id) = thread_id(config)? {
        body["message_thread_id"] = json!(id);
    }

    Ok(body)
}

pub async fn send(
    http: &reqwest::Client,
    config: &Value,
    event: &AlertEvent,
) -> Result<(), String> {
    let token = token(config)?;
    let body = payload(config, event)?;

    // Keep the destination fixed. Never expose the request URL,
    // raw transport error or response body: the URL contains the token.
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");

    let response = http
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|_| "Telegram request failed".to_string())?;

    let status = response.status();

    if status.as_u16() == 429 {
        return Err("Telegram rate limit exceeded (HTTP 429)".into());
    }

    if !status.is_success() {
        return Err(format!("Telegram returned HTTP {status}"));
    }

    let result: Value = response
        .json()
        .await
        .map_err(|_| "Invalid Telegram API response".to_string())?;

    if result.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err("Telegram rejected the notification".into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Value {
        json!({
            "bot_token": "123:abc_DEF-1",
            "chat_id": "-1001234567890"
        })
    }

    fn event() -> AlertEvent {
        AlertEvent {
            event_type: "agent.offline".into(),
            severity: "warning",
            title: "Agent offline".into(),
            message: "Agent has stopped responding.".into(),
            details: None,
            dedup_key: None,
        }
    }

    #[test]
    fn valid_configuration() {
        assert!(validate_config(&config()).is_ok());
    }

    #[test]
    fn invalid_token_is_rejected() {
        for value in ["", "__REDACTED__", "123:abc/evil", "abc:123"] {
            let mut cfg = config();
            cfg["bot_token"] = json!(value);

            assert!(validate_config(&cfg).is_err());
        }
    }

    #[test]
    fn invalid_chat_id_is_rejected() {
        let mut cfg = config();

        for value in ["", "0", "invalid", "@"] {
            cfg["chat_id"] = json!(value);
            assert!(validate_config(&cfg).is_err());
        }
    }

    #[test]
    fn channel_username_is_supported() {
        let mut cfg = config();
        cfg["chat_id"] = json!("@operations_alerts");

        assert!(validate_config(&cfg).is_ok());
    }

    #[test]
    fn topic_id_is_optional() {
        let body = payload(&config(), &event()).unwrap();

        assert!(body.get("message_thread_id").is_none());
    }

    #[test]
    fn topic_id_is_included() {
        let mut cfg = config();
        cfg["message_thread_id"] = json!("42");

        let body = payload(&cfg, &event()).unwrap();

        assert_eq!(body["message_thread_id"], 42);
    }

    #[test]
    fn invalid_topic_is_rejected() {
        let mut cfg = config();
        cfg["message_thread_id"] = json!(-1);

        assert!(validate_config(&cfg).is_err());
    }

    #[test]
    fn message_is_plain_text() {
        let mut evt = event();
        evt.message = "<test> *test* _test_".into();

        let body = payload(&config(), &evt).unwrap();

        assert!(body.get("parse_mode").is_none());
        assert!(body["text"].as_str().unwrap().contains(&evt.message));
    }

    #[test]
    fn message_does_not_contain_token() {
        let body = payload(&config(), &event()).unwrap();

        assert!(!body.to_string().contains("123:abc_DEF-1"));
    }

    #[test]
    fn long_unicode_messages_are_truncated() {
        let mut evt = event();
        evt.message = "😀".repeat(5000);

        let body = payload(&config(), &evt).unwrap();
        let text = body["text"].as_str().unwrap();

        assert!(text.encode_utf16().count() <= 4096);
        assert!(text.contains("Severity: warning"));
    }
}
