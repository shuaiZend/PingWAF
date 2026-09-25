// Copyright 2024-2025 Tree xie.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Challenge plugin — CC protection / "5-second shield".
//!
//! Runs at [`PluginStep::EarlyRequest`]. For every request it either:
//! * serves the `/_pingwaf/challenge/verify` endpoint (solution submission), or
//! * asks the [`ChallengeEngine`] whether the visitor should be challenged and,
//!   if so, returns the JS challenge page (503) or a hard block (403).
//!
//! The WAF plugin delegates its `Challenge` verdicts here: both plugins share
//! the [`PENDING_CHALLENGES`] store so a challenge issued by one can be
//! verified by the other's verify endpoint.

use super::{
    Error, get_hash_key, get_int_conf_or_default, get_str_conf,
    get_str_slice_conf,
};
use async_trait::async_trait;
use bytes::{BufMut, BytesMut};
use dashmap::DashMap;
use http::header;
use http::{HeaderValue, StatusCode};
use pingap_config::{PluginCategory, PluginConf};
use pingap_core::{
    Ctx, HTTP_HEADER_CONTENT_HTML, HttpResponse, Plugin, PluginStep,
    RequestPluginResult, ensure_client_ip, get_cookie_value, get_host,
    get_req_header_value,
};
use pingora::proxy::Session;
use pingwaf_challenge::js_challenge::page::{
    ChallengePageParams, generate_interactive_challenge_html,
    generate_js_challenge_html, generate_managed_challenge_html,
};
use pingwaf_challenge::{
    ChallengeConfig, ChallengeDecision, ChallengeEngine, ChallengeRequest,
    ChallengeSubmission, ClearanceLevel, VerifyResult, generate_request_id,
};
use std::borrow::Cow;
use std::sync::{Arc, LazyLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, warn};

type Result<T, E = Error> = std::result::Result<T, E>;

/// Endpoint the challenge page POSTs its proof-of-work solution to.
pub(crate) const VERIFY_ENDPOINT: &str = "/_pingwaf/challenge/verify";

/// Maximum request-body size accepted on the verify endpoint (64 KiB).
const MAX_VERIFY_BODY: usize = 64 * 1024;

/// Sliding fixed-window (60s) length used for per-IP rate estimation.
const RATE_WINDOW_SECS: i64 = 60;

// ─────────────────────────────────────────────────────────────
// Shared pending-challenge store
// ─────────────────────────────────────────────────────────────

/// State kept server-side between issuing a challenge and verifying the
/// solution. The browser only echoes back `{request_id, solution,
/// fingerprint_json, timestamp}` — the nonce and site id never leave the edge,
/// so they are looked up here by request id.
pub(crate) struct PendingChallenge {
    pub nonce: String,
    pub site_id: String,
    pub original_url: String,
    pub created_at: i64,
}

/// Process-wide map of outstanding challenges, keyed by request id. Shared by
/// the WAF and Challenge plugins.
pub(crate) static PENDING_CHALLENGES: LazyLock<
    DashMap<String, PendingChallenge>,
> = LazyLock::new(DashMap::new);

/// How long an unverified challenge stays resolvable (5 minutes).
const PENDING_TTL_SECS: i64 = 300;

#[inline]
fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Generate a random 32-hex-char nonce (the challenge crate keeps its own
/// generator private).
fn generate_nonce() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Record a freshly issued challenge, opportunistically dropping stale ones.
pub(crate) fn store_pending(
    request_id: &str,
    site_id: &str,
    original_url: &str,
) -> String {
    let now = now_secs();
    // Cheap garbage collection so the map cannot grow without bound.
    if PENDING_CHALLENGES.len() > 1024 {
        PENDING_CHALLENGES
            .retain(|_, v| now.saturating_sub(v.created_at) < PENDING_TTL_SECS);
    }
    let nonce = generate_nonce();
    PENDING_CHALLENGES.insert(
        request_id.to_string(),
        PendingChallenge {
            nonce: nonce.clone(),
            site_id: site_id.to_string(),
            original_url: original_url.to_string(),
            created_at: now,
        },
    );
    nonce
}

/// Take (and remove) the pending challenge for a request id.
fn take_pending(request_id: &str) -> Option<PendingChallenge> {
    let pending = PENDING_CHALLENGES.remove(request_id).map(|(_, v)| v)?;
    if now_secs().saturating_sub(pending.created_at) >= PENDING_TTL_SECS {
        return None;
    }
    Some(pending)
}

/// Which challenge page to render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChallengeKind {
    Js,
    Managed,
    Interactive,
}

