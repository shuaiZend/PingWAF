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

//! WAF plugin — inspects every request against the PingWAF detection engine.
//!
//! Runs at [`PluginStep::EarlyRequest`], ahead of most other plugins. Rules are
//! resolved per request:
//! * When a [`PingWafAgent`] control-plane instance is running, site rules for
//!   the request's domain are used (cached per-domain and rebuilt when the
//!   agent's config hash changes).
//! * Otherwise (standalone pingap) the locally configured engine is used.
//!
//! Verdicts map onto the proxy pipeline: `Pass`/`Monitor` continue, `Block`
//! returns 403, and `Challenge` delegates to the challenge subsystem.

use super::{
    Error, get_bool_conf, get_hash_key, get_int_conf_or_default, get_str_conf,
    get_str_slice_conf,
};
use crate::challenge::{ChallengeKind, block_page, build_challenge_response};
use async_trait::async_trait;
use bytes::{BufMut, BytesMut};
use dashmap::DashMap;
use pingap_config::{PluginCategory, PluginConf};
use pingap_core::{
    Ctx, HttpResponse, Plugin, PluginStep, RequestPluginResult,
    ensure_client_ip, get_host,
};
use pingora::proxy::Session;
use pingwaf_agent::cache::{
    WafAction as CacheWafAction, WafConfig as CacheWafConfig,
    WafMode as CacheWafMode,
};
use pingwaf_agent::{PingWafAgent, SecurityEvent};
use pingwaf_challenge::generate_request_id;
use pingwaf_waf::{
    CompiledRule, RequestData, RuleAction, WafAction, WafEngine,
    WafEngineConfig, WafMode, WafVerdict,
};
use std::borrow::Cow;
use std::sync::{Arc, RwLock};
use tracing::debug;

type Result<T, E = Error> = std::result::Result<T, E>;

/// Which engine handles the current request.
enum EngineChoice {
    /// Locally configured engine (standalone mode).
    Base,
    /// Per-domain engine built from agent-supplied site rules.
    Site(Arc<WafEngine>),
    /// WAF explicitly disabled for this site by the control plane.
    Disabled,
}

/// A cached per-domain engine plus the fingerprint it was built from.
struct CachedEngine {
    fingerprint: String,
    engine: Arc<WafEngine>,
}

/// Web Application Firewall plugin.
pub struct WafPlugin {
    plugin_step: PluginStep,
    /// Locally configured engine; shared and hot-reloadable.
    engine: Arc<RwLock<WafEngine>>,
    mode: WafMode,
    paranoia_level: u8,
    anomaly_threshold: u32,
    detections: Vec<String>,
    ml_enabled: bool,
    ml_threshold: f64,
    /// Opt-in request-body inspection (kept off by default: reading the body
    /// on every request is expensive and interferes with streaming uploads).
    inspect_body: bool,
    max_body_size: usize,
    /// Proof-of-work difficulty used when delegating a `Challenge` verdict.
    pow_difficulty: u32,
    hash_value: String,
    /// Per-domain engines built from agent rules, keyed by host.
    site_engines: DashMap<String, CachedEngine>,
}

fn parse_mode(value: &str) -> WafMode {
    match value.to_lowercase().as_str() {
        "off" => WafMode::Off,
        "monitor" => WafMode::Monitor,
        "block" => WafMode::Block,
        _ => WafMode::Block,
    }
}

fn action_str(action: &WafAction) -> &'static str {
    match action {
        WafAction::Pass => "pass",
        WafAction::Monitor => "monitor",
        WafAction::Block => "block",
        WafAction::Challenge => "challenge",
    }
}

