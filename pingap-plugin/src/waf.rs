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
    Ctx, HTTP_HEADER_NAME_X_REQUEST_ID, Plugin, PluginStep,
    RequestPluginResult, ResponsePluginResult, ensure_client_ip, get_host,
};
use pingora::http::ResponseHeader;
use pingora::proxy::Session;
use pingwaf_agent::cache::{
    WafAction as CacheWafAction, WafConfig as CacheWafConfig,
    WafMode as CacheWafMode,
};
use pingwaf_agent::{AccessLogEntry, PingWafAgent, SecurityEvent};
use pingwaf_challenge::generate_request_id;
use pingwaf_waf::{
    CompiledRule, RequestData, RuleAction, WafAction, WafEngine,
    WafEngineConfig, WafMode, WafVerdict,
};
use std::borrow::Cow;
use std::sync::{Arc, LazyLock, RwLock};
use std::time::{Duration, Instant};
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

// ─────────────────────────────────────────────────────────────
// Pending access log store
// ─────────────────────────────────────────────────────────────

/// Request facts captured before inspection and held until the upstream
/// response allows the access log to be emitted.
struct PendingAccess {
    site_id: String,
    client_ip: String,
    method: String,
    scheme: String,
    protocol: String,
    host: String,
    path: String,
    query: String,
    user_agent: String,
    referer: String,
    tls_version: String,
    start: Instant,
}

/// Orphaned entries (e.g. upstream connect failures that bypass
/// `handle_response`) are evicted once they outlive this window.
const PENDING_ACCESS_TTL: Duration = Duration::from_secs(60);

/// Only pay for garbage collection above this many outstanding entries.
const PENDING_ACCESS_GC_THRESHOLD: usize = 65536;

static PENDING_ACCESS: LazyLock<DashMap<String, PendingAccess>> =
    LazyLock::new(DashMap::new);

fn register_pending_access(request_id: &str, pending: PendingAccess) {
    if PENDING_ACCESS.len() > PENDING_ACCESS_GC_THRESHOLD {
        PENDING_ACCESS.retain(|_, v| v.start.elapsed() < PENDING_ACCESS_TTL);
    }
    PENDING_ACCESS.insert(request_id.to_string(), pending);
}

/// Emit an access log entry, either at response time or immediately for
/// plugin-generated responses (which never reach `handle_response`).
fn emit_access(
    agent: &PingWafAgent,
    request_id: &str,
    pending: PendingAccess,
    status_code: u32,
    upstream_addr: String,
    upstream_latency_ms: u64,
) {
    let total_latency_ms = pending.start.elapsed().as_millis() as u64;
    agent.log_access(AccessLogEntry {
        site_id: pending.site_id,
        request_id: request_id.to_string(),
        client_ip: pending.client_ip,
        method: pending.method,
        scheme: pending.scheme,
        host: pending.host,
        path: pending.path,
        query_string: pending.query,
        protocol: pending.protocol,
        status_code,
        response_size: 0,
        upstream_addr,
        upstream_latency_ms,
        total_latency_ms,
        cache_status: String::new(),
        user_agent: pending.user_agent,
        referer: pending.referer,
        tls_version: pending.tls_version,
        country_code: String::new(),
        request_body: None,
        request_body_truncated: false,
    });
}

