//! Aggregation endpoints backing the dashboard charts: traffic over time, the
//! rules that fire most often, the loudest client IPs and the hottest paths.
//!
//! The queries are hand-written SQL because sea-query's builder cannot express
//! `FILTER (WHERE …)` aggregates or `date_trunc` bucketing without giving up
//! most of the readability.

use axum::extract::{Query, State};
use axum::routing::get;
use axum::Json;
use axum::Router;
use chrono::{DateTime, Duration, Utc};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, TryGetable, Value};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::common::{
    load_site_read, non_empty, parse_optional_datetime, parse_uuid, query_all,
};
use crate::api::error::ApiError;
use crate::api::state::AppState;
use crate::auth::AuthUser;
use crate::models::site;

/// Default look-back window for every analytics endpoint.
const DEFAULT_WINDOW_HOURS: i64 = 24;
/// Analytics scans more rows than the log endpoints, so the ceiling is tighter.
const MAX_WINDOW_DAYS: i64 = 31;
/// Upper bound for the `limit` query parameter.
const MAX_LIMIT: i64 = 100;
/// Buckets returned by `requests-over-time` are capped so that a year-long
/// minute-granularity request cannot blow up the response.
const MAX_BUCKETS: i64 = 8 * 1024;

/// `date_trunc` fields the API accepts.
const INTERVALS: [&str; 4] = ["minute", "hour", "day", "week"];

#[derive(Debug, Deserialize)]
pub struct RangeQuery {
    pub site_id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    /// `minute`, `hour` (default), `day` or `week`.
    #[serde(default = "default_interval")]
    pub interval: String,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_interval() -> String {
    "hour".to_string()
}

fn default_limit() -> i64 {
    10
}

/// Resolved query parameters shared by every endpoint below.
struct Resolved {
    /// `None` means "every site the caller may see" (administrators only).
    site_id: Option<Uuid>,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    interval: String,
    limit: i64,
}

impl Resolved {
    /// Builds the `WHERE` fragment plus its bound values.
    fn filter(&self, table: &str) -> (String, Vec<Value>) {
        let mut sql =
            format!("{table}.timestamp >= $1 AND {table}.timestamp < $2");
        let mut values: Vec<Value> = vec![self.from.into(), self.to.into()];
        if let Some(id) = self.site_id {
            sql.push_str(" AND site_id = $3");
            values.push(id.into());
        }
        (sql, values)
    }