/// Build a [`WafEngine`] from control-plane site WAF config.
fn build_site_engine(cfg: &CacheWafConfig) -> WafEngine {
    let mode = match cfg.mode {
        CacheWafMode::Off => WafMode::Off,
        CacheWafMode::Monitor => WafMode::Monitor,
        CacheWafMode::Block => WafMode::Block,
    };
    let rules: Vec<CompiledRule> = cfg
        .custom_rules
        .iter()
        .filter(|r| r.enabled)
        .filter_map(|r| {
            let action = match r.action {
                CacheWafAction::Block => RuleAction::Block,
                CacheWafAction::Log => RuleAction::Log,
                CacheWafAction::Challenge => RuleAction::Challenge,
                CacheWafAction::JsChallenge => RuleAction::JsChallenge,
                CacheWafAction::Allow => RuleAction::Allow,
            };
            CompiledRule::compile(
                r.id.clone(),
                r.name.clone(),
                &r.expression,
                action,
                r.severity as u8,
                r.tags.clone(),
            )
            .ok()
        })
        .collect();

    let engine_config = WafEngineConfig {
        mode,
        threshold: if cfg.anomaly_threshold > 0 {
            cfg.anomaly_threshold
        } else {
            40
        },
        paranoia_level: (cfg.paranoia_level as u8).clamp(1, 4),
        max_decode_layers: 3,
        rules,
        enable_managed_rules: true,
        fast_path_block_on_critical: true,
    };
    WafEngine::new(&engine_config)
}

impl WafPlugin {
    /// Create a new plugin from configuration.
    pub fn new(params: &PluginConf) -> Result<Self> {
        debug!(params = params.to_string(), "new waf plugin");
        Self::try_from(params)
    }

    /// Resolve which engine to use for `host`, consulting the agent rule cache
    /// when available and caching per-domain engines.
    fn resolve_engine(&self, host: &str) -> (EngineChoice, String) {
        let Some(agent) = PingWafAgent::instance() else {
            return (EngineChoice::Base, host.to_string());
        };
        if host.is_empty() {
            return (EngineChoice::Base, String::new());
        }
        let Some(site_rules) = agent.get_rules_for_domain(host) else {
            return (EngineChoice::Base, host.to_string());
        };
        let site_id = site_rules.site_id.clone();
        let Some(waf_cfg) = site_rules.waf_config.as_ref() else {
            return (EngineChoice::Base, site_id);
        };
        if !waf_cfg.enabled {
            return (EngineChoice::Disabled, site_id);
        }

        let fingerprint = agent.config_hash();
        if let Some(cached) = self.site_engines.get(host)
            && cached.fingerprint == fingerprint
        {
            return (EngineChoice::Site(cached.engine.clone()), site_id);
        }

        let engine = Arc::new(build_site_engine(waf_cfg));
        self.site_engines.insert(
            host.to_string(),
            CachedEngine {
                fingerprint,
                engine: Arc::clone(&engine),
            },
        );
        (EngineChoice::Site(engine), site_id)
    }

    /// Ship a security event to the control plane (best effort, non-blocking).
    fn log_event(
        site_id: &str,
        request_id: &str,
        host: &str,
        request_data: &RequestData,
        verdict: &WafVerdict,
    ) {
        let Some(agent) = PingWafAgent::instance() else {
            return;
        };
        let blocked = matches!(
            verdict.action,
            WafAction::Block | WafAction::Challenge
        );
        agent.record_request(blocked);
        let user_agent = request_data
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        agent.log_security_event(SecurityEvent {
            site_id: site_id.to_string(),
            request_id: request_id.to_string(),
            client_ip: request_data.client_ip.clone(),
            method: request_data.method.clone(),
            host: host.to_string(),
            path: request_data.path.clone(),
            query_string: request_data.query.clone(),
            rule_id: verdict
                .matched_rules
                .first()
                .cloned()
                .unwrap_or_default(),
            rule_name: String::new(),
            action: action_str(&verdict.action).to_string(),
            score: verdict.score as u32,
            details: verdict.details.clone(),
            user_agent,
            country_code: request_data
                .country_code
                .clone()
                .unwrap_or_default(),
            matched_tags: verdict.matched_rules.clone(),
        });
    }
}

