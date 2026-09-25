//! Log queries: security events produced by the WAF and the full access log.

use axum::Json;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::Router;
use chrono::{DateTime, Duration, Utc};
use sea_orm::{ColumnTrait, Condition, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::common::{
    Page, Pagination, non_empty, parse_optional_datetime, parse_uuid, scope_site,
};
use crate::api::error::ApiError;
use crate::api::state::AppState;
use crate::auth::AuthUser;
use crate::models::{access_log, security_event, site};

/// How far back a query may reach when the client does not say.
const DEFAULT_WINDOW_HOURS: i64 = 24;
/// Hard ceiling on the requested window, so a stray `from=1970` cannot scan the
/// whole table.
const MAX_WINDOW_DAYS: i64 = 90;

#[derive(Debug, Deserialize)]
pub struct SecurityQuery {
    #[serde(flatten)]
    pub pagination: Pagination,
    pub site_id: Option<String>,
    /// RFC 3339; defaults to 24 hours ago.
    pub from: Option<String>,
    /// RFC 3339; defaults to now.
    pub to: Option<String>,
    pub client_ip: Option<String>,
    pub action: Option<String>,
    pub rule_id: Option<String>,
    pub host: Option<String>,
    pub path: Option<String>,
    pub country_code: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AccessQuery {
    #[serde(flatten)]
    pub pagination: Pagination,
    pub site_id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub client_ip: Option<String>,
    pub method: Option<String>,
    pub status_code: Option<i32>,
    pub status_class: Option<i32>,
    pub host: Option<String>,
    pub path: Option<String>,
    pub cache_status: Option<String>,
    pub country_code: Option<String>,
    pub min_latency_ms: Option<i64>,
}

/// Retention endpoint payload.
#[derive(Debug, Deserialize)]
pub struct PurgeQuery {
    pub site_id: Option<String>,
    /// Delete rows older than this many days; defaults to 30.
    #[serde(default = "default_retention_days")]
    pub older_than_days: i64,
}

fn default_retention_days() -> i64 {
    30
}

#[derive(Debug, Serialize)]
pub struct PurgeResult {
    pub deleted_security_events: u64,
    pub deleted_access_logs: u64,
    pub cutoff: DateTime<Utc>,
}

/// Routes contributed to `/api/v1`.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/logs/security", get(list_security))
        .route("/logs/access", get(list_access))
        .route("/logs/purge", axum::routing::delete(purge))
}

/// Resolves the site filter and the caller's visibility at once.
///
/// Returns `Ok(None)` for an administrator that did not name a site (meaning "all
/// sites") and a list of allowed site ids otherwise.
async fn resolve_scope(
    db: &DatabaseConnection,
    requested: &Option<String>,
    current: &AuthUser,
) -> Result<Option<Vec<Uuid>>, ApiError> {
    let requested_id = match non_empty(requested) {
        Some(raw) => Some(parse_uuid(&raw, "site id")?),
        None => None,
    };
    // Errors out when a non-admin asks for "everything".
    scope_site(requested_id, current)?;

    if let Some(id) = requested_id {
        return Ok(Some(vec![id]));
    }
    if current.is_admin() {
        return Ok(None);
    }

    let owned: Vec<Uuid> = site::Entity::find()
        .filter(site::Column::UserId.eq(current.id))
        .all(db)
        .await?
        .into_iter()
        .map(|row| row.id)
        .collect();
    Ok(Some(owned))
}

/// Applies the site scope to a condition. An empty allow-list yields a condition
/// that matches nothing, which keeps the query cheap and correct.
fn apply_scope(condition: Condition, scope: &Option<Vec<Uuid>>, column: impl ColumnTrait) -> Condition {
    match scope {
        None => condition,
        Some(ids) if ids.is_empty() => condition.add(column.eq(Uuid::nil()).and(column.ne(Uuid::nil()))),
        Some(ids) => condition.add(column.is_in(ids.iter().copied())),
    }
}

/// Resolves and validates the `from`/`to` window.
fn resolve_window(
    from: &Option<String>,
    to: &Option<String>,
) -> Result<(DateTime<Utc>, DateTime<Utc>), ApiError> {
    let end = match parse_optional_datetime(to, "to")? {
        Some(instant) => instant,
        None => Utc::now(),
    };
    let start = match parse_optional_datetime(from, "from")? {
        Some(instant) => instant,
        None => end - Duration::hours(DEFAULT_WINDOW_HOURS),
    };
    if start >= end {
        return Err(ApiError::BadRequest(
            "'from' must be earlier than 'to'".to_string(),
        ));
    }
    let floor = end - Duration::days(MAX_WINDOW_DAYS);
    if start < floor {
        return Err(ApiError::BadRequest(format!(
            "the query window may not exceed {MAX_WINDOW_DAYS} days"
        )));
    }
    Ok((start, end))
}

/// `GET /api/v1/logs/security`
async fn list_security(
    State(state): State<AppState>,
    current: AuthUser,
    Query(query): Query<SecurityQuery>,
) -> Result<Json<Page<security_event::Model>>, ApiError> {
    let scope = resolve_scope(&state.db, &query.site_id, &current).await?;
    let (from, to) = resolve_window(&query.from, &query.to)?;
    let pagination = query.pagination.normalise();

    let mut condition = Condition::all()
        .add(security_event::Column::Timestamp.gte(from))
        .add(security_event::Column::Timestamp.lt(to));
    condition = apply_scope(condition, &scope, security_event::Column::SiteId);

    if let Some(ip) = non_empty(&query.client_ip) {
        condition = condition.add(security_event::Column::ClientIp.eq(ip));
    }
    if let Some(action) = non_empty(&query.action) {
        condition = condition.add(security_event::Column::Action.eq(action));
    }
    if let Some(rule_id) = non_empty(&query.rule_id) {
        condition = condition.add(security_event::Column::RuleId.eq(rule_id));
    }
    if let Some(host) = non_empty(&query.host) {
        condition = condition.add(security_event::Column::Host.eq(host));
    }
    if let Some(path) = non_empty(&query.path) {
        condition = condition.add(security_event::Column::Path.contains(path));
    }
    if let Some(country) = non_empty(&query.country_code) {
        condition = condition.add(security_event::Column::CountryCode.eq(country.to_uppercase()));
    }

    let paginator = security_event::Entity::find()
        .filter(condition)
        .order_by_desc(security_event::Column::Timestamp)
        .paginate(&state.db, pagination.limit());

    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(pagination.index()).await?;
    Ok(Json(Page::new(rows, total, pagination)))
}

/// `GET /api/v1/logs/access`
async fn list_access(
    State(state): State<AppState>,
    current: AuthUser,
    Query(query): Query<AccessQuery>,
) -> Result<Json<Page<access_log::Model>>, ApiError> {
    let scope = resolve_scope(&state.db, &query.site_id, &current).await?;
    let (from, to) = resolve_window(&query.from, &query.to)?;
    let pagination = query.pagination.normalise();

    let mut condition = Condition::all()
        .add(access_log::Column::Timestamp.gte(from))
        .add(access_log::Column::Timestamp.lt(to));
    condition = apply_scope(condition, &scope, access_log::Column::SiteId);

    if let Some(ip) = non_empty(&query.client_ip) {
        condition = condition.add(access_log::Column::ClientIp.eq(ip));
    }
    if let Some(method) = non_empty(&query.method) {
        condition = condition.add(access_log::Column::Method.eq(method.to_uppercase()));
    }
    if let Some(status) = query.status_code {
        if !(100..=599).contains(&status) {
            return Err(ApiError::BadRequest(
                "status_code must be between 100 and 599".to_string(),
            ));
        }
        condition = condition.add(access_log::Column::StatusCode.eq(status));
    }
    if let Some(class) = query.status_class {
        if ![1, 2, 3, 4, 5].contains(&class) {
            return Err(ApiError::BadRequest(
                "status_class must be one of 1, 2, 3, 4, 5".to_string(),
            ));
        }
        condition = condition
            .add(access_log::Column::StatusCode.gte(class * 100))
            .add(access_log::Column::StatusCode.lt((class + 1) * 100));
    }
    if let Some(host) = non_empty(&query.host) {
        condition = condition.add(access_log::Column::Host.eq(host));
    }
    if let Some(path) = non_empty(&query.path) {
        condition = condition.add(access_log::Column::Path.contains(path));
    }
    if let Some(cache_status) = non_empty(&query.cache_status) {
        condition = condition.add(access_log::Column::CacheStatus.eq(cache_status.to_lowercase()));
    }
    if let Some(country) = non_empty(&query.country_code) {
        condition = condition.add(access_log::Column::CountryCode.eq(country.to_uppercase()));
    }
    if let Some(min_latency) = query.min_latency_ms {
        condition = condition.add(access_log::Column::TotalLatencyMs.gte(min_latency));
    }

    let paginator = access_log::Entity::find()
        .filter(condition)
        .order_by_desc(access_log::Column::Timestamp)
        .paginate(&state.db, pagination.limit());

    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(pagination.index()).await?;
    Ok(Json(Page::new(rows, total, pagination)))
}

/// `DELETE /api/v1/logs/purge` — retention sweep, administrators only.
async fn purge(
    State(state): State<AppState>,
    current: AuthUser,
    Query(query): Query<PurgeQuery>,
) -> Result<Json<PurgeResult>, ApiError> {
    current.require_admin().map_err(ApiError::from)?;

    let days = query.older_than_days;
    if !(1..=3650).contains(&days) {
        return Err(ApiError::BadRequest(
            "older_than_days must be between 1 and 3650".to_string(),
        ));
    }
    let cutoff = Utc::now() - Duration::days(days);

    let site_filter = match non_empty(&query.site_id) {
        Some(raw) => Some(parse_uuid(&raw, "site id")?),
        None => None,
    };

    let mut security_condition = Condition::all().add(security_event::Column::Timestamp.lt(cutoff));
    let mut access_condition = Condition::all().add(access_log::Column::Timestamp.lt(cutoff));
    if let Some(id) = site_filter {
        security_condition = security_condition.add(security_event::Column::SiteId.eq(id));
        access_condition = access_condition.add(access_log::Column::SiteId.eq(id));
    }

    let security = security_event::Entity::delete_many()
        .filter(security_condition)
        .exec(&state.db)
        .await?;
    let access = access_log::Entity::delete_many()
        .filter(access_condition)
        .exec(&state.db)
        .await?;

    tracing::info!(
        cutoff = %cutoff,
        site_id = ?site_filter,
        security_events = security.rows_affected,
        access_logs = access.rows_affected,
        requested_by = %current.id,
        "log retention sweep completed"
    );

    Ok(Json(PurgeResult {
        deleted_security_events: security.rows_affected,
        deleted_access_logs: access.rows_affected,
        cutoff,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_window_is_the_last_day() {
        let (from, to) = resolve_window(&None, &None).unwrap();
        let span = to - from;
        assert!(span >= Duration::hours(DEFAULT_WINDOW_HOURS - 1));
        assert!(span <= Duration::hours(DEFAULT_WINDOW_HOURS + 1));
    }

    #[test]
    fn explicit_windows_are_validated() {
        let from = "2024-01-01T00:00:00Z".to_string();
        let to = "2024-01-02T00:00:00Z".to_string();
        assert!(resolve_window(&Some(from.clone()), &Some(to.clone())).is_ok());
        // Reversed range.
        assert!(resolve_window(&Some(to.clone()), &Some(from.clone())).is_err());
        // Window far beyond the ceiling.
        assert!(resolve_window(&Some("1970-01-01T00:00:00Z".to_string()), &Some(to)).is_err());
        // Unparsable input.
        assert!(resolve_window(&Some("yesterday".to_string()), &None).is_err());
    }

    #[test]
    fn retention_defaults_are_sane() {
        assert_eq!(default_retention_days(), 30);
    }
}