/// Build a 503 challenge response, registering the pending challenge so the
/// verify endpoint can later resolve the nonce.
pub(crate) fn build_challenge_response(
    request_id: &str,
    original_url: &str,
    site_id: &str,
    difficulty: u32,
    kind: ChallengeKind,
) -> HttpResponse {
    let nonce = store_pending(request_id, site_id, original_url);
    let params = ChallengePageParams {
        request_id: request_id.to_string(),
        challenge_nonce: nonce,
        difficulty,
        verify_endpoint: VERIFY_ENDPOINT.to_string(),
        original_url: original_url.to_string(),
        brand_name: "PingWAF".to_string(),
    };
    let body = match kind {
        ChallengeKind::Js => generate_js_challenge_html(&params),
        ChallengeKind::Managed => generate_managed_challenge_html(&params),
        ChallengeKind::Interactive => {
            generate_interactive_challenge_html(&params)
        },
    };
    HttpResponse::builder(StatusCode::SERVICE_UNAVAILABLE)
        .body(body)
        .header(HTTP_HEADER_CONTENT_HTML.clone())
        .no_store()
        .finish()
}

/// A simple 403 HTML block page.
pub(crate) fn block_page(request_id: &str, reason: &str) -> HttpResponse {
    let body = format!(
        "<html><body><h1>403 Forbidden</h1><p>Request blocked by PingWAF.</p>\
<p>Reason: {reason}</p><p>Event ID: {request_id}</p></body></html>"
    );
    HttpResponse::builder(StatusCode::FORBIDDEN)
        .body(body)
        .header(HTTP_HEADER_CONTENT_HTML.clone())
        .no_store()
        .finish()
}

// ─────────────────────────────────────────────────────────────
// Per-IP fixed-window rate counter
// ─────────────────────────────────────────────────────────────

struct RateWindow {
    count: u32,
    window_start: i64,
}

// ─────────────────────────────────────────────────────────────
// ChallengePlugin
// ─────────────────────────────────────────────────────────────

/// CC protection / browser challenge plugin.
pub struct ChallengePlugin {
    plugin_step: PluginStep,
    /// Hot-reloadable challenge engine.
    engine: Arc<RwLock<ChallengeEngine>>,
    hash_value: String,
    enabled: bool,
    cookie_name: String,
    pow_difficulty: u32,
    /// Per-IP request counter used to feed `request_rate` to the engine.
    rates: DashMap<String, RateWindow>,
}

impl ChallengePlugin {
    /// Create a new plugin from configuration.
    pub fn new(params: &PluginConf) -> Result<Self> {
        debug!(params = params.to_string(), "new challenge plugin");
        Self::try_from(params)
    }

    /// Record a hit for `ip` and return its request count in the current
    /// 60-second window.
    fn record_rate(&self, ip: &str) -> u32 {
        let now = now_secs();
        let mut entry = self
            .rates
            .entry(ip.to_string())
            .or_insert(RateWindow {
                count: 0,
                window_start: now,
            });
        if now.saturating_sub(entry.window_start) >= RATE_WINDOW_SECS {
            entry.count = 0;
            entry.window_start = now;
        }
        entry.count = entry.count.saturating_add(1);
        let count = entry.count;
        drop(entry);
        // Opportunistic cleanup so the map does not grow unbounded.
        if self.rates.len() > 8192 {
            self.rates.retain(|_, w| {
                now.saturating_sub(w.window_start) < RATE_WINDOW_SECS * 2
            });
        }
        count
    }

