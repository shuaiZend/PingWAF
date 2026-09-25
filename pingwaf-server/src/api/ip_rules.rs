//! IP access rules management: allow, block, challenge specific IPs/CIDRs.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
use axum::Router;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, Set,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::api::common::{
    load_site_read, load_site_write, non_empty, parse_uuid, Page, Pagination,
};
use crate::api::error::ApiError;
use crate::api::sites::touch_site;
use crate::api::state::AppState;
use crate::auth::AuthUser;
use crate::grpc::notify_config_changed;
use crate::models::ip_access_rules;

/// Valid IP access actions.
pub mod ip_action {
    pub const BLOCK: &str = "block";
    pub const CHALLENGE: &str = "challenge";
    pub const JS_CHALLENGE: &str = "js_challenge";
    pub const ALLOW: &str = "allow";

    pub fn is_valid(action: &str) -> bool {
        matches!(action, BLOCK | CHALLENGE | JS_CHALLENGE | ALLOW)
    }

    /// Maps to the proto `IpAccessAction` enum.
    pub fn to_proto(action: &str) -> i32 {
        match action {
            BLOCK => 0,
            CHALLENGE => 1,
            JS_CHALLENGE => 2,
            ALLOW => 3,
            _ => 0,
        }
    }
}

fn default_action() -> String {
    ip_action::BLOCK.to_string()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(flatten)]
    pub pagination: Pagination,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    pub name: String,
    pub ip_ranges: Vec<String>,
    #[serde(default = "default_action")]
    pub action: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub priority: i32,
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub ip_ranges: Option<Vec<String>>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub priority: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct BulkImportRequest {
    pub ip_ranges: Vec<String>,
    #[serde(default = "default_action")]
    pub action: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// Routes contributed to `/api/v1`.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/sites/{site_id}/ip-rules", get(list).post(create))
        .route(
            "/sites/{site_id}/ip-rules/{rule_id}",
            axum::routing::put(update).delete(remove),
        )
        .route(
            "/sites/{site_id}/ip-rules/bulk",
            axum::routing::post(bulk_import),
        )
}

/// Validates IP/CIDR ranges (basic format check).
fn validate_ip_ranges(ranges: &[String]) -> Result<Vec<String>, ApiError> {
    if ranges.is_empty() {
        return Err(ApiError::BadRequest(
            "at least one IP range is required".to_string(),
        ));
    }
    if ranges.len() > 1000 {
        return Err(ApiError::BadRequest(
            "a rule may contain at most 1000 IP ranges".to_string(),
        ));
    }
    let mut out = Vec::with_capacity(ranges.len());
    for range in ranges {
        let trimmed = range.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Basic validation: must look like an IP or CIDR
        let base = trimmed.split('/').next().unwrap_or(trimmed);
        if base.parse::<std::net::IpAddr>().is_err() {
            return Err(ApiError::BadRequest(format!(
                "invalid IP address or CIDR: '{trimmed}'"
            )));
        }
        if !out.contains(&trimmed.to_string()) {
            out.push(trimmed.to_string());
        }
    }
    if out.is_empty() {
        return Err(ApiError::BadRequest(
            "at least one valid IP range is required".to_string(),
        ));
    }
    Ok(out)
}

fn validate(name: &str, action: &str) -> Result<(), ApiError> {
    if name.trim().is_empty() || name.len() > 200 {
        return Err(ApiError::BadRequest(
            "name must be 1-200 characters".to_string(),
        ));
    }
    if !ip_action::is_valid(action) {
        return Err(ApiError::BadRequest(format!("unknown action '{action}'")));
    }
    Ok(())
}

/// `GET /api/v1/sites/{site_id}/ip-rules`
async fn list(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Page<ip_access_rules::Model>>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    load_site_read(&state.db, id, &current).await?;
    let pagination = query.pagination.normalise();

    let mut condition =
        sea_orm::Condition::all().add(ip_access_rules::Column::SiteId.eq(id));
    if let Some(enabled) = query.enabled {
        condition = condition.add(ip_access_rules::Column::Enabled.eq(enabled));
    }

    let paginator = ip_access_rules::Entity::find()
        .filter(condition)
        .order_by_asc(ip_access_rules::Column::Priority)
        .order_by_asc(ip_access_rules::Column::Name)
        .paginate(&state.db, pagination.limit());

    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(pagination.index()).await?;
    Ok(Json(Page::new(rows, total, pagination)))
}