    /// Index of the next free `$n` placeholder.
    fn next_placeholder(&self) -> usize {
        if self.site_id.is_some() {
            4
        } else {
            3
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Summary {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub site_id: Option<Uuid>,
    pub requests: i64,
    pub unique_ips: i64,
    pub cache_hits: i64,
    pub cache_hit_rate: f64,
    pub avg_latency_ms: i64,
    pub max_latency_ms: i64,
    pub client_errors: i64,
    pub server_errors: i64,
    pub security_events: i64,
    pub blocked_requests: i64,
    pub distinct_attackers: i64,
    pub rules_triggered: i64,
}

#[derive(Debug, Serialize)]
pub struct TimeBucket {
    pub bucket: DateTime<Utc>,
    pub requests: i64,
    pub cache_hits: i64,
    pub client_errors: i64,
    pub server_errors: i64,
    pub avg_latency_ms: i64,
    pub blocked: i64,
}

#[derive(Debug, Serialize)]
pub struct TopRule {
    pub rule_id: String,
    pub rule_name: String,
    pub action: String,
    pub hits: i64,
    pub unique_ips: i64,
}

#[derive(Debug, Serialize)]
pub struct TopIp {
    pub client_ip: String,
    pub country_code: Option<String>,
    pub requests: i64,
    pub blocked: i64,
}

#[derive(Debug, Serialize)]
pub struct TopPath {
    pub path: String,
    pub requests: i64,
    pub cache_hits: i64,
    pub avg_latency_ms: i64,
}

#[derive(Debug, Serialize)]
pub struct StatusCodeCount {
    pub status_code: i32,
    pub requests: i64,
}

#[derive(Debug, Serialize)]
pub struct SiteOverview {
    pub site_id: Uuid,
    pub name: String,
    pub domain: String,
    pub status: String,
    pub plan: String,
    pub requests: i64,
    pub blocked: i64,
}

/// Routes contributed to `/api/v1`.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/analytics/summary", get(summary))
        .route("/analytics/requests-over-time", get(requests_over_time))
        .route("/analytics/top-rules", get(top_rules))
        .route("/analytics/top-ips", get(top_ips))
        .route("/analytics/top-paths", get(top_paths))
        .route("/analytics/status-codes", get(status_codes))
        .route("/analytics/sites", get(sites_overview))
}

/// Validates the incoming range and resolves tenant visibility.
async fn resolve(
    state: &AppState,
    current: &AuthUser,
    query: &RangeQuery,
) -> Result<Resolved, ApiError> {
    let site_id = match non_empty(&query.site_id) {
        Some(raw) => {
            let id = parse_uuid(&raw, "site id")?;
            // `load_site_read` returns 404 for sites the caller may not see.
            load_site_read(&state.db, id, current).await?;
            Some(id)
        },
        None if current.is_admin() => None,
        None => {
            // Non-admins may still see everything they own; pick the single site
            // when they only have one, otherwise require the parameter.
            let owned = site::Entity::find()
                .filter(site::Column::UserId.eq(current.id))
                .all(&state.db)
                .await?;
            match owned.len() {
                0 => {
                    return Err(ApiError::NotFound(
                        "this account does not own any site".to_string(),
                    ))
                }
                1 => Some(owned[0].id),
                _ => {
                    return Err(ApiError::BadRequest(
                        "site_id is required when the account owns more than one site".to_string(),
                    ))
                }
            }
        },
    };

    let to = match parse_optional_datetime(&query.to, "to")? {
        Some(instant) => instant,
        None => Utc::now(),
    };
    let from = match parse_optional_datetime(&query.from, "from")? {
        Some(instant) => instant,
        None => to - Duration::hours(DEFAULT_WINDOW_HOURS),
    };
    if from >= to {
        return Err(ApiError::BadRequest(
            "'from' must be earlier than 'to'".to_string(),
        ));
    }
    if from < to - Duration::days(MAX_WINDOW_DAYS) {
        return Err(ApiError::BadRequest(format!(
            "the analytics window may not exceed {MAX_WINDOW_DAYS} days"
        )));
    }

    let interval = query.interval.trim().to_lowercase();
    if !INTERVALS.contains(&interval.as_str()) {
        return Err(ApiError::BadRequest(format!(
            "interval must be one of {}",
            INTERVALS.join(", ")
        )));
    }

    let limit = query.limit.clamp(1, MAX_LIMIT);

    Ok(Resolved {
        site_id,
        from,
        to,
        interval,
        limit,
    })
}

/// `GET /api/v1/analytics/summary`
async fn summary(
    State(state): State<AppState>,
    current: AuthUser,
    Query(query): Query<RangeQuery>,
) -> Result<Json<Summary>, ApiError> {
    let resolved = resolve(&state, &current, &query).await?;

    let (access_where, access_values) = resolved.filter("access_logs");
    let access_sql = format!(
        "SELECT COUNT(*) AS requests, \
                COUNT(DISTINCT client_ip) AS unique_ips, \
                COUNT(*) FILTER (WHERE cache_status = 'hit') AS cache_hits, \
                COALESCE(AVG(total_latency_ms), 0)::BIGINT AS avg_latency_ms, \
                COALESCE(MAX(total_latency_ms), 0)::BIGINT AS max_latency_ms, \
                COUNT(*) FILTER (WHERE status_code >= 400 AND status_code < 500) AS client_errors, \
                COUNT(*) FILTER (WHERE status_code >= 500) AS server_errors \
         FROM access_logs WHERE {access_where}"
    );
    let access = query_all(&state.db, &access_sql, access_values)
        .await?
        .into_iter()
        .next();

    let (event_where, event_values) = resolved.filter("security_events");
    let event_sql = format!(
        "SELECT COUNT(*) AS security_events, \
                COUNT(*) FILTER (WHERE action = 'block') AS blocked_requests, \
                COUNT(DISTINCT client_ip) AS distinct_attackers, \
                COUNT(DISTINCT rule_id) AS rules_triggered \
         FROM security_events WHERE {event_where}"
    );
    let events = query_all(&state.db, &event_sql, event_values)
        .await?
        .into_iter()
        .next();

    let requests: i64 = read(&access, "requests");
    let cache_hits: i64 = read(&access, "cache_hits");
    let cache_hit_rate = if requests > 0 {
        cache_hits as f64 / requests as f64
    } else {
        0.0
    };

    Ok(Json(Summary {
        from: resolved.from,
        to: resolved.to,
        site_id: resolved.site_id,
        requests,
        unique_ips: read(&access, "unique_ips"),
        cache_hits,
        cache_hit_rate,
        avg_latency_ms: read(&access, "avg_latency_ms"),
        max_latency_ms: read(&access, "max_latency_ms"),
        client_errors: read(&access, "client_errors"),
        server_errors: read(&access, "server_errors"),
        security_events: read(&events, "security_events"),
        blocked_requests: read(&events, "blocked_requests"),
        distinct_attackers: read(&events, "distinct_attackers"),
        rules_triggered: read(&events, "rules_triggered"),
    }))
}

/// `GET /api/v1/analytics/requests-over-time`
async fn requests_over_time(
    State(state): State<AppState>,
    current: AuthUser,
    Query(query): Query<RangeQuery>,
) -> Result<Json<Vec<TimeBucket>>, ApiError> {
    let resolved = resolve(&state, &current, &query).await?;

    // Reject ranges that would produce an absurd number of buckets before
    // touching the database.
    let bucket_seconds = match resolved.interval.as_str() {
        "minute" => 60,
        "hour" => 3_600,
        "day" => 86_400,
        _ => 604_800, // week
    };
    let span = (resolved.to - resolved.from).num_seconds().max(1);
    if span / bucket_seconds > MAX_BUCKETS {
        return Err(ApiError::BadRequest(format!(
            "the requested range would produce too many {0} buckets; widen the interval",
            resolved.interval
        )));
    }

    // The interval is validated against a fixed whitelist above, so it is safe
    // to inline; binding it would make PostgreSQL infer `unknown` for the type.
    let interval = resolved.interval.clone();
    let (where_sql, values) = resolved.filter("access_logs");
    let sql = format!(
        "SELECT date_trunc('{interval}', timestamp) AS bucket, \
                COUNT(*) AS requests, \
                COUNT(*) FILTER (WHERE cache_status = 'hit') AS cache_hits, \
                COUNT(*) FILTER (WHERE status_code >= 400 AND status_code < 500) AS client_errors, \
                COUNT(*) FILTER (WHERE status_code >= 500) AS server_errors, \
                COALESCE(AVG(total_latency_ms), 0)::BIGINT AS avg_latency_ms \
         FROM access_logs WHERE {where_sql} \
         GROUP BY bucket ORDER BY bucket ASC"
    );

    let rows = query_all(&state.db, &sql, values).await?;
    let mut buckets: Vec<TimeBucket> = Vec::with_capacity(rows.len());
    for row in &rows {
        buckets.push(TimeBucket {
            bucket: try_get::<DateTime<Utc>>(row, "bucket")
                .unwrap_or_else(Utc::now),
            requests: get_value::<i64>(row, "requests"),
            cache_hits: get_value::<i64>(row, "cache_hits"),
            client_errors: get_value::<i64>(row, "client_errors"),
            server_errors: get_value::<i64>(row, "server_errors"),
            avg_latency_ms: get_value::<i64>(row, "avg_latency_ms"),
            blocked: 0,
        });
    }

    // Fold the blocked counts in from the security events table so the chart can
    // overlay attacks on traffic with a single request.
    let (event_where, event_values) = resolved.filter("security_events");
    let event_sql = format!(
        "SELECT date_trunc('{interval}', timestamp) AS bucket, COUNT(*) AS blocked \
         FROM security_events \
         WHERE {event_where} AND action <> 'allow' \
         GROUP BY bucket ORDER BY bucket ASC"
    );
    if let Ok(event_rows) = query_all(&state.db, &event_sql, event_values).await
    {
        for row in &event_rows {
            let Some(bucket) = try_get::<DateTime<Utc>>(row, "bucket") else {
                continue;
            };
            let blocked = get_value::<i64>(row, "blocked");
            if let Some(target) =
                buckets.iter_mut().find(|item| item.bucket == bucket)
            {
                target.blocked = blocked;
            }
        }
    }

    Ok(Json(buckets))
}

/// `GET /api/v1/analytics/top-rules`
async fn top_rules(
    State(state): State<AppState>,
    current: AuthUser,
    Query(query): Query<RangeQuery>,
) -> Result<Json<Vec<TopRule>>, ApiError> {
    let resolved = resolve(&state, &current, &query).await?;
    let (where_sql, mut values) = resolved.filter("security_events");
    let limit_placeholder = resolved.next_placeholder();
    values.push(Value::from(resolved.limit));

    let sql = format!(
        "SELECT COALESCE(rule_id, 'unknown') AS rule_id, \
                COALESCE(rule_name, '') AS rule_name, \
                action, \
                COUNT(*) AS hits, \
                COUNT(DISTINCT client_ip) AS unique_ips \
         FROM security_events WHERE {where_sql} \
         GROUP BY rule_id, rule_name, action \
         ORDER BY hits DESC, rule_id ASC \
         LIMIT ${limit_placeholder}"
    );

    let rows = query_all(&state.db, &sql, values).await?;
    Ok(Json(
        rows.iter()
            .map(|row| TopRule {
                rule_id: get_value::<String>(row, "rule_id"),
                rule_name: get_value::<String>(row, "rule_name"),
                action: get_value::<String>(row, "action"),
                hits: get_value::<i64>(row, "hits"),
                unique_ips: get_value::<i64>(row, "unique_ips"),
            })
            .collect(),
    ))
}

/// `GET /api/v1/analytics/top-ips`
async fn top_ips(
    State(state): State<AppState>,
    current: AuthUser,
    Query(query): Query<RangeQuery>,
) -> Result<Json<Vec<TopIp>>, ApiError> {
    let resolved = resolve(&state, &current, &query).await?;
    let (where_sql, mut values) = resolved.filter("access_logs");
    let limit_placeholder = resolved.next_placeholder();
    values.push(Value::from(resolved.limit));

    let sql = format!(
        "SELECT client_ip, \
                MAX(country_code) AS country_code, \
                COUNT(*) AS requests, \
                COUNT(*) FILTER (WHERE status_code = 403 OR status_code = 429) AS blocked \
         FROM access_logs WHERE {where_sql} \
         GROUP BY client_ip ORDER BY requests DESC LIMIT ${limit_placeholder}"
    );

    let rows = query_all(&state.db, &sql, values).await?;
    Ok(Json(
        rows.iter()
            .map(|row| TopIp {
                client_ip: get_value::<String>(row, "client_ip"),
                country_code: try_get::<String>(row, "country_code"),
                requests: get_value::<i64>(row, "requests"),
                blocked: get_value::<i64>(row, "blocked"),
            })
            .collect(),
    ))
}

/// `GET /api/v1/analytics/top-paths`
async fn top_paths(
    State(state): State<AppState>,
    current: AuthUser,
    Query(query): Query<RangeQuery>,
) -> Result<Json<Vec<TopPath>>, ApiError> {
    let resolved = resolve(&state, &current, &query).await?;
    let (where_sql, mut values) = resolved.filter("access_logs");
    let limit_placeholder = resolved.next_placeholder();
    values.push(Value::from(resolved.limit));

    let sql = format!(
        "SELECT COALESCE(path, '/') AS path, \
                COUNT(*) AS requests, \
                COUNT(*) FILTER (WHERE cache_status = 'hit') AS cache_hits, \
                COALESCE(AVG(total_latency_ms), 0)::BIGINT AS avg_latency_ms \
         FROM access_logs WHERE {where_sql} \
         GROUP BY path ORDER BY requests DESC LIMIT ${limit_placeholder}"
    );

    let rows = query_all(&state.db, &sql, values).await?;
    Ok(Json(
        rows.iter()
            .map(|row| TopPath {
                path: get_value::<String>(row, "path"),
                requests: get_value::<i64>(row, "requests"),
                cache_hits: get_value::<i64>(row, "cache_hits"),
                avg_latency_ms: get_value::<i64>(row, "avg_latency_ms"),
            })
            .collect(),
    ))
}

/// `GET /api/v1/analytics/status-codes`
async fn status_codes(
    State(state): State<AppState>,
    current: AuthUser,
    Query(query): Query<RangeQuery>,
) -> Result<Json<Vec<StatusCodeCount>>, ApiError> {
    let resolved = resolve(&state, &current, &query).await?;
    let (where_sql, values) = resolved.filter("access_logs");

    let sql = format!(
        "SELECT status_code, COUNT(*) AS requests \
         FROM access_logs WHERE {where_sql} AND status_code IS NOT NULL \
         GROUP BY status_code ORDER BY status_code ASC"
    );

    let rows = query_all(&state.db, &sql, values).await?;
    Ok(Json(
        rows.iter()
            .map(|row| StatusCodeCount {
                status_code: get_value::<i32>(row, "status_code"),
                requests: get_value::<i64>(row, "requests"),
            })
            .collect(),
    ))
}

/// `GET /api/v1/analytics/sites` — traffic per site, used by the home page.
async fn sites_overview(
    State(state): State<AppState>,
    current: AuthUser,
    Query(query): Query<RangeQuery>,
) -> Result<Json<Vec<SiteOverview>>, ApiError> {
    let resolved = resolve(&state, &current, &query).await?;

    let sites = match resolved.site_id {
        Some(id) => vec![load_site_read(&state.db, id, &current).await?],
        None if current.is_admin() => {
            site::Entity::find().all(&state.db).await?
        },
        None => {
            site::Entity::find()
                .filter(site::Column::UserId.eq(current.id))
                .all(&state.db)
                .await?
        },
    };

    let mut out = Vec::with_capacity(sites.len());
    for row in sites {
        // Two correlated counts per site: traffic from the access log and
        // blocked traffic from the security event log.
        let sql = "SELECT \
                     (SELECT COUNT(*) FROM access_logs \
                      WHERE site_id = $1 AND timestamp >= $2 AND timestamp < $3) AS requests, \
                     (SELECT COUNT(*) FROM security_events \
                      WHERE site_id = $1 AND timestamp >= $2 AND timestamp < $3 \
                        AND action = 'block') AS blocked";
        let values: Vec<Value> =
            vec![row.id.into(), resolved.from.into(), resolved.to.into()];

        let counts =
            query_all(&state.db, sql, values).await?.into_iter().next();
        out.push(SiteOverview {
            site_id: row.id,
            name: row.name,
            domain: row.domain,
            status: row.status,
            plan: row.plan,
            requests: read(&counts, "requests"),
            blocked: read(&counts, "blocked"),
        });
    }

    Ok(Json(out))
}

/// Reads a column, falling back to the type's default when the aggregate
/// produced no row (which happens for an empty window).
fn read<T>(row: &Option<sea_orm::QueryResult>, column: &str) -> T
where
    T: TryGetable + Default,
{
    match row {
        Some(row) => get_value::<T>(row, column),
        None => T::default(),
    }
}

/// Reads a column from a row that is guaranteed to exist.
fn get_value<T>(row: &sea_orm::QueryResult, column: &str) -> T
where
    T: TryGetable + Default,
{
    row.try_get::<T>("", column).unwrap_or_default()
}

/// Same as [`get_value`] but without the default fallback, for nullable columns.
fn try_get<T>(row: &sea_orm::QueryResult, column: &str) -> Option<T>
where
    T: TryGetable,
{
    row.try_get::<T>("", column).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolved(site_id: Option<Uuid>) -> Resolved {
        Resolved {
            site_id,
            from: Utc::now() - Duration::hours(1),
            to: Utc::now(),
            interval: "hour".to_string(),
            limit: 10,
        }
    }

    #[test]
    fn filter_placeholders_shift_with_the_site_scope() {
        let open = resolved(None);
        let (sql, values) = open.filter("access_logs");
        assert_eq!(
            sql,
            "access_logs.timestamp >= $1 AND access_logs.timestamp < $2"
        );
        assert_eq!(values.len(), 2);
        assert_eq!(open.next_placeholder(), 3);

        let scoped = resolved(Some(Uuid::new_v4()));
        let (sql, values) = scoped.filter("access_logs");
        assert!(sql.ends_with("AND site_id = $3"));
        assert_eq!(values.len(), 3);
        assert_eq!(scoped.next_placeholder(), 4);
    }

    #[test]
    fn intervals_are_whitelisted() {
        assert!(INTERVALS.contains(&"hour"));
        assert!(INTERVALS.contains(&"week"));
        assert!(!INTERVALS.contains(&"1 hour; DROP TABLE users"));
        assert_eq!(default_interval(), "hour");
        assert_eq!(default_limit(), 10);
    }
}
