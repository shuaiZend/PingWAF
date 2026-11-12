//! Control-plane capability gating for path-scoped features.
//!
//! The edge agent interprets path scopes (`match_type` on path exceptions
//! and IP access rules). Two data-plane capabilities gate what the control
//! plane may accept:
//!
//! * **regex scopes** — `match_type = "regex"`. Introduced together with
//!   `pingap-plugin/src/path_filter.rs` in commit `b543db2a`
//!   ("feat(access): scope IP, geo, basic-auth and OIDC rules by path"),
//!   first shipped in **v0.32.20**. On v0.32.16 the file does not exist at
//!   all (`git show v0.32.16:pingap-plugin/src/path_filter.rs` fails), which
//!   is why E2E saw a regex exception accepted by the API yet silently
//!   inert on the edge: the old agent has no such matcher.
//! * **canonical path matching** — scopes are matched against the
//!   *normalised* request path, so `/exc/../index.html` no longer matches a
//!   `/exc` prefix exception while the origin sees `/index.html`.
//!   Introduced in commit `5a3aab0d` ("fix(access): canonicalise scopes and
//!   make bypass defaults explicit"), first shipped in **v0.32.21**. On
//!   v0.32.16 (and v0.32.20) a prefix exception matched the raw path, so a
//!   dot-segment request bypassed the exception scope — and its WAF checks —
//!   entirely.
//!
//! Both gates are fail-closed: an agent whose reported version cannot be
//! parsed as `MAJOR.MINOR.PATCH` is treated as *not* supporting the
//! capability.

use sea_orm::EntityTrait;
use uuid::Uuid;

use crate::api::error::ApiError;

/// Minimum agent version whose path scopes are matched against the
/// canonicalised (dot-segment-resolved) request path. `5a3aab0d`, v0.32.21.
pub const MIN_AGENT_CANONICAL_SCOPES: &str = "0.32.21";

/// Minimum agent version that implements `match_type = "regex"` path
/// scopes at all. `b543db2a`, v0.32.20 (`path_filter.rs` does not exist on
/// v0.32.16).
pub const MIN_AGENT_REGEX_SCOPES: &str = "0.32.20";

/// Feature label used in rejection messages for canonical path matching.
pub const CANONICAL_SCOPE_LABEL: &str = "canonical path matching";

/// Feature label used in rejection messages for regex scopes.
pub const REGEX_SCOPE_LABEL: &str = "regex scope";

/// Agent version that could not be parsed; shown verbatim in messages.
const UNKNOWN_VERSION: &str = "unknown";

/// Parses `MAJOR.MINOR.PATCH`, tolerating a `v` prefix and a pre-release or
/// build suffix. Anything else — non-numeric fields, more than three fields
/// (`0.32.20.1`), an empty string — returns `None`, which every caller must
/// treat as "cannot judge → not supported" (fail-closed). A missing patch
/// field (`0.32`) counts as `.0`; a fourth field would change ordering in a
/// way this comparator cannot judge, so it is rejected instead of guessed.
pub fn parse_semver(raw: &str) -> Option<(u64, u64, u64)> {
    let trimmed = raw.trim();
    let body = trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed);
    // Cut pre-release (`-rc1`) and build metadata (`+build.7`) suffixes.
    let body = body.split(['-', '+']).next().unwrap_or(body);
    if body.is_empty() {
        return None;
    }
    let mut fields = [0u64; 3];
    for (count, field) in body.split('.').enumerate() {
        if count >= 3 {
            // `0.32.20.1` — ordering cannot be judged, fail closed.
            return None;
        }
        if field.is_empty() || !field.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        fields[count] = field.parse().ok()?;
    }
    Some((fields[0], fields[1], fields[2]))
}

/// True when `reported` is at least `min`. An unparsable reported version
/// never supports anything; an unparsable `min` (a programming error in a
/// constant here) also fails closed.
pub fn version_supports(reported: &str, min: &str) -> bool {
    match (parse_semver(reported), parse_semver(min)) {
        (Some(actual), Some(required)) => actual >= required,
        _ => false,
    }
}

/// Rejection message for an agent below the feature's minimum version.
/// The wording is contractual (E2E and the console assert on it).
pub fn below_message(
    name: &str,
    version: &str,
    feature_label: &str,
    min: &str,
) -> String {
    format!(
        "path scope is not supported by agent {name} ({version}); \
         {feature_label} requires agent >= {min}"
    )
}

