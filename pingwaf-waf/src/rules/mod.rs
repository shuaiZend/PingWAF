//! Rule definitions and matching.
//!
//! Three layers:
//! - [`expression`] — Cloudflare-style expression AST plus a recursive-descent
//!   parser and evaluator.
//! - [`signatures`] — Aho-Corasick automaton over a built-in pattern table,
//!   plus libinjection-style SQLi / XSS detectors.
//! - [`managed`] — Default rule sets shipped with the engine (OWASP-flavoured).

pub mod expression;
pub mod managed;
pub mod signatures;

pub use expression::{
    EvalContext, Expression, Field, FieldKind, Operator, Value, evaluate, parse_expression,
};
pub use managed::default_managed_rules;
pub use signatures::{
    AttackCategory, SignatureEngine, SignatureHit, SignaturePattern, detect_sqli, detect_xss,
};

use serde::{Deserialize, Serialize};

/// What the engine should do when a rule matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleAction {
    Block,
    Log,
    Challenge,
    JsChallenge,
    Allow,
}

impl RuleAction {
    pub fn as_str(self) -> &'static str {
        match self {
            RuleAction::Block => "block",
            RuleAction::Log => "log",
            RuleAction::Challenge => "challenge",
            RuleAction::JsChallenge => "js_challenge",
            RuleAction::Allow => "allow",
        }
    }
}

/// A user- or control-plane-supplied rule, parsed once and reused per request.
#[derive(Debug, Clone)]
pub struct CompiledRule {
    pub id: String,
    pub name: String,
    pub expression: Expression,
    pub action: RuleAction,
    /// 1 (info) … 5 (critical). Drives the anomaly-score contribution.
    pub severity: u8,
    pub tags: Vec<String>,
    pub enabled: bool,
    /// Minimum paranoia level (1-4) at which the rule is active.
    pub paranoia_level: u8,
}

impl CompiledRule {
    /// Build a rule from its raw expression string. Returns an error if the
    /// expression fails to parse.
    pub fn compile(
        id: impl Into<String>,
        name: impl Into<String>,
        expression: &str,
        action: RuleAction,
        severity: u8,
        tags: Vec<String>,
    ) -> anyhow::Result<Self> {
        let parsed = parse_expression(expression)?;
        Ok(Self {
            id: id.into(),
            name: name.into(),
            expression: parsed,
            action,
            severity: severity.clamp(1, 5),
            tags,
            enabled: true,
            paranoia_level: 1,
        })
    }
}

/// Serializable form for transport between control plane and agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleSpec {
    pub id: String,
    pub name: String,
    pub expression: String,
    pub action: RuleAction,
    #[serde(default = "default_severity")]
    pub severity: u8,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_paranoia")]
    pub paranoia_level: u8,
}

fn default_severity() -> u8 {
    3
}
fn default_true() -> bool {
    true
}
fn default_paranoia() -> u8 {
    1
}

impl RuleSpec {
    pub fn compile(&self) -> anyhow::Result<CompiledRule> {
        let mut rule = CompiledRule::compile(
            self.id.clone(),
            self.name.clone(),
            &self.expression,
            self.action,
            self.severity,
            self.tags.clone(),
        )?;
        rule.enabled = self.enabled;
        rule.paranoia_level = self.paranoia_level.clamp(1, 4);
        Ok(rule)
    }
}