    /// Handle a POST to the verify endpoint: validate the proof-of-work and,
    /// on success, hand back a clearance cookie plus a redirect to the
    /// originally requested URL.
    async fn handle_verify(
        &self,
        session: &mut Session,
    ) -> pingora::Result<RequestPluginResult> {
        let mut buf = BytesMut::with_capacity(4096);
        while let Some(chunk) = session.read_request_body().await? {
            buf.put(chunk.as_ref());
            if buf.len() > MAX_VERIFY_BODY {
                break;
            }
        }

        let value: serde_json::Value = match serde_json::from_slice(&buf) {
            Ok(v) => v,
            Err(_) => {
                return Ok(RequestPluginResult::Respond(block_page(
                    "-",
                    "malformed challenge submission",
                )));
            },
        };

        let request_id = value
            .get("request_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let solution = value
            .get("solution")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let fingerprint_json = value
            .get("fingerprint_json")
            .and_then(|v| v.as_str())
            .unwrap_or("{}")
            .to_string();
        let timestamp = value
            .get("timestamp")
            .and_then(|v| v.as_i64())
            .unwrap_or_else(now_secs);

        let Some(pending) = take_pending(&request_id) else {
            return Ok(RequestPluginResult::Respond(block_page(
                &request_id,
                "challenge expired or unknown, please retry",
            )));
        };

        let submission = ChallengeSubmission {
            request_id: request_id.clone(),
            challenge_nonce: pending.nonce,
            solution,
            fingerprint_json,
            timestamp,
            site_id: pending.site_id,
        };

        let result = {
            let engine =
                self.engine.read().unwrap_or_else(|e| e.into_inner());
            engine.verify_solution(&submission)
        };

        match result {
            VerifyResult::Success {
                cookie_value,
                cookie_attributes,
            } => {
                let set_cookie = format!(
                    "{}={}; {}",
                    self.cookie_name, cookie_value, cookie_attributes
                );
                let mut builder = HttpResponse::builder(StatusCode::OK).body(
                    format!(
                        "<html><body><script>window.location.href = \"{}\";\
</script></body></html>",
                        pending.original_url
                    ),
                );
                builder = builder.header(HTTP_HEADER_CONTENT_HTML.clone());
                if let Ok(hv) = HeaderValue::from_str(&set_cookie) {
                    builder = builder.header((header::SET_COOKIE, hv));
                }
                Ok(RequestPluginResult::Respond(builder.no_store().finish()))
            },
            VerifyResult::Failed { reason } => {
                warn!(request_id = %request_id, reason = %reason, "challenge verification failed");
                Ok(RequestPluginResult::Respond(block_page(
                    &request_id,
                    &reason,
                )))
            },
        }
    }
}

impl TryFrom<&PluginConf> for ChallengePlugin {
    type Error = Error;

    fn try_from(value: &PluginConf) -> Result<Self> {
        let hash_value = get_hash_key(value);
        let category = PluginCategory::Challenge.to_string();

        // A challenge plugin that is added is meant to be active; only an
        // explicit `enabled = false` disables it.
        let enabled = value
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let under_attack_mode = value
            .get("under_attack_mode")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let clearance_duration_secs =
            get_int_conf_or_default(value, "clearance_duration_secs", 1800);
        let rate_threshold =
            get_int_conf_or_default(value, "rate_threshold", 100) as u32;
        let pow_difficulty =
            get_int_conf_or_default(value, "pow_difficulty", 20) as u32;

        let mut cookie_secret = get_str_conf(value, "cookie_secret");
        if cookie_secret.is_empty() {
            cookie_secret = "change-me".to_string();
        }
        let mut cookie_name = get_str_conf(value, "cookie_name");
        if cookie_name.is_empty() {
            cookie_name = "__pingwaf_clearance".to_string();
        }
        let exempt_paths = get_str_slice_conf(value, "exempt_paths");
        let exempt_user_agents = get_str_slice_conf(value, "exempt_user_agents");

        let plugin_step = match super::get_step_conf_in(
            value,
            "challenge",
            PluginStep::EarlyRequest,
            &[PluginStep::EarlyRequest, PluginStep::Request],
        ) {
            Ok(step) => step,
            Err(e) => {
                return Err(Error::Invalid {
                    category,
                    message: e.to_string(),
                });
            },
        };

        let config = ChallengeConfig {
            enabled,
            under_attack_mode,
            default_level: ClearanceLevel::NonInteractive,
            clearance_duration_secs,
            exempt_paths,
            exempt_user_agents,
            rate_threshold,
            browser_integrity_check: value
                .get("browser_integrity_check")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            tls_fingerprint_check: false,
            cookie_secret,
            cookie_name: cookie_name.clone(),
            pow_difficulty,
            submission_max_age_secs: get_int_conf_or_default(
                value,
                "submission_max_age_secs",
                300,
            ),
        };

        Ok(Self {
            plugin_step,
            engine: Arc::new(RwLock::new(ChallengeEngine::new(config))),
            hash_value,
            enabled,
            cookie_name,
            pow_difficulty,
            rates: DashMap::new(),
        })
    }
}

#[async_trait]
impl Plugin for ChallengePlugin {
    #[inline]
    fn config_key(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.hash_value)
    }

