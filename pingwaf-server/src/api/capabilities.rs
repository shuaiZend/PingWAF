//! Control-plane capability gating for path-scoped features.
//!
//! The edge agent interprets path scopes — `match_type` on path exceptions and
//! the optional scope carried by IP, geo, basic-auth and OIDC rules. Three data
//! plane capabilities gate what the control plane may accept:
//!
//! * **path scopes on access/identity rules** — the optional `path` +
//!   `match_type` pair on `IpAccessRule`, `GeoConfig`, `BasicAuthConfig` and the
//!   OIDC config. Introduced together with `pingap-plugin/src/path_filter.rs` in
//!   commit `b543db2a`, first shipped in **v0.32.20**. On v0.32.16 those
//!   messages have no such field at all (`git show
//!   v0.32.16:pingwaf-proto/proto/control_plane.proto`), so the agent decodes an
//!   empty path — and an empty path means *the whole site*
//!   (`pingap-plugin/src/path_filter.rs`: empty ⇒ `None` ⇒ no filter). A scoped
//!   `allow` rule therefore becomes allow-everywhere, and a scoped basic-auth
//!   rule locks the whole site.
//! * **regex scopes** — `match_type = "regex"`. Same introduction, **v0.32.20**:
//!   on v0.32.16 the matcher does not exist at all, which is why E2E saw a regex
//!   exception accepted by the API yet silently inert on the edge.
//! * **canonical path matching** — scopes are matched against the
//!   *normalised* request path, so `/exc/../index.html` no longer matches a
//!   `/exc` prefix exception while the origin sees `/index.html`. Introduced in
//!   commit `5a3aab0d`, first shipped in **v0.32.21**. On v0.32.16 and v0.32.20
//!   a prefix exception matched the raw path, so a dot-segment request bypassed
//!   the exception scope — and its WAF checks — entirely.
//!
//! Every gate is fail-closed: an agent whose reported version cannot be parsed
//! is treated as *not* supporting the capability.
//!
//! # One decision, many write paths
//!
//! Five write paths can store a scope: path exceptions, IP rules, geo rules,
//! basic-auth, OIDC. Wiring each handler to its own `ensure_*` was how the first
//! release of this gate shipped with three of the five unguarded. The contract
//! is therefore a pure function per surface ([`path_exception_requirements`],
//! [`attached_scope_requirements`]) plus one async enforcer
//! ([`ensure_scope_supported`]); a handler decides *what* it is about to store
//! and the enforcer checks it. Adding a sixth surface means calling one function,
//! not inventing a sixth `ensure_*`.

use sea_orm::EntityTrait;
use uuid::Uuid;

use crate::api::error::ApiError;

/// Minimum agent version that understands an optional path scope attached to an
/// access or identity rule (IP / geo / basic-auth / OIDC).
pub const MIN_AGENT_PATH_SCOPES: &str = "0.32.20";

/// Minimum agent version whose path scopes are matched against the
/// canonicalised (dot-segment-resolved) request path. `5a3aab0d`, v0.32.21.
pub const MIN_AGENT_CANONICAL_SCOPES: &str = "0.32.21";

/// Minimum agent version that implements `match_type = "regex"` path
/// scopes at all. `b543db2a`, v0.32.20 (`path_filter.rs` does not exist on
/// v0.32.16).
pub const MIN_AGENT_REGEX_SCOPES: &str = "0.32.20";

/// Feature label used in rejection messages for an attached path scope.
pub const PATH_SCOPE_LABEL: &str = "path scope";

/// Feature label used in rejection messages for canonical path matching.
pub const CANONICAL_SCOPE_LABEL: &str = "canonical path matching";

/// Feature label used in rejection messages for regex scopes.
pub const REGEX_SCOPE_LABEL: &str = "regex scope";

/// Agent version that could not be parsed; shown verbatim in messages.
const UNKNOWN_VERSION: &str = "unknown";