impl TryFrom<&PluginConf> for WafPlugin {
    type Error = Error;

    fn try_from(value: &PluginConf) -> Result<Self> {
        let hash_value = get_hash_key(value);
        let category = PluginCategory::Waf.to_string();

        let mode = parse_mode(&get_str_conf(value, "mode"));
        let paranoia_level =
            (get_int_conf_or_default(value, "paranoia_level", 2) as u8)
                .clamp(1, 4);
        let anomaly_threshold =
            get_int_conf_or_default(value, "anomaly_threshold", 40) as u32;
        let detections = get_str_slice_conf(value, "detections");
        let ml_enabled = get_bool_conf(value, "ml_enabled");
        let ml_threshold = value
            .get("ml_threshold")
            .and_then(|v| v.as_float())
            .unwrap_or(0.5);
        let inspect_body = get_bool_conf(value, "inspect_body");
        let max_body_size =
            get_int_conf_or_default(value, "max_body_size", 64 * 1024) as usize;
        let pow_difficulty =
            get_int_conf_or_default(value, "pow_difficulty", 20) as u32;

        let plugin_step = match super::get_step_conf_in(
            value,
            "waf",
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

        let engine_config = WafEngineConfig {
            mode,
            threshold: anomaly_threshold,
            paranoia_level,
            max_decode_layers: 3,
            rules: Vec::new(),
            enable_managed_rules: true,
            fast_path_block_on_critical: true,
        };

        Ok(Self {
            plugin_step,
            engine: Arc::new(RwLock::new(WafEngine::new(&engine_config))),
            mode,
            paranoia_level,
            anomaly_threshold,
            detections,
            ml_enabled,
            ml_threshold,
            inspect_body,
            max_body_size,
            pow_difficulty,
            hash_value,
            site_engines: DashMap::new(),
        })
    }
}

#[async_trait]
impl Plugin for WafPlugin {
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

        // ── Extract immutable request facts (all borrowed data is cloned) ──
        let req_header = session.req_header();
        let method = req_header.method.as_str().to_string();
        let path = req_header.uri.path().to_string();
        let query = req_header.uri.query().unwrap_or_default().to_string();
        let mut headers: Vec<(String, String)> = Vec::new();
        for (name, value) in req_header.headers.iter() {
            if let Ok(v) = value.to_str() {
                headers.push((name.as_str().to_string(), v.to_string()));
            }
        }
        let host = get_host(req_header).unwrap_or_default().to_string();
        let scheme = if ctx.conn.tls_version.is_some() {
            "https".to_string()
        } else {
            "http".to_string()
        };

        // ── Resolve the engine for this domain ──
        let (choice, site_id) = self.resolve_engine(&host);
        if matches!(choice, EngineChoice::Disabled) {
            return Ok(RequestPluginResult::Continue);
        }
        if matches!(choice, EngineChoice::Base) && self.mode == WafMode::Off {
            return Ok(RequestPluginResult::Skipped);
        }

        let client_ip = ensure_client_ip(session, ctx).to_string();

        // ── Optional request-body inspection ──
        let body = if self.inspect_body {
            let mut buf = BytesMut::with_capacity(4096);
            while let Some(chunk) = session.read_request_body().await? {
                buf.put(chunk.as_ref());
                if buf.len() >= self.max_body_size {
                    break;
                }
            }
            if buf.is_empty() {
                None
            } else {
                Some(buf.to_vec())
            }
        } else {
            None
        };

        let request_data = RequestData {
            method,
            path: path.clone(),
            query: query.clone(),
            headers,
            body,
            client_ip,
            country_code: None,
            scheme,
        };

