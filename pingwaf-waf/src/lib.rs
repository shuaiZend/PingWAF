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
        matches!(
            self.action,
            WafAction::Block | WafAction::Challenge
        )
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