    async fn handle_request(
        &self,
        step: PluginStep,
        session: &mut Session,
        ctx: &mut Ctx,
    ) -> pingora::Result<RequestPluginResult> {
        if step != self.plugin_step {
            return Ok(RequestPluginResult::Skipped);
        }
        if !self.enabled {
            return Ok(RequestPluginResult::Skipped);
        }

        let path = session.req_header().uri.path().to_string();
        let method = session.req_header().method.as_str().to_string();

        // The verify endpoint is always served, even if challenges would
        // otherwise be skipped for this path.
        if path == VERIFY_ENDPOINT && method == "POST" {
            return self.handle_verify(session).await;
        }

        let client_ip = ensure_client_ip(session, ctx).to_string();
        let user_agent =
            get_req_header_value(session.req_header(), "user-agent")
                .unwrap_or_default()
                .to_string();

        // A valid clearance cookie short-circuits everything.
        if let Some(cookie) = get_cookie_value(session.req_header(), &self.cookie_name)
        {
            let engine =
                self.engine.read().unwrap_or_else(|e| e.into_inner());
            if engine.validate_clearance(cookie).is_some() {
                return Ok(RequestPluginResult::Continue);
            }
        }

        let request_rate = self.record_rate(&client_ip);
        let challenge_request = ChallengeRequest {
            path,
            method,
            client_ip,
            user_agent,
            cookies: Vec::new(),
            has_js_support: None,
            request_rate,
        };

        let decision = {
            let engine =
                self.engine.read().unwrap_or_else(|e| e.into_inner());
            engine.should_challenge(&challenge_request)
        };

        if decision == ChallengeDecision::Pass {
            return Ok(RequestPluginResult::Continue);
        }
        if decision == ChallengeDecision::Block {
            let request_id = ctx
                .state
                .request_id
                .clone()
                .unwrap_or_else(generate_request_id);
            return Ok(RequestPluginResult::Respond(block_page(
                &request_id,
                "blocked by challenge policy",
            )));
        }

        // Any challenge flavour needs the original URL and a request id.
        let uri = &session.req_header().uri;
        let original_url = match uri.query() {
            Some(q) => format!("{}?{}", uri.path(), q),
            None => uri.path().to_string(),
        };
        let site_id = get_host(session.req_header())
            .unwrap_or_default()
            .to_string();
        let request_id = ctx
            .state
            .request_id
            .clone()
            .unwrap_or_else(generate_request_id);

        let kind = match decision {
            ChallengeDecision::ManagedChallenge => ChallengeKind::Managed,
            ChallengeDecision::InteractiveChallenge => {
                ChallengeKind::Interactive
            },
            _ => ChallengeKind::Js,
        };

        Ok(RequestPluginResult::Respond(build_challenge_response(
            &request_id,
            &original_url,
            &site_id,
            self.pow_difficulty,
            kind,
        )))
    }
}

register_plugin!("challenge", ChallengePlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use pingap_core::PluginStep;
    use pingora::proxy::Session;
    use tokio_test::io::Builder;

    #[test]
    fn test_challenge_params() {
        let plugin = ChallengePlugin::new(
            &toml::from_str::<PluginConf>(
                r###"
enabled = true
under_attack_mode = true
clearance_duration_secs = 600
rate_threshold = 50
pow_difficulty = 0
cookie_secret = "test-secret"
cookie_name = "__test_clearance"
exempt_paths = ["/health"]
"###,
            )
            .unwrap(),
        )
        .unwrap();
        assert!(plugin.enabled);
        assert_eq!("__test_clearance", plugin.cookie_name);
        assert_eq!(0, plugin.pow_difficulty);
        assert_eq!(PluginStep::EarlyRequest, plugin.plugin_step);
    }

    #[tokio::test]
    async fn test_under_attack_challenges() {
        let plugin = ChallengePlugin::new(
            &toml::from_str::<PluginConf>(
                r###"
under_attack_mode = true
pow_difficulty = 0
cookie_secret = "test-secret"
"###,
            )
            .unwrap(),
        )
        .unwrap();

        let input_header =
            "GET /dashboard HTTP/1.1\r\nHost: example.com\r\n\r\n";
        let mock_io = Builder::new().read(input_header.as_bytes()).build();
        let mut session = Session::new_h1(Box::new(mock_io));
        session.read_request().await.unwrap();

        let result = plugin
            .handle_request(
                PluginStep::EarlyRequest,
                &mut session,
                &mut Ctx::default(),
            )
            .await
            .unwrap();
        let RequestPluginResult::Respond(resp) = result else {
            panic!("expected a challenge response");
        };
        assert_eq!(StatusCode::SERVICE_UNAVAILABLE, resp.status);
    }

    #[tokio::test]
    async fn test_exempt_path_passes() {
        let plugin = ChallengePlugin::new(
            &toml::from_str::<PluginConf>(
                r###"
under_attack_mode = true
exempt_paths = ["/health"]
cookie_secret = "test-secret"
"###,
            )
            .unwrap(),
        )
        .unwrap();

        let input_header = "GET /health HTTP/1.1\r\nHost: example.com\r\n\r\n";
        let mock_io = Builder::new().read(input_header.as_bytes()).build();
        let mut session = Session::new_h1(Box::new(mock_io));
        session.read_request().await.unwrap();

        let result = plugin
            .handle_request(
                PluginStep::EarlyRequest,
                &mut session,
                &mut Ctx::default(),
            )
            .await
            .unwrap();
        assert_eq!(true, result == RequestPluginResult::Continue);
    }
}