/// `POST /api/v1/sites/{site_id}/ip-rules`
async fn create(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
    Json(payload): Json<CreateRequest>,
) -> Result<Response, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    load_site_write(&state.db, id, &current).await?;
    validate(&payload.name, &payload.action)?;
    let ip_ranges = validate_ip_ranges(&payload.ip_ranges)?;

    let timestamp = chrono::Utc::now();
    let model = ip_access_rules::ActiveModel {
        id: Set(Uuid::new_v4()),
        site_id: Set(id),
        name: Set(payload.name.trim().to_string()),
        ip_ranges: Set(ip_ranges),
        action: Set(payload.action),
        note: Set(non_empty(&payload.note)),
        enabled: Set(payload.enabled),
        priority: Set(payload.priority),
        created_at: Set(timestamp),
        updated_at: Set(timestamp),
    }
    .insert(&state.db)
    .await?;

    tracing::info!(site_id = %id, rule_id = %model.id, "IP access rule created");
    touch_site(&state, id).await?;
    notify_config_changed(&state, id).await;

    Ok((StatusCode::CREATED, Json(model)).into_response())
}

/// `PUT /api/v1/sites/{site_id}/ip-rules/{rule_id}`
async fn update(
    State(state): State<AppState>,
    current: AuthUser,
    Path((site_id, rule_id)): Path<(String, String)>,
    Json(payload): Json<UpdateRequest>,
) -> Result<Json<ip_access_rules::Model>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    let target = parse_uuid(&rule_id, "IP rule id")?;
    load_site_write(&state.db, id, &current).await?;

    let row = find(&state, id, target).await?;
    let mut active: ip_access_rules::ActiveModel = row.into();

    if let Some(name) = non_empty(&payload.name) {
        if name.len() > 200 {
            return Err(ApiError::BadRequest(
                "name must be at most 200 characters".to_string(),
            ));
        }
        active.name = Set(name);
    }
    if let Some(ranges) = payload.ip_ranges {
        active.ip_ranges = Set(validate_ip_ranges(&ranges)?);
    }
    if let Some(action) = non_empty(&payload.action) {
        if !ip_action::is_valid(&action) {
            return Err(ApiError::BadRequest(format!(
                "unknown action '{action}'"
            )));
        }
        active.action = Set(action);
    }
    if let Some(note) = payload.note {
        active.note = Set(non_empty(&Some(note)));
    }
    if let Some(enabled) = payload.enabled {
        active.enabled = Set(enabled);
    }
    if let Some(priority) = payload.priority {
        active.priority = Set(priority);
    }
    active.updated_at = Set(chrono::Utc::now());

    let updated = active.update(&state.db).await?;
    tracing::info!(site_id = %id, rule_id = %target, "IP access rule updated");
    touch_site(&state, id).await?;
    notify_config_changed(&state, id).await;

    Ok(Json(updated))
}

/// `DELETE /api/v1/sites/{site_id}/ip-rules/{rule_id}`
async fn remove(
    State(state): State<AppState>,
    current: AuthUser,
    Path((site_id, rule_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    let target = parse_uuid(&rule_id, "IP rule id")?;
    load_site_write(&state.db, id, &current).await?;
    find(&state, id, target).await?;

    ip_access_rules::Entity::delete_by_id(target)
        .exec(&state.db)
        .await?;

    tracing::info!(site_id = %id, rule_id = %target, "IP access rule deleted");
    touch_site(&state, id).await?;
    notify_config_changed(&state, id).await;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `POST /api/v1/sites/{site_id}/ip-rules/bulk`
async fn bulk_import(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
    Json(payload): Json<BulkImportRequest>,
) -> Result<Response, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    load_site_write(&state.db, id, &current).await?;

    if !ip_action::is_valid(&payload.action) {
        return Err(ApiError::BadRequest(format!(
            "unknown action '{}'",
            payload.action
        )));
    }
    let ip_ranges = validate_ip_ranges(&payload.ip_ranges)?;

    let timestamp = chrono::Utc::now();
    let model = ip_access_rules::ActiveModel {
        id: Set(Uuid::new_v4()),
        site_id: Set(id),
        name: Set(format!("bulk-import-{}", timestamp.format("%Y%m%d%H%M%S"))),
        ip_ranges: Set(ip_ranges.clone()),
        action: Set(payload.action),
        note: Set(non_empty(&payload.note)),
        enabled: Set(payload.enabled),
        priority: Set(0),
        created_at: Set(timestamp),
        updated_at: Set(timestamp),
    }
    .insert(&state.db)
    .await?;

    tracing::info!(site_id = %id, rule_id = %model.id, count = ip_ranges.len(), "bulk IP import");
    touch_site(&state, id).await?;
    notify_config_changed(&state, id).await;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": model.id,
            "imported": ip_ranges.len(),
        })),
    )
        .into_response())
}

async fn find(
    state: &AppState,
    site_id: Uuid,
    rule_id: Uuid,
) -> Result<ip_access_rules::Model, ApiError> {
    ip_access_rules::Entity::find_by_id(rule_id)
        .filter(ip_access_rules::Column::SiteId.eq(site_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| {
            ApiError::NotFound(format!("IP access rule {rule_id} not found"))
        })
}
