//! Helpers shared by the API route modules: pagination, identifier parsing and
//! the multi-tenancy checks that every handler performs before touching data.

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, EntityTrait, Statement};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::error::ApiError;
use crate::auth::AuthUser;
use crate::models::{role, site};

/// Default number of rows returned by a list endpoint.
pub const DEFAULT_PAGE_SIZE: u64 = 50;
/// Upper bound accepted from clients, to keep queries predictable.
pub const MAX_PAGE_SIZE: u64 = 200;

/// `?page=&page_size=` query parameters.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Pagination {
    #[serde(default = "default_page")]
    pub page: u64,
    #[serde(default = "default_page_size")]
    pub page_size: u64,
}

fn default_page() -> u64 {
    1
}

fn default_page_size() -> u64 {
    DEFAULT_PAGE_SIZE
}

impl Default for Pagination {
    fn default() -> Self {
        Self {
            page: 1,
            page_size: DEFAULT_PAGE_SIZE,
        }
    }
}

impl Pagination {
    /// Clamps user supplied values into the supported range.
    pub fn normalise(&self) -> Self {
        Self {
            page: self.page.max(1),
            page_size: self.page_size.clamp(1, MAX_PAGE_SIZE),
        }
    }

    /// Zero-based page index used by [`sea_orm::PaginatorTrait::fetch_page`].
    pub fn index(&self) -> u64 {
        self.normalise().page - 1
    }

    pub fn limit(&self) -> u64 {
        self.normalise().page_size
    }

    pub fn offset(&self) -> u64 {
        self.index() * self.limit()
    }
}

/// Envelope returned by every list endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: u64,
    pub page: u64,
    pub page_size: u64,
}

impl<T> Page<T> {
    pub fn new(items: Vec<T>, total: u64, pagination: Pagination) -> Self {
        let p = pagination.normalise();
        Self {
            items,
            total,
            page: p.page,
            page_size: p.page_size,
        }
    }

    pub fn empty(pagination: Pagination) -> Self {
        Self::new(Vec::new(), 0, pagination)
    }
}

/// Parses a path/query UUID with a 400 instead of a 500.
pub fn parse_uuid(value: &str, what: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(value.trim()).map_err(|err| {
        ApiError::BadRequest(format!("invalid {what} '{value}': {err}"))
    })
}

/// Parses an RFC 3339 timestamp filter.
pub fn parse_datetime(
    value: &str,
    what: &str,
) -> Result<DateTime<Utc>, ApiError> {
    DateTime::parse_from_rfc3339(value.trim())
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|err| {
            ApiError::BadRequest(format!("invalid {what} '{value}': {err}"))
        })
}

/// Optional RFC 3339 filter that tolerates an empty string.
pub fn parse_optional_datetime(
    value: &Option<String>,
    what: &str,
) -> Result<Option<DateTime<Utc>>, ApiError> {
    match value {
        None => Ok(None),
        Some(raw) if raw.trim().is_empty() => Ok(None),
        Some(raw) => parse_datetime(raw, what).map(Some),
    }
}

/// Guards against a `viewer` role performing a mutation.
pub fn require_write(user: &AuthUser) -> Result<(), ApiError> {
    if user.role == role::VIEWER {
        return Err(ApiError::Forbidden(
            "viewer accounts cannot modify resources".to_string(),
        ));
    }
    Ok(())
}

/// Loads a site and asserts that `user` may see (and optionally mutate) it.
///
/// Administrators see every site; other roles only see sites they own. A site
/// owned by somebody else is reported as `404` rather than `403` so that the API
/// does not leak which domains exist.
pub async fn load_site(
    db: &DatabaseConnection,
    site_id: Uuid,
    user: &AuthUser,
    write: bool,
) -> Result<site::Model, ApiError> {
    let model = site::Entity::find_by_id(site_id)
        .one(db)
        .await?
        .ok_or_else(|| {
            ApiError::NotFound(format!("site {site_id} not found"))
        })?;

    if !user.is_admin() {
        if model.user_id != user.id {
            return Err(ApiError::NotFound(format!(
                "site {site_id} not found"
            )));
        }
        if write {
            require_write(user)?;
        }
    }
    Ok(model)
}

/// Convenience wrapper around [`load_site`] for read-only handlers.
pub async fn load_site_read(
    db: &DatabaseConnection,
    site_id: Uuid,
    user: &AuthUser,
) -> Result<site::Model, ApiError> {
    load_site(db, site_id, user, false).await
}

