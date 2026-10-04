//! PingWAF detection engine.
//!
//! Multi-stage request inspection:
//! 1. Normalize / decode input (URL, HTML entities, path traversal collapsing).
//! 2. Stage 1 — fast path: Aho-Corasick signature scan plus libinjection-style
//!    SQLi / XSS detectors. Target ≤ 100µs.
//! 3. Stage 2 — rule engine: expression-based custom and managed rules with
//!    anomaly scoring. Target ≤ 300µs.
//! 4. Emit a [`WafVerdict`] describing the action, score and matched rules.

pub mod engine;
pub mod normalize;
pub mod rules;
pub mod score;

pub use engine::{RequestData, WafEngine, WafEngineConfig, WafMode};
pub use rules::{CompiledRule, RuleAction};
pub use score::{AnomalyScorer, ScoreBreakdown, ScoreClass};

use serde::{Deserialize, Serialize};

/// Detection level of the engine. Users trade coverage against false
/// positives / per-request cost: `Normal` keeps detection cheap and quiet,
/// `Strict` adds extra decoding passes, structural detectors, tighter
/// sub-score gates and activates the paranoia-level-3 rules.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum WafLevel {
    #[default]
    Normal,
    Strict,
}

impl WafLevel {
    pub fn is_strict(self) -> bool {
        matches!(self, Self::Strict)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Strict => "strict",
        }
    }

    /// Parse a config string ("normal" / "strict", case-insensitive).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "normal" => Some(Self::Normal),
            "strict" => Some(Self::Strict),
            _ => None,
        }
    }
}

/// Backend-technology dimension of the detection profile.
///
/// Payload families are tagged with the stack whose runtime actually parses
/// them (`aced0005` only matters to a JVM, `unserialize(` to PHP, …), so an
/// engine built for a known stack set can drop irrelevant needles and rules
/// from the hot path entirely. [`StackSet::GENERIC`] patterns apply to every
/// backend and can never be switched off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StackSet(u8);

impl StackSet {
    /// Language-agnostic payloads (SQLi, XSS, traversal, …). Always active.
    pub const GENERIC: Self = Self(1 << 0);
    pub const JAVA: Self = Self(1 << 1);
    pub const PHP: Self = Self(1 << 2);
    pub const PYTHON: Self = Self(1 << 3);
    /// Node.js / JavaScript runtimes.
    pub const NODE: Self = Self(1 << 4);
    pub const ALL: Self = Self(0b0001_1111);

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// `true` when every stack in `other` is enabled here.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Parse a single stack name. Accepts common aliases so control-plane
    /// configs written by humans or LLMs still resolve.
    pub fn parse_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "generic" => Some(Self::GENERIC),
            "java" | "jvm" => Some(Self::JAVA),
            "php" => Some(Self::PHP),
            "python" => Some(Self::PYTHON),
            "node" | "nodejs" | "javascript" | "js" => Some(Self::NODE),
            _ => None,
        }
    }

    /// Union of every recognized name. [`StackSet::GENERIC`] is always
    /// included: language-agnostic detection is not optional.
    pub fn from_names<'a, I>(names: I) -> Self
    where
        I: IntoIterator<Item = &'a str>,
    {
        let mut set = Self::GENERIC;
        for name in names {
            if let Some(s) = Self::parse_name(name) {
                set = set.union(s);
            }
        }
        set
    }
}

impl Default for StackSet {
    /// Unconfigured deployments get the full pattern table — narrowing to a
    /// known stack set is an explicit opt-in, never a default.
    fn default() -> Self {
        Self::ALL
    }
}

/// Final verdict produced by [`WafEngine::inspect`].
#[derive(Debug, Clone)]
pub struct WafVerdict {
    /// Action the caller should take.
    pub action: WafAction,
    /// Aggregated anomaly score, saturating at `u8::MAX`.
    pub score: u8,
    /// Identifiers of every rule / signature that fired.
    pub matched_rules: Vec<String>,
    /// Human-readable explanation suitable for logs.
    pub details: String,
    /// Structured score breakdown by attack family.
    pub breakdown: ScoreBreakdown,
}

impl WafVerdict {
    /// Build a clean "pass" verdict with no hits.
    pub fn pass() -> Self {
        Self {
            action: WafAction::Pass,
            score: 0,
            matched_rules: Vec::new(),
            details: String::new(),
            breakdown: ScoreBreakdown::clean(),
        }
    }

    /// `true` when the request should be blocked or challenged.
    pub fn is_blocked(&self) -> bool {
        matches!(self.action, WafAction::Block | WafAction::Challenge)
    }
}

/// Action the upstream proxy should perform for the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WafAction {
    /// No threat detected, allow through.
    Pass,
    /// Threat detected but engine is in monitor mode — log only.
    Monitor,
    /// Hard block.
    Block,
    /// Issue a JS / managed challenge instead of blocking outright.
    Challenge,
}
