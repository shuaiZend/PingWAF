//! `log_retention_settings` — one global row holding how long the two log
//! tables are kept, in days.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize,
)]
#[sea_orm(table_name = "log_retention_settings")]
pub struct Model {
    /// Always `1`: the settings are global, not per-site.
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: i32,
    pub access_log_retention_days: i32,
    pub security_event_retention_days: i32,
    /// Skip access-log rows for static-asset requests that triggered no
    /// security rule. Requests that did trigger one are always logged.
    pub skip_static_access_logs: bool,
    /// Path suffixes classified as static assets; lowercase, no leading dot.
    pub static_suffixes: Vec<String>,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Accepted retention windows, matching the manual purge endpoint.
pub const MIN_RETENTION_DAYS: i32 = 1;
pub const MAX_RETENTION_DAYS: i32 = 3650;
/// Retention a settings row is created with.
pub const DEFAULT_RETENTION_DAYS: i32 = 180;

/// Suffixes a fresh settings row (or a migration default) classifies as
/// static assets. Lowercase, no leading dot. Deliberately excludes
/// document-ish extensions (`json`, `html`, `txt`, `xml`): APIs commonly
/// answer with those, and skipping them would hide real traffic.
pub const DEFAULT_STATIC_SUFFIXES: [&str; 18] = [
    "css", "js", "mjs", "map", "png", "jpg", "jpeg", "gif", "webp", "avif",
    "svg", "ico", "bmp", "woff", "woff2", "ttf", "otf", "eot",
];
/// Upper bound on the user-editable suffix list.
pub const MAX_STATIC_SUFFIXES: usize = 64;
/// Upper bound on one suffix, after normalization.
pub const MAX_STATIC_SUFFIX_LEN: usize = 10;

/// Normalizes one user-supplied suffix: trims whitespace, strips one leading
/// dot, lowercases. Returns `None` when the result is not a usable suffix
/// (empty, too long, or characters outside `[a-z0-9]`), which the API turns
/// into a 400 instead of silently dropping input.
pub fn normalize_suffix(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let stripped =
        trimmed.strip_prefix('.').unwrap_or(trimmed).to_lowercase();
    if stripped.is_empty()
        || stripped.len() > MAX_STATIC_SUFFIX_LEN
        || !stripped
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        return None;
    }
    Some(stripped)
}

/// Normalizes and validates a whole suffix list: rejects invalid entries,
/// deduplicates preserving order, enforces the length cap. `Err` carries the
/// first offending raw entry for the API error message.
pub fn normalize_suffix_list(
    raw: &[String],
) -> Result<Vec<String>, String> {
    if raw.len() > MAX_STATIC_SUFFIXES {
        return Err(format!(
            "at most {MAX_STATIC_SUFFIXES} suffixes are allowed"
        ));
    }
    let mut out: Vec<String> = Vec::with_capacity(raw.len());
    for entry in raw {
        match normalize_suffix(entry) {
            Some(suffix) => {
                if !out.contains(&suffix) {
                    out.push(suffix);
                }
            },
            None => {
                return Err(format!(
                    "invalid suffix {entry:?}: use 1-{MAX_STATIC_SUFFIX_LEN} \
                     lowercase letters or digits, without a leading dot"
                ));
            },
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_suffixes_are_normalized() {
        for suffix in DEFAULT_STATIC_SUFFIXES {
            assert_eq!(
                normalize_suffix(suffix).as_deref(),
                Some(suffix),
                "built-in suffix `{suffix}` must pass its own validation"
            );
        }
    }

    #[test]
    fn default_suffixes_have_no_duplicates() {
        let set: std::collections::BTreeSet<&str> =
            DEFAULT_STATIC_SUFFIXES.iter().copied().collect();
        assert_eq!(set.len(), DEFAULT_STATIC_SUFFIXES.len());
    }

    #[test]
    fn normalize_suffix_accepts_and_strips_dot() {
        assert_eq!(normalize_suffix(" PNG ").as_deref(), Some("png"));
        assert_eq!(normalize_suffix(".Woff2").as_deref(), Some("woff2"));
        assert_eq!(normalize_suffix("woff2").as_deref(), Some("woff2"));
    }

    #[test]
    fn normalize_suffix_rejects_bad_input() {
        assert_eq!(normalize_suffix(""), None);
        assert_eq!(normalize_suffix("   "), None);
        assert_eq!(normalize_suffix("."), None);
        assert_eq!(normalize_suffix("*.png"), None);
        assert_eq!(normalize_suffix("a.b"), None);
        assert_eq!(normalize_suffix("ä"), None);
    }

    #[test]
    fn suffix_list_dedupes_and_rejects() {
        let list: Vec<String> = ["png", ".png", "JPG"]
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            normalize_suffix_list(&list).unwrap(),
            vec!["png".to_string(), "jpg".to_string()]
        );
        assert!(normalize_suffix_list(&["bad.suffix".to_string()]).is_err());
        let too_many: Vec<String> =
            (0..65).map(|i| format!("s{i:02}")).collect();
        assert!(normalize_suffix_list(&too_many).is_err());
    }
}