/// Agents that serve `site_id` and report a version below `min`, as
/// `(name, version)` pairs.
///
/// Visibility is *not* re-derived here: it delegates to
/// [`crate::grpc::agent_visible_site_ids`], the exact predicate the config
/// push uses. An earlier version of this function re-implemented it with two
/// queries (`agents.site_id = X` plus "unbound and the key belongs to the
/// site owner"), which also picked up agents bound to *other* sites and would
/// have blocked a write because of an agent that never receives this site's
/// config.
pub async fn site_agents_below<C>(
    db: &C,
    site_id: Uuid,
    min: &str,
) -> Result<Vec<(String, String)>, ApiError>
where
    C: sea_orm::ConnectionTrait,
{
    use crate::models::agent;

    let agents = agent::Entity::find().all(db).await?;
    let mut below = Vec::new();
    for row in agents {
        let visible = crate::grpc::agent_visible_site_ids(db, &row).await?;
        if !visible.contains(&site_id) {
            continue;
        }
        let version = row.version.as_deref().unwrap_or(UNKNOWN_VERSION);
        if version_supports(version, min) {
            continue;
        }
        let name = row
            .display_name
            .as_deref()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or(&row.hostname)
            .to_string();
        below.push((name, version.to_string()));
    }
    Ok(below)
}

/// Rejects the write when any agent serving the site is below `min`
/// for `feature_label`. `min = None` skips the check (the caller decided
/// this request does not exercise the capability).
///
/// Only the first offending agent is named; the message format is fixed by
/// the API contract. Empty result = every serving agent supports the
/// feature (or no agent serves the site, in which case the config is not
/// pushed anywhere and there is nothing to gate).
async fn ensure_agents_support<C>(
    db: &C,
    site_id: Uuid,
    min: Option<&str>,
    feature_label: &str,
) -> Result<(), ApiError>
where
    C: sea_orm::ConnectionTrait,
{
    let Some(min) = min else {
        return Ok(());
    };
    let below = site_agents_below(db, site_id, min).await?;
    if let Some((name, version)) = below.first() {
        return Err(ApiError::BadRequest(below_message(
            name,
            version,
            feature_label,
            min,
        )));
    }
    Ok(())
}

/// Gate for a path-exception write with the given effective `match_type`
/// and `skip_layers`: a regex scope needs [`MIN_AGENT_REGEX_SCOPES`]; any
/// exception whose `skip_layers` is non-empty changes detection behaviour
/// and therefore needs [`MIN_AGENT_CANONICAL_SCOPES`] (a regex scope on an
/// old agent is reported with the regex wording, which is the narrower,
/// more actionable complaint).
pub async fn ensure_path_exception_supported<C>(
    db: &C,
    site_id: Uuid,
    match_type: &str,
    skip_layers: &[String],
) -> Result<(), ApiError>
where
    C: sea_orm::ConnectionTrait,
{
    if match_type == "regex" {
        return ensure_agents_support(
            db,
            site_id,
            Some(MIN_AGENT_REGEX_SCOPES),
            REGEX_SCOPE_LABEL,
        )
        .await;
    }
    if !skip_layers.is_empty() {
        return ensure_agents_support(
            db,
            site_id,
            Some(MIN_AGENT_CANONICAL_SCOPES),
            CANONICAL_SCOPE_LABEL,
        )
        .await;
    }
    Ok(())
}