/// Convenience wrapper around [`load_site`] for mutating handlers.
pub async fn load_site_write(
    db: &DatabaseConnection,
    site_id: Uuid,
    user: &AuthUser,
) -> Result<site::Model, ApiError> {
    load_site(db, site_id, user, true).await
}

/// Restricts a site id filter to what `user` is allowed to query.
///
/// Returns `None` when the caller asked for "all sites" *and* is an admin; in
/// every other case handlers must filter by the returned id.
pub fn scope_site(
    requested: Option<Uuid>,
    user: &AuthUser,
) -> Result<Option<Uuid>, ApiError> {
    match requested {
        Some(id) => Ok(Some(id)),
        None if user.is_admin() => Ok(None),
        None => Err(ApiError::BadRequest(
            "site_id is required for non-admin accounts".to_string(),
        )),
    }
}

/// Trims an optional filter string, turning blanks into `None`.
pub fn non_empty(value: &Option<String>) -> Option<String> {
    value
        .as_ref()
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
}

/// Runs a parameterised read-only statement.
pub async fn query_all(
    db: &DatabaseConnection,
    sql: &str,
    values: Vec<sea_orm::Value>,
) -> Result<Vec<sea_orm::QueryResult>, ApiError> {
    db.query_all(Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        sql,
        values,
    ))
    .await
    .map_err(ApiError::from)
}

/// Strips a trailing slash so that both `/api/v1/sites` and `/api/v1/sites/`
/// behave the same when building absolute URLs.
pub fn trim_trailing_slash(value: &str) -> &str {
    value.trim_end_matches('/')
}

/// Lower-cases and validates a domain name.
pub fn normalise_domain(value: &str) -> Result<String, ApiError> {
    let domain = value.trim().trim_start_matches('.').to_lowercase();
    let domain = trim_trailing_slash(&domain)
        .trim_end_matches('.')
        .to_string();
    if domain.is_empty() || domain.len() > 253 {
        return Err(ApiError::BadRequest(format!(
            "invalid domain '{value}': must be 1-253 characters"
        )));
    }
    if !domain.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label.chars().all(is_domain_char)
    }) {
        return Err(ApiError::BadRequest(format!(
            "invalid domain '{value}': labels may only contain letters, digits and '-'"
        )));
    }
    Ok(domain)
}

fn is_domain_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '*'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_is_clamped() {
        assert_eq!(Pagination::default().limit(), DEFAULT_PAGE_SIZE);
        assert_eq!(Pagination::default().offset(), 0);

        let huge = Pagination {
            page: 0,
            page_size: 10_000,
        };
        assert_eq!(huge.normalise().page, 1);
        assert_eq!(huge.limit(), MAX_PAGE_SIZE);

        let third = Pagination {
            page: 3,
            page_size: 20,
        };
        assert_eq!(third.index(), 2);
        assert_eq!(third.offset(), 40);
    }

    #[test]
    fn domains_are_normalised_and_validated() {
        assert_eq!(normalise_domain(" Example.COM ").unwrap(), "example.com");
        assert!(normalise_domain("").is_err());
        assert!(normalise_domain("bad domain.com").is_err());
        assert!(normalise_domain("a..b.com").is_err());
    }

    #[test]
    fn uuid_parsing_reports_bad_request() {
        assert!(parse_uuid("nope", "site id").is_err());
        assert!(parse_uuid(&Uuid::nil().to_string(), "site id").is_ok());
    }

    #[test]
    fn datetime_filters_tolerate_blanks() {
        assert_eq!(parse_optional_datetime(&None, "from").unwrap(), None);
        assert_eq!(
            parse_optional_datetime(&Some("".into()), "from").unwrap(),
            None
        );
        assert!(parse_optional_datetime(
            &Some("2024-01-01T00:00:00Z".into()),
            "from"
        )
        .unwrap()
        .is_some());
        assert!(
            parse_optional_datetime(&Some("yesterday".into()), "from").is_err()
        );
    }

    #[test]
    fn blank_filters_are_dropped() {
        assert_eq!(non_empty(&Some("   ".into())), None);
        assert_eq!(non_empty(&Some(" x ".into())).as_deref(), Some("x"));
        assert_eq!(non_empty(&None), None);
    }
}