/// One thing a scope needs from the data plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeRequirement {
    /// Minimum agent version, e.g. `0.32.21`.
    pub min: &'static str,
    /// Feature name used in the rejection message.
    pub label: &'static str,
}

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
    let base = format!(
        "path scope is not supported by agent {name} ({version}); \
         {feature_label} requires agent >= {min}"
    );
    if version == UNKNOWN_VERSION {
        // "Upgrade it" is not actionable when there is no version to compare:
        // the row predates registration reporting or was written by an older
        // control plane. Name the two ways out instead of the version bump.
        return format!(
            "{base}; this agent never reported a version — restart it so it \
             re-registers, or delete its row if it is decommissioned"
        );
    }
    base
}

/// Requirements a **path exception** imposes.
///
/// A path exception always carries a path, so the base row works on every agent
/// that ever shipped the feature (0.32.16 included, verified on the demo); only
/// the two newer semantics are gated. Both can apply at once — a regex
/// exception that also skips layers needs regex *and* canonical matching — so
/// the list is returned narrowest-first: an agent that fails both should hear
/// the higher bar (`0.32.21`), because upgrading to `0.32.20` would not help.
pub fn path_exception_requirements(
    match_type: &str,
    skip_layers: &[String],
) -> Vec<ScopeRequirement> {
    let mut requirements = Vec::new();
    if match_type == "regex" {
        requirements.push(ScopeRequirement {
            min: MIN_AGENT_REGEX_SCOPES,
            label: REGEX_SCOPE_LABEL,
        });
    }
    if !skip_layers.is_empty() {
        requirements.push(ScopeRequirement {
            min: MIN_AGENT_CANONICAL_SCOPES,
            label: CANONICAL_SCOPE_LABEL,
        });
    }
    narrowest_first(requirements)
}

/// Requirements an **optional scope attached to an access/identity rule**
/// imposes (IP rules, geo, basic-auth, OIDC).
///
/// Any non-empty path needs the whole `path_filter` feature, not merely regex:
/// an agent from before v0.32.20 has no field to receive it and applies the rule
/// to the entire site. An empty path is the historical "whole site" scope and is
/// therefore not gated.
pub fn attached_scope_requirements(
    path: Option<&str>,
    _match_type: &str,
) -> Vec<ScopeRequirement> {
    let has_scope = path.is_some_and(|value| !value.trim().is_empty());
    if !has_scope {
        return Vec::new();
    }
    vec![ScopeRequirement {
        min: MIN_AGENT_PATH_SCOPES,
        label: PATH_SCOPE_LABEL,
    }]
}