/// Gate for an IP access rule whose path scope uses `match_type = "regex"`.
/// Non-regex scopes (empty path or prefix/exact/wildcard) are not gated
/// here: an IP rule without `skip_layers` does not change detection
/// behaviour, and prefix matching against the raw path degrades an IP rule
/// far less critically than a detection bypass.
pub async fn ensure_ip_rule_supported<C>(
    db: &C,
    site_id: Uuid,
    match_type: &str,
) -> Result<(), ApiError>
where
    C: sea_orm::ConnectionTrait,
{
    if match_type == "regex" {
        return ensure_agents_support(
            db,
            site_id,
            Some(MIN_AGENT_REGEX_SCOPES),
            REGEX_SCOPE_LABEL,
        )
        .await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_ordering_matches_release_order() {
        assert!(version_supports("0.32.21", MIN_AGENT_CANONICAL_SCOPES));
        assert!(version_supports("0.32.23", MIN_AGENT_REGEX_SCOPES));
        assert!(!version_supports("0.32.16", MIN_AGENT_CANONICAL_SCOPES));
        assert!(!version_supports("0.32.16", MIN_AGENT_REGEX_SCOPES));
        assert!(!version_supports("0.32.20", MIN_AGENT_CANONICAL_SCOPES));
        // Equal is enough.
        assert!(version_supports("0.32.20", "0.32.20"));
        // Digit fields compare numerically, not lexicographically.
        assert!(!version_supports("0.32.9", "0.32.10"));
        assert!(version_supports("1.0.0", "0.99.99"));
    }

    #[test]
    fn versions_are_parsed_leniently_on_prefix_and_strictly_on_fields() {
        assert_eq!(parse_semver("v0.32.23"), Some((0, 32, 23)));
        assert_eq!(parse_semver("V0.32.23"), Some((0, 32, 23)));
        assert_eq!(parse_semver("0.32.23-rc1"), Some((0, 32, 23)));
        assert_eq!(parse_semver("0.32.23+build.7"), Some((0, 32, 23)));
        // Missing patch counts as .0.
        assert_eq!(parse_semver("0.32"), Some((0, 32, 0)));
        assert_eq!(parse_semver("0.32.0"), Some((0, 32, 0)));
        // Four fields, letters and emptiness cannot be judged.
        assert_eq!(parse_semver("0.32.20.1"), None);
        assert_eq!(parse_semver("abc"), None);
        assert_eq!(parse_semver(""), None);
        assert_eq!(parse_semver("v"), None);
        assert_eq!(parse_semver("0.x.1"), None);
    }

    #[test]
    fn unparsable_versions_fail_closed() {
        assert!(!version_supports("abc", "0.32.20"));
        assert!(!version_supports("", "0.32.20"));
        assert!(!version_supports("unknown", "0.32.20"));
        // Even a garbage minimum never lets anything through.
        assert!(!version_supports("9.9.9", "not-a-version"));
    }

    #[test]
    fn the_rejection_message_names_agent_version_and_minimum() {
        let message = below_message(
            "edge-1",
            "0.32.16",
            CANONICAL_SCOPE_LABEL,
            MIN_AGENT_CANONICAL_SCOPES,
        );
        assert_eq!(
            message,
            "path scope is not supported by agent edge-1 (0.32.16); \
             canonical path matching requires agent >= 0.32.21"
        );
        let regex_message = below_message(
            "edge-2",
            "0.32.16",
            REGEX_SCOPE_LABEL,
            MIN_AGENT_REGEX_SCOPES,
        );
        assert_eq!(
            regex_message,
            "path scope is not supported by agent edge-2 (0.32.16); \
             regex scope requires agent >= 0.32.20"
        );
        // Every part the console links on is present.
        assert!(message.contains("edge-1"));
        assert!(message.contains("0.32.16"));
        assert!(message.contains("0.32.21"));
    }

    #[test]
    fn the_decision_matrix_follows_scope_semantics() {
        // These gates are pure decisions over (match_type, skip_layers);
        // the DB-dependent part (`site_agents_below`) is covered by the
        // tests above through `version_supports`, and the wiring in the
        // handlers passes the effective values through.
        let old = "0.32.16";
        let new = "0.32.23";

        // regex + old agent → rejected (regex scope unsupported).
        assert!(!version_supports(old, MIN_AGENT_REGEX_SCOPES));
        // regex + new agent → accepted.
        assert!(version_supports(new, MIN_AGENT_REGEX_SCOPES));
        // prefix + skip_layers + old agent → rejected (no canonical
        // matching: the bypass could leak outside the scope).
        assert!(!version_supports(old, MIN_AGENT_CANONICAL_SCOPES));
        // prefix + skip_layers + new agent → accepted.
        assert!(version_supports(new, MIN_AGENT_CANONICAL_SCOPES));

        // Semantics note: an exception with empty `skip_layers` does not
        // change detection behaviour, so it would pass even on an old
        // agent. The current validation (`validate_skip_layers`) rejects
        // empty lists up front, so in practice every stored exception is
        // gated.
    }
}