/// Web Application Firewall plugin.
pub struct WafPlugin {
    plugin_step: PluginStep,
    /// Locally configured engine; shared and hot-reloadable.
    engine: Arc<RwLock<WafEngine>>,
    mode: WafMode,
    // TODO: parsed from config but not yet wired into WafEngine (detections/
    // ml_enabled/ml_threshold are currently ignored at runtime)
    #[allow(dead_code)]
    paranoia_level: u8,
    #[allow(dead_code)]
    anomaly_threshold: u32,
    #[allow(dead_code)]
    detections: Vec<String>,
    #[allow(dead_code)]
    ml_enabled: bool,
    #[allow(dead_code)]
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
        let blocked =
            matches!(verdict.action, WafAction::Block | WafAction::Challenge);
        agent.record_request(blocked);
        let user_agent = request_data
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        let response_status = match verdict.action {
            WafAction::Block => 403,
            WafAction::Challenge => 503,
            _ => 0,
        };
        agent.log_security_event(SecurityEvent {
            site_id: site_id.to_string(),
            request_id: request_id.to_string(),
            client_ip: request_data.client_ip.clone(),
            method: request_data.method.clone(),
            scheme: request_data.scheme.clone(),
            protocol: request_data.protocol.clone(),
            host: host.to_string(),
            path: request_data.path.clone(),
            query_string: request_data.query.clone(),
            rule_id: verdict.matched_rules.first().cloned().unwrap_or_default(),
            rule_name: String::new(),
            action: action_str(&verdict.action).to_string(),
            score: verdict.score as u32,
            details: verdict.details.clone(),
            response_status,
            user_agent,
            country_code: request_data.country_code.clone().unwrap_or_default(),
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
        let mut user_agent = String::new();
        let mut referer = String::new();
        for (name, value) in req_header.headers.iter() {
            let Ok(v) = value.to_str() else {
                continue;
            };
            match name.as_str() {
                "user-agent" => user_agent = v.to_string(),
                "referer" => referer = v.to_string(),
                _ => {},
            }
            headers.push((name.as_str().to_string(), v.to_string()));
        }
        let host = get_host(req_header).unwrap_or_default().to_string();
        let scheme = if ctx.conn.tls_version.is_some() {
            "https".to_string()
        } else {
            "http".to_string()
        };
        let protocol = format!("{:?}", req_header.version);
        let client_ip = ensure_client_ip(session, ctx).to_string();

        // Get or create the request id early so the access log, security
        // events and the X-Request-ID header all share one stable value.
        let request_id = match ctx.state.request_id.clone() {
            Some(id) => id,
            None => {
                let id = generate_request_id();
                ctx.state.request_id = Some(id.clone());
                let _ = session
                    .req_header_mut()
                    .insert_header(&HTTP_HEADER_NAME_X_REQUEST_ID, &id);
                id
            },
        };

        // ── Resolve the engine for this domain ──
        let (choice, site_id) = self.resolve_engine(&host);

        // Track the request for access logging until the response phase —
        // including WAF-disabled sites, so traffic data stays complete.
        let agent = PingWafAgent::instance();
        if agent.is_some() {
            register_pending_access(
                &request_id,
                PendingAccess {
                    site_id: site_id.clone(),
                    client_ip: client_ip.clone(),
                    method: method.clone(),
                    scheme: scheme.clone(),
                    protocol: protocol.clone(),
                    host: host.clone(),
                    path: path.clone(),
                    query: query.clone(),
                    user_agent,
                    referer,
                    tls_version: ctx
                        .conn
                        .tls_version
                        .clone()
                        .unwrap_or_default()
                        .to_string(),
                    start: Instant::now(),
                },
            );
        }

        if matches!(choice, EngineChoice::Disabled) {
            if let Some(agent) = &agent {
                agent.record_request(false);
            }
            return Ok(RequestPluginResult::Continue);
        }
        if matches!(choice, EngineChoice::Base) && self.mode == WafMode::Off {
            if let Some(agent) = &agent {
                agent.record_request(false);
            }
            return Ok(RequestPluginResult::Skipped);
        }

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
            protocol,
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
            if let Some(agent) = &agent {
                agent.record_request(false);
            }
            return Ok(RequestPluginResult::Continue);
        }

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

        // Block / Challenge: log the security event, then emit the access
        // entry with the real status — a Respond result never reaches
        // handle_response, so this is the only chance to log it.
        Self::log_event(&site_id, &request_id, &host, &request_data, &verdict);
        if let Some(agent) = &agent
            && let Some((_, pending)) = PENDING_ACCESS.remove(&request_id)
        {
            let status = if verdict.action == WafAction::Block {
                403
            } else {
                503
            };
            emit_access(agent, &request_id, pending, status, String::new(), 0);
        }

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

    async fn handle_response(
        &self,
        _session: &mut Session,
        ctx: &mut Ctx,
        upstream_response: &mut ResponseHeader,
    ) -> pingora::Result<ResponsePluginResult> {
        let Some(request_id) = ctx.state.request_id.clone() else {
            return Ok(ResponsePluginResult::Unchanged);
        };
        // Without an agent nothing was registered at request time.
        let Some(agent) = PingWafAgent::instance() else {
            return Ok(ResponsePluginResult::Unchanged);
        };
        let Some((_, pending)) = PENDING_ACCESS.remove(&request_id) else {
            return Ok(ResponsePluginResult::Unchanged);
        };
        let upstream_latency_ms =
            ctx.timing.upstream_processing.unwrap_or(0).max(0) as u64;
        emit_access(
            &agent,
            &request_id,
            pending,
            upstream_response.status.as_u16() as u32,
            ctx.upstream.address.clone(),
            upstream_latency_ms,
        );
        Ok(ResponsePluginResult::Unchanged)
    }
}

register_plugin!("waf", WafPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use pingap_core::PluginStep;
    use pingora::proxy::Session;
    use pingwaf_agent::cache::RuleCache;
    use pingwaf_agent::client::ControlPlaneClient;
    use pingwaf_agent::config::AgentConfig;
    use pingwaf_agent::heartbeat::MetricsCollector;
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
        let _agent_lock = lock_agent().await;
        // Start from no instance: a leftover agent from a sibling test
        // would otherwise observe (and count) this request.
        PingWafAgent::set_agent_instance(None);
        let plugin = WafPlugin::new(
            &toml::from_str::<PluginConf>(r###"mode = "block""###).unwrap(),
        )
        .unwrap();

        let input_header = "GET /api/users?id=1'%20OR%201=1%20-- HTTP/1.1\r\nHost: example.com\r\n\r\n";
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
        let _agent_lock = lock_agent().await;
        PingWafAgent::set_agent_instance(None);
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
        let _agent_lock = lock_agent().await;
        PingWafAgent::set_agent_instance(None);
        let plugin = WafPlugin::new(
            &toml::from_str::<PluginConf>(r###"mode = "monitor""###).unwrap(),
        )
        .unwrap();

        let input_header = "GET /api/users?id=1'%20OR%201=1%20-- HTTP/1.1\r\nHost: example.com\r\n\r\n";
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
        let _agent_lock = lock_agent().await;
        PingWafAgent::set_agent_instance(None);
        let plugin = WafPlugin::new(
            &toml::from_str::<PluginConf>(r###"mode = "off""###).unwrap(),
        )
        .unwrap();

        let input_header = "GET /api/users?id=1'%20OR%201=1%20-- HTTP/1.1\r\nHost: example.com\r\n\r\n";
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

    /// The agent instance is a process-wide global, and cargo runs a
    /// binary's tests in parallel: any test reaching the plugin's agent
    /// paths while another has an agent installed records into that
    /// agent's metrics collector. Tests touching the instance therefore
    /// serialize on this lock.
    static AGENT_LOCK: tokio::sync::Mutex<()> =
        tokio::sync::Mutex::const_new(());

    async fn lock_agent() -> tokio::sync::MutexGuard<'static, ()> {
        AGENT_LOCK.lock().await
    }

    /// Install a never-connecting agent for the duration of one test,
    /// holding the agent lock so sibling tests stay off the global.
    async fn install_test_agent() -> (
        tokio::sync::MutexGuard<'static, ()>,
        Arc<PingWafAgent>,
        tempfile::TempDir,
    ) {
        let lock = lock_agent().await;
        let dir = tempfile::tempdir().unwrap();
        let config = AgentConfig {
            cache_dir: dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        let rule_cache =
            RuleCache::new(dir.path().to_path_buf(), "test-agent".to_string())
                .unwrap();
        let metrics = Arc::new(MetricsCollector::new());
        let client = Arc::new(ControlPlaneClient::new(
            config.clone(),
            Arc::clone(&rule_cache),
            Arc::clone(&metrics),
        ));
        let agent = Arc::new(PingWafAgent {
            config,
            client,
            rule_cache,
            metrics,
        });
        PingWafAgent::set_agent_instance(Some(Arc::clone(&agent)));
        (lock, agent, dir)
    }

    #[tokio::test]
    async fn test_access_log_pipeline() {
        let (_guard, agent, _dir) = install_test_agent().await;
        let plugin = WafPlugin::new(
            &toml::from_str::<PluginConf>(r###"mode = "block""###).unwrap(),
        )
        .unwrap();

        // ── Blocked request: security event + immediate access entry ──
        let input_header = "GET /api/users?id=1'%20OR%201=1%20-- HTTP/1.1\r\nHost: example.com\r\n\r\n";
        let mock_io = Builder::new().read(input_header.as_bytes()).build();
        let mut session = Session::new_h1(Box::new(mock_io));
        session.read_request().await.unwrap();
        let mut ctx = Ctx::default();
        let result = plugin
            .handle_request(PluginStep::EarlyRequest, &mut session, &mut ctx)
            .await
            .unwrap();
        let RequestPluginResult::Respond(resp) = result else {
            panic!("expected a block response");
        };
        assert_eq!(http::StatusCode::FORBIDDEN, resp.status);
        assert_eq!(1, agent.metrics.requests_total());
        assert_eq!(1, agent.metrics.blocked_requests_total());

        // First shipped entry is the security event with the real scheme,
        // protocol and status (no more hardcoded https/HTTP/1.1/403 trio).
        let event = agent.client.pop_log().await.unwrap();
        assert_eq!("block", event.waf_action);
        assert_eq!(403, event.response_status);
        assert_eq!("http", event.scheme);
        assert_eq!("HTTP/1.1", event.protocol);

        // Second is the access entry, emitted at request time because a
        // Respond result never reaches handle_response.
        let blocked_id = event.request_id.clone();
        let entry = agent.client.pop_log().await.unwrap();
        assert_eq!(blocked_id, entry.request_id);
        assert_eq!(403, entry.response_status);
        assert_eq!("GET", entry.method);
        assert_eq!("/api/users", entry.path);
        assert_eq!("example.com", entry.host);
        assert_eq!("http", entry.scheme);
        assert_eq!("HTTP/1.1", entry.protocol);

        // The pending entry was consumed: handle_response must not re-send.
        let mut resp = ResponseHeader::build(200, None).unwrap();
        plugin
            .handle_response(&mut session, &mut ctx, &mut resp)
            .await
            .unwrap();
        assert!(agent.client.pop_log().await.is_none());

        // ── Passed request: metrics counted, access logged at response ──
        let input_header =
            "GET /api/users?page=1 HTTP/1.1\r\nHost: example.com\r\n\r\n";
        let mock_io = Builder::new().read(input_header.as_bytes()).build();
        let mut session = Session::new_h1(Box::new(mock_io));
        session.read_request().await.unwrap();
        let mut ctx = Ctx::default();
        let result = plugin
            .handle_request(PluginStep::EarlyRequest, &mut session, &mut ctx)
            .await
            .unwrap();
        assert_eq!(true, result == RequestPluginResult::Continue);
        assert_eq!(2, agent.metrics.requests_total());
        assert_eq!(1, agent.metrics.blocked_requests_total());

        // Nothing ships until the response arrives.
        assert!(agent.client.pop_log().await.is_none());

        let request_id = ctx.state.request_id.clone().unwrap();
        let mut resp = ResponseHeader::build(200, None).unwrap();
        plugin
            .handle_response(&mut session, &mut ctx, &mut resp)
            .await
            .unwrap();
        let entry = agent.client.pop_log().await.unwrap();
        assert_eq!(request_id, entry.request_id);
        assert_eq!(200, entry.response_status);
        assert_eq!("page=1", entry.query_string);
        // Consumed exactly once.
        assert!(agent.client.pop_log().await.is_none());
    }
}