/// Sorts so the strictest requirement is reported first.
fn narrowest_first(
    mut requirements: Vec<ScopeRequirement>,
) -> Vec<ScopeRequirement> {
    requirements.sort_by(|a, b| {
        let left = parse_semver(a.min);
        let right = parse_semver(b.min);
        right.cmp(&left)
    });
    requirements
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

/// Enforces every requirement, narrowest first, and reports the first failure.
///
/// Empty list = nothing to gate (either no requirement, or no agent serves the
/// site, in which case the config is not pushed anywhere).
pub async fn ensure_scope_supported<C>(
    db: &C,
    site_id: Uuid,
    requirements: &[ScopeRequirement],
) -> Result<(), ApiError>
where
    C: sea_orm::ConnectionTrait,
{
    for requirement in requirements {
        let below = site_agents_below(db, site_id, requirement.min).await?;
        if let Some((name, version)) = below.first() {
            return Err(ApiError::BadRequest(below_message(
                name,
                version,
                requirement.label,
                requirement.min,
            )));
        }
    }
    Ok(())
}

/// Gate for a path-exception write with the given effective `match_type`
/// and `skip_layers`.
pub async fn ensure_path_exception_supported<C>(
    db: &C,
    site_id: Uuid,
    match_type: &str,
    skip_layers: &[String],
) -> Result<(), ApiError>
where
    C: sea_orm::ConnectionTrait,
{
    let requirements = path_exception_requirements(match_type, skip_layers);
    ensure_scope_supported(db, site_id, &requirements).await
}

/// Gate for the optional scope attached to an IP access rule.
pub async fn ensure_ip_rule_supported<C>(
    db: &C,
    site_id: Uuid,
    path: &str,
    match_type: &str,
) -> Result<(), ApiError>
where
    C: sea_orm::ConnectionTrait,
{
    let requirements = attached_scope_requirements(Some(path), match_type);
    ensure_scope_supported(db, site_id, &requirements).await
}

/// Gate for the optional scope attached to a geo, basic-auth or OIDC rule.
///
/// `path` is optional because those endpoints treat an absent scope as "whole
/// site"; passing `None` is equivalent to an empty path.
pub async fn ensure_attached_scope_supported<C>(
    db: &C,
    site_id: Uuid,
    path: Option<&str>,
    match_type: &str,
) -> Result<(), ApiError>
where
    C: sea_orm::ConnectionTrait,
{
    let requirements = attached_scope_requirements(path, match_type);
    ensure_scope_supported(db, site_id, &requirements).await
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

        // An agent that never reported a version cannot be "upgraded" — the
        // message has to name the ways out that actually exist.
        let unknown = below_message(
            "ghost",
            UNKNOWN_VERSION,
            CANONICAL_SCOPE_LABEL,
            MIN_AGENT_CANONICAL_SCOPES,
        );
        assert!(unknown.contains("never reported a version"), "{unknown}");
        assert!(unknown.contains("restart"), "{unknown}");
        assert!(unknown.contains("delete its row"), "{unknown}");
    }

    /// The decision table every write path shares. A new scope kind must appear
    /// here and in the handler that stores it — that is the whole point of
    /// keeping the decision pure.
    #[test]
    fn path_exception_requirements_cover_the_semantics_matrix() {
        let layers = vec!["waf".to_string()];

        // Plain prefix exception: works on every agent that has the feature.
        assert!(path_exception_requirements("prefix", &[]).is_empty());

        // Prefix + skip_layers: needs canonical matching (the dot-segment
        // escape), not merely the feature.
        let prefix_skip = path_exception_requirements("prefix", &layers);
        assert_eq!(
            prefix_skip,
            vec![ScopeRequirement {
                min: MIN_AGENT_CANONICAL_SCOPES,
                label: CANONICAL_SCOPE_LABEL
            }]
        );

        // Regex alone: needs the regex matcher.
        let regex_only = path_exception_requirements("regex", &[]);
        assert_eq!(
            regex_only,
            vec![ScopeRequirement {
                min: MIN_AGENT_REGEX_SCOPES,
                label: REGEX_SCOPE_LABEL
            }]
        );

        // Regex + skip_layers: both bars apply, and the stricter one is
        // reported first so a 0.32.20 agent is not told to upgrade to 0.32.20.
        let both = path_exception_requirements("regex", &layers);
        assert_eq!(both.len(), 2);
        assert_eq!(both[0].min, MIN_AGENT_CANONICAL_SCOPES);
        assert_eq!(both[1].min, MIN_AGENT_REGEX_SCOPES);
        assert!(
            parse_semver(both[0].min) > parse_semver(both[1].min),
            "narrowest requirement must come first: {both:?}"
        );
    }

    /// A scope attached to an access/identity rule is all-or-nothing: the old
    /// proto has no field for it, so the rule silently applies site-wide.
    #[test]
    fn attached_scopes_are_gated_on_any_non_empty_path() {
        for match_type in ["prefix", "exact", "wildcard", "regex"] {
            assert_eq!(
                attached_scope_requirements(Some("/admin"), match_type),
                vec![ScopeRequirement {
                    min: MIN_AGENT_PATH_SCOPES,
                    label: PATH_SCOPE_LABEL
                }],
                "{match_type} scope must be gated"
            );
        }

        // Whole-site scope is the historical meaning and needs nothing new.
        assert!(attached_scope_requirements(Some(""), "prefix").is_empty());
        assert!(attached_scope_requirements(Some("   "), "regex").is_empty());
        assert!(attached_scope_requirements(None, "regex").is_empty());
    }
}
