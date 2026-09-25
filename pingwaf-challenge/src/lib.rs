pub mod cookie;
pub mod fingerprint;
pub mod js_challenge;

// Re-export main types for convenient access
pub use cookie::{ClearanceLevel, ClearancePayload, CookieError, CookieManager};
pub use fingerprint::BrowserFingerprint;
pub use js_challenge::{
    ChallengeConfig, ChallengeDecision, ChallengeEngine, ChallengeRequest, ChallengeResponse,
    ChallengeSubmission, VerifyResult, generate_request_id,
};
pub use js_challenge::verify::{IntegrityResult, check_browser_integrity};

/// Challenge level (public enum matching the proto definition)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChallengeLevel {
    /// No challenge required
    None,
    /// JS Challenge (5-second shield)
    NonInteractive,
    /// System auto-decides between JS and interactive based on risk
    Managed,
    /// Full CAPTCHA-style verification
    Interactive,
}

impl From<ClearanceLevel> for ChallengeLevel {
    fn from(level: ClearanceLevel) -> Self {
        match level {
            ClearanceLevel::None => Self::None,
            ClearanceLevel::NonInteractive => Self::NonInteractive,
            ClearanceLevel::Managed => Self::Managed,
            ClearanceLevel::Interactive => Self::Interactive,
        }
    }
}

impl From<ChallengeLevel> for ClearanceLevel {
    fn from(level: ChallengeLevel) -> Self {
        match level {
            ChallengeLevel::None => Self::None,
            ChallengeLevel::NonInteractive => Self::NonInteractive,
            ChallengeLevel::Managed => Self::Managed,
            ChallengeLevel::Interactive => Self::Interactive,
        }
    }
}

impl ChallengeLevel {
    /// Returns true if this level requires at least a JS challenge
    pub fn requires_challenge(&self) -> bool {
        !matches!(self, Self::None)
    }

    /// Returns the string representation used in configuration
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::NonInteractive => "non_interactive",
            Self::Managed => "managed",
            Self::Interactive => "interactive",
        }
    }

    /// Parse from a string representation
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "non_interactive" | "noninteractive" | "js" | "js_challenge" => {
                Self::NonInteractive
            }
            "managed" => Self::Managed,
            "interactive" | "captcha" => Self::Interactive,
            _ => Self::None,
        }
    }
}