        // ── Inspect ──
        let verdict = match &choice {
            EngineChoice::Site(engine) => engine.inspect(&request_data),
            EngineChoice::Base => {
                let guard =
                    self.engine.read().unwrap_or_else(|e| e.into_inner());
                guard.inspect(&request_data)
            },
            EngineChoice::Disabled => unreachable!("handled above"),
        };

        if verdict.action == WafAction::Pass {
            return Ok(RequestPluginResult::Continue);
        }

        let request_id = ctx
            .state
            .request_id
            .clone()
            .unwrap_or_else(generate_request_id);

        // Monitor: log the event but let the request through.
        if verdict.action == WafAction::Monitor {
            Self::log_event(
                &site_id,
                &request_id,
                &host,
                &request_data,
                &verdict,
            );
            return Ok(RequestPluginResult::Continue);
        }

        // Block / Challenge: log, then respond.
        Self::log_event(&site_id, &request_id, &host, &request_data, &verdict);

        if verdict.action == WafAction::Block {
            return Ok(RequestPluginResult::Respond(block_page(
                &request_id,
                &verdict.details,
            )));
        }

        // Challenge verdict — delegate to the challenge subsystem.
        let original_url = if query.is_empty() {
            path
        } else {
            format!("{path}?{query}")
        };
        Ok(RequestPluginResult::Respond(build_challenge_response(
            &request_id,
            &original_url,
            &site_id,
            self.pow_difficulty,
            ChallengeKind::Js,
        )))
    }
}

register_plugin!("waf", WafPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use pingap_core::PluginStep;
    use pingora::proxy::Session;
    use tokio_test::io::Builder;

    #[test]
    fn test_waf_params() {
        let plugin = WafPlugin::new(
            &toml::from_str::<PluginConf>(
                r###"
mode = "block"
paranoia_level = 3
anomaly_threshold = 30
detections = ["sqli", "xss", "rce", "lfi", "ssrf"]
ml_enabled = true
ml_threshold = 0.75
"###,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(WafMode::Block, plugin.mode);
        assert_eq!(3, plugin.paranoia_level);
        assert_eq!(30, plugin.anomaly_threshold);
        assert!(plugin.ml_enabled);
        assert_eq!(5, plugin.detections.len());
        assert_eq!(PluginStep::EarlyRequest, plugin.plugin_step);
    }

    #[tokio::test]
    async fn test_blocks_sqli() {
        let plugin = WafPlugin::new(
            &toml::from_str::<PluginConf>(r###"mode = "block""###).unwrap(),
        )
        .unwrap();

        let input_header =
            "GET /api/users?id=1'%20OR%201=1%20-- HTTP/1.1\r\nHost: example.com\r\n\r\n";
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
            panic!("expected a block response");
        };
        assert_eq!(http::StatusCode::FORBIDDEN, resp.status);
    }

    #[tokio::test]
    async fn test_passes_clean_request() {
        let plugin = WafPlugin::new(
            &toml::from_str::<PluginConf>(r###"mode = "block""###).unwrap(),
        )
        .unwrap();

        let input_header =
            "GET /api/users?page=1 HTTP/1.1\r\nHost: example.com\r\n\r\n";
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

    #[tokio::test]
    async fn test_monitor_mode_does_not_block() {
        let plugin = WafPlugin::new(
            &toml::from_str::<PluginConf>(r###"mode = "monitor""###).unwrap(),
        )
        .unwrap();

        let input_header =
            "GET /api/users?id=1'%20OR%201=1%20-- HTTP/1.1\r\nHost: example.com\r\n\r\n";
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

    #[tokio::test]
    async fn test_off_mode_skips() {
        let plugin = WafPlugin::new(
            &toml::from_str::<PluginConf>(r###"mode = "off""###).unwrap(),
        )
        .unwrap();

        let input_header =
            "GET /api/users?id=1'%20OR%201=1%20-- HTTP/1.1\r\nHost: example.com\r\n\r\n";
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
        assert_eq!(true, result == RequestPluginResult::Skipped);
    }
}
