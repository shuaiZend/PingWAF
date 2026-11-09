//! Shared path matching for access-control scoping and path exceptions.
//!
//! The control plane validates `match_type` the same way for both, and the
//! data plane must interpret the four values identically: one matcher keeps
//! the prefix/exact/wildcard/regex semantics (and the regex budget) from
//! drifting between features.

use regex::{Regex, RegexBuilder};
use std::borrow::Cow;

/// Maximum automaton size, matching the WAF expression engine.
const MAX_REGEX_SIZE: usize = 4 * 1024 * 1024;

/// How a configured path is compared with the request path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MatchType {
    /// The path itself or anything below it (`/api` matches `/api/x`, never
    /// `/apix`).
    Prefix,
    /// Exact equality.
    Exact,
    /// A single trailing `*`, i.e. a raw prefix (`/api/*` matches `/api/x`,
    /// not `/api`).
    Wildcard,
    /// Operator-written Rust regex, unanchored and case-sensitive.
    Regex,
}

impl MatchType {
    /// Unknown values degrade to `prefix` — the proto default — rather than
    /// to a broader match.
    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "exact" => Self::Exact,
            "wildcard" => Self::Wildcard,
            "regex" => Self::Regex,
            _ => Self::Prefix,
        }
    }
}

/// A compiled path scope. `None` from [`PathFilter::compile`] means "the whole
/// site"; an empty path is deliberately not a filter.
#[derive(Clone, Debug)]
pub(crate) struct PathFilter {
    match_type: MatchType,
    /// For `Wildcard` this is the pattern with the trailing `*` removed.
    pattern: String,
    regex: Option<Regex>,
}

impl PathFilter {
    /// Compiles `path`/`match_type`; `Ok(None)` when the path is empty.
    pub(crate) fn compile(
        path: &str,
        match_type: &str,
    ) -> Result<Option<Self>, regex::Error> {
        // A historical cache may carry padding; treat it as the whole site
        // rather than as a pattern that can never match.
        if path.trim().is_empty() {
            return Ok(None);
        }
        let match_type = MatchType::parse(match_type);
        let (pattern, regex) = match match_type {
            MatchType::Wildcard => {
                (path.trim_end_matches('*').to_string(), None)
            },
            MatchType::Regex => (path.to_string(), Some(compile_regex(path)?)),
            MatchType::Prefix | MatchType::Exact => (path.to_string(), None),
        };
        Ok(Some(Self {
            match_type,
            pattern,
            regex,
        }))
    }

    pub(crate) fn match_type(&self) -> MatchType {
        self.match_type
    }

    pub(crate) fn matches(&self, path: &str) -> bool {
        // Scope decisions must use the same canonical view the WAF engine
        // inspects. Matching the raw URI would let `/api/../admin` satisfy a
        // `/api` exception while the origin resolves it to `/admin`.
        let Some(path) = canonical_path(path) else {
            // A suspicious path that cannot be canonicalised safely matches
            // no scope: fail-safe beats guessing what the origin will do.
            return false;
        };
        match self.match_type {
            MatchType::Exact => path == self.pattern,
            MatchType::Prefix => {
                path == self.pattern
                    || path
                        .strip_prefix(self.pattern.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            },
            MatchType::Wildcard => path.starts_with(self.pattern.as_str()),
            MatchType::Regex => self
                .regex
                .as_ref()
                .is_some_and(|pattern| pattern.is_match(&path)),
        }
    }
}

/// Canonical path for scope matching, without touching the original path used
/// for logging and proxying. Decoding and dot-segment resolution only run when
/// the raw path could hide a different path (`%`, `..`, `//`, `/.`), so the
/// common case stays an allocation-free borrow.
fn canonical_path(path: &str) -> Option<Cow<'_, str>> {
    // `normalize` treats `#` as a fragment and truncates there; a request
    // target should never carry one, and truncating it could turn a
    // non-matching path into a matching one. Refuse to scope it instead.
    if path.contains('#') || path.contains("..;") {
        return None;
    }
    if !(path.contains('%')
        || path.contains("..")
        || path.contains("//")
        || path.contains("/."))
    {
        return Some(Cow::Borrowed(path));
    }
    // Keep this in lock-step with the engine's default decode depth. A site
    // configured for more layers may see a more-decoded path in the engine;
    // the conservative safe reading is to scope what three layers expose.
    let decoded = pingwaf_waf::normalize::url::multi_decode(path, 3);
    Some(Cow::Owned(pingwaf_waf::normalize::path::normalize(
        &decoded,
    )))
}

/// The same automaton budget the WAF expression engine uses: one pathological
/// pattern cannot pin tens of megabytes on the data plane.
fn compile_regex(pattern: &str) -> Result<Regex, regex::Error> {
    RegexBuilder::new(pattern)
        .size_limit(MAX_REGEX_SIZE)
        .dfa_size_limit(MAX_REGEX_SIZE)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_path_is_the_whole_site() {
        assert!(PathFilter::compile("", "prefix").unwrap().is_none());
        assert!(PathFilter::compile("", "regex").unwrap().is_none());
    }

    #[test]
    fn prefix_matches_below_but_not_a_sibling() {
        let filter = PathFilter::compile("/api", "prefix").unwrap().unwrap();
        assert!(filter.matches("/api"));
        assert!(filter.matches("/api/v1"));
        assert!(!filter.matches("/apix"));
        assert!(!filter.matches("/"));
    }

    #[test]
    fn exact_does_not_match_below() {
        let filter = PathFilter::compile("/admin", "exact").unwrap().unwrap();
        assert!(filter.matches("/admin"));
        assert!(!filter.matches("/admin/users"));
    }

    #[test]
    fn wildcard_is_a_raw_prefix() {
        let filter = PathFilter::compile("/static/*", "wildcard")
            .unwrap()
            .unwrap();
        assert!(filter.matches("/static/app.js"));
        assert!(!filter.matches("/static"));
        assert!(!filter.matches("/other"));
    }

    #[test]
    fn scope_matching_uses_the_canonical_path() {
        let api = PathFilter::compile("/api", "prefix").unwrap().unwrap();
        assert!(!api.matches("/api/../admin"));
        assert!(!api.matches("/api/%2e%2e/admin"));
        assert!(!api.matches("/api#/../admin"));
        assert!(!api.matches("/api/..;/admin"));
        assert!(api.matches("/api//v1"));
        assert!(api.matches("/api/v1"));

        let admin = PathFilter::compile("/admin", "exact").unwrap().unwrap();
        assert!(!admin.matches("/admin/"));
    }

    #[test]
    fn regex_is_unanchored_and_case_sensitive() {
        let filter = PathFilter::compile(r"^/Admin(/|$)", "regex")
            .unwrap()
            .unwrap();
        assert!(filter.matches("/Admin"));
        assert!(filter.matches("/Admin/users"));
        assert!(!filter.matches("/admin"));
        assert!(!filter.matches("/x/Admin"));
    }

    #[test]
    fn unknown_match_type_degrades_to_prefix() {
        let filter = PathFilter::compile("/api", "future").unwrap().unwrap();
        assert_eq!(filter.match_type(), MatchType::Prefix);
        assert!(filter.matches("/api/x"));
    }

    #[test]
    fn invalid_regex_is_reported() {
        assert!(PathFilter::compile("^/broken(", "regex").is_err());
    }
}
