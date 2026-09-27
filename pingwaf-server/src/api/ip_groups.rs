//! IP groups management: global or per-site IP blacklist/whitelist with
//! optional subscription source for dynamic updates.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, QueryTrait, Set,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::api::common::{
    load_site_read, non_empty, parse_uuid, Page, Pagination,
};
use crate::api::error::ApiError;
use crate::api::sites::touch_site;
use crate::api::state::AppState;
use crate::auth::AuthUser;
use crate::grpc::notify_config_changed;
use crate::models::{ip_group_sites, ip_groups, site};

/// Valid IP group actions.
pub mod ip_group_action {
    pub const BLOCK: &str = "block";
    pub const ALLOW: &str = "allow";

    pub fn is_valid(action: &str) -> bool {
        matches!(action, BLOCK | ALLOW)
    }
}

fn default_action() -> String {
    ip_group_action::BLOCK.to_string()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(flatten)]
    pub pagination: Pagination,
    pub action: Option<String>,
    pub is_global: Option<bool>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub ip_ranges: Vec<String>,
    #[serde(default = "default_action")]
    pub action: String,
    #[serde(default)]
    pub is_global: bool,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub sync_interval_minutes: Option<i32>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub ip_ranges: Option<Vec<String>>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub is_global: Option<bool>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub sync_interval_minutes: Option<i32>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct SetSitesRequest {
    pub site_ids: Vec<String>,
}

/// Routes contributed to `/api/v1`.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/ip-groups", get(list).post(create))
        .route(
            "/ip-groups/{group_id}",
            get(show).put(update).delete(remove),
        )
        .route(
            "/ip-groups/{group_id}/sites",
            get(list_sites).put(set_sites),
        )
        .route("/ip-groups/{group_id}/sync", post(sync_now))
}

/// Validates IP/CIDR ranges (basic format check).
fn validate_ip_ranges(ranges: &[String]) -> Result<Vec<String>, ApiError> {
    if ranges.len() > 10000 {
        return Err(ApiError::BadRequest(
            "an IP group may contain at most 10000 IP ranges".to_string(),
        ));
    }
    let mut out = Vec::with_capacity(ranges.len());
    for range in ranges {
        let trimmed = range.trim();
        if trimmed.is_empty() {
            continue;
        }
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
    Ok(out)
}

fn validate_name(name: &str) -> Result<(), ApiError> {
    if name.trim().is_empty() || name.len() > 200 {
        return Err(ApiError::BadRequest(
            "name must be 1-200 characters".to_string(),
        ));
    }
    Ok(())
}

/// Response type that includes the count of associated sites.
#[derive(Debug, serde::Serialize)]
pub struct IpGroupResponse {
    #[serde(flatten)]
    pub group: ip_groups::Model,
    pub site_count: usize,
}

/// `GET /api/v1/ip-groups`
async fn list(
    State(state): State<AppState>,
    current: AuthUser,
    Query(query): Query<ListQuery>,
) -> Result<Json<Page<IpGroupResponse>>, ApiError> {
    let pagination = query.pagination.normalise();

    let mut condition = sea_orm::Condition::all();
    if !current.is_admin() {
        let user_site_ids = site::Entity::find()
            .filter(site::Column::UserId.eq(current.id))
            .select_only()
            .column(site::Column::Id)
            .into_query();

        let linked_group_ids = ip_group_sites::Entity::find()
            .filter(ip_group_sites::Column::SiteId.in_subquery(user_site_ids))
            .select_only()
            .column(ip_group_sites::Column::IpGroupId)
            .into_query();

        condition = condition.add(
            ip_groups::Column::IsGlobal
                .eq(true)
                .or(ip_groups::Column::Id.in_subquery(linked_group_ids)),
        );
    }
    if let Some(ref action) = non_empty(&query.action) {
        if !ip_group_action::is_valid(action) {
            return Err(ApiError::BadRequest(format!(
                "unknown action '{action}'"
            )));
        }
        condition =
            condition.add(ip_groups::Column::Action.eq(action.as_str()));
    }
    if let Some(is_global) = query.is_global {
        condition = condition.add(ip_groups::Column::IsGlobal.eq(is_global));
    }
    if let Some(enabled) = query.enabled {
        condition = condition.add(ip_groups::Column::Enabled.eq(enabled));
    }

    let paginator = ip_groups::Entity::find()
        .filter(condition)
        .order_by_desc(ip_groups::Column::IsGlobal)
        .order_by_asc(ip_groups::Column::Name)
        .paginate(&state.db, pagination.limit());

    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(pagination.index()).await?;

    let group_ids: Vec<Uuid> = rows.iter().map(|g| g.id).collect();
    let site_counts: std::collections::HashMap<Uuid, usize> =
        if group_ids.is_empty() {
            std::collections::HashMap::new()
        } else {
            ip_group_sites::Entity::find()
                .filter(ip_group_sites::Column::IpGroupId.is_in(group_ids))
                .all(&state.db)
                .await?
                .into_iter()
                .fold(std::collections::HashMap::new(), |mut acc, assoc| {
                    *acc.entry(assoc.ip_group_id).or_insert(0usize) += 1;
                    acc
                })
        };

    let items: Vec<IpGroupResponse> = rows
        .into_iter()
        .map(|g| {
            let site_count = site_counts.get(&g.id).copied().unwrap_or(0);
            IpGroupResponse {
                group: g,
                site_count,
            }
        })
        .collect();
    Ok(Json(Page::new(items, total, pagination)))
}

/// `POST /api/v1/ip-groups`
async fn create(
    State(state): State<AppState>,
    _current: AuthUser,
    Json(payload): Json<CreateRequest>,
) -> Result<Response, ApiError> {
    validate_name(&payload.name)?;
    if !ip_group_action::is_valid(&payload.action) {
        return Err(ApiError::BadRequest(format!(
            "unknown action '{}'",
            payload.action
        )));
    }
    let ip_ranges = validate_ip_ranges(&payload.ip_ranges)?;

    let timestamp = chrono::Utc::now();
    let model = ip_groups::ActiveModel {
        id: Set(Uuid::new_v4()),
        name: Set(payload.name.trim().to_string()),
        description: Set(non_empty(&payload.description)),
        ip_ranges: Set(ip_ranges),
        action: Set(payload.action),
        is_global: Set(payload.is_global),
        source_url: Set(non_empty(&payload.source_url)),
        sync_interval_minutes: Set(payload.sync_interval_minutes),
        last_synced_at: Set(None),
        enabled: Set(payload.enabled),
        created_at: Set(timestamp),
        updated_at: Set(timestamp),
    }
    .insert(&state.db)
    .await?;

    tracing::info!(group_id = %model.id, name = %model.name, "IP group created");

    Ok((StatusCode::CREATED, Json(model)).into_response())
}

/// `GET /api/v1/ip-groups/{group_id}`
async fn show(
    State(state): State<AppState>,
    _current: AuthUser,
    Path(group_id): Path<String>,
) -> Result<Json<IpGroupResponse>, ApiError> {
    let target = parse_uuid(&group_id, "IP group id")?;
    let group = find_group(&state, target).await?;
    let site_count = ip_group_sites::Entity::find()
        .filter(ip_group_sites::Column::IpGroupId.eq(target))
        .count(&state.db)
        .await? as usize;
    Ok(Json(IpGroupResponse { group, site_count }))
}

/// `PUT /api/v1/ip-groups/{group_id}`
async fn update(
    State(state): State<AppState>,
    _current: AuthUser,
    Path(group_id): Path<String>,
    Json(payload): Json<UpdateRequest>,
) -> Result<Json<ip_groups::Model>, ApiError> {
    let target = parse_uuid(&group_id, "IP group id")?;
    let row = find_group(&state, target).await?;
    let mut active: ip_groups::ActiveModel = row.into();

    if let Some(name) = non_empty(&payload.name) {
        validate_name(&name)?;
        active.name = Set(name);
    }
    if let Some(desc) = payload.description {
        active.description = Set(non_empty(&Some(desc)));
    }
    if let Some(ranges) = payload.ip_ranges {
        active.ip_ranges = Set(validate_ip_ranges(&ranges)?);
    }
    if let Some(action) = non_empty(&payload.action) {
        if !ip_group_action::is_valid(&action) {
            return Err(ApiError::BadRequest(format!(
                "unknown action '{action}'"
            )));
        }
        active.action = Set(action);
    }
    if let Some(is_global) = payload.is_global {
        active.is_global = Set(is_global);
    }
    if let Some(url) = payload.source_url {
        active.source_url = Set(non_empty(&Some(url)));
    }
    if let Some(interval) = payload.sync_interval_minutes {
        active.sync_interval_minutes = Set(Some(interval));
    }
    if let Some(enabled) = payload.enabled {
        active.enabled = Set(enabled);
    }
    active.updated_at = Set(chrono::Utc::now());

    let updated = active.update(&state.db).await?;
    tracing::info!(group_id = %target, "IP group updated");

    Ok(Json(updated))
}

/// `DELETE /api/v1/ip-groups/{group_id}`
async fn remove(
    State(state): State<AppState>,
    _current: AuthUser,
    Path(group_id): Path<String>,
) -> Result<Response, ApiError> {
    let target = parse_uuid(&group_id, "IP group id")?;
    find_group(&state, target).await?;

    ip_groups::Entity::delete_by_id(target)
        .exec(&state.db)
        .await?;

    tracing::info!(group_id = %target, "IP group deleted");

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `GET /api/v1/ip-groups/{group_id}/sites`
async fn list_sites(
    State(state): State<AppState>,
    _current: AuthUser,
    Path(group_id): Path<String>,
) -> Result<Json<Vec<site::Model>>, ApiError> {
    let target = parse_uuid(&group_id, "IP group id")?;
    find_group(&state, target).await?;

    let associations = ip_group_sites::Entity::find()
        .filter(ip_group_sites::Column::IpGroupId.eq(target))
        .all(&state.db)
        .await?;

    let site_ids: Vec<Uuid> = associations.iter().map(|a| a.site_id).collect();
    if site_ids.is_empty() {
        return Ok(Json(vec![]));
    }

    let sites = site::Entity::find()
        .filter(site::Column::Id.is_in(site_ids))
        .all(&state.db)
        .await?;

    Ok(Json(sites))
}

/// `PUT /api/v1/ip-groups/{group_id}/sites`
async fn set_sites(
    State(state): State<AppState>,
    _current: AuthUser,
    Path(group_id): Path<String>,
    Json(payload): Json<SetSitesRequest>,
) -> Result<Response, ApiError> {
    let target = parse_uuid(&group_id, "IP group id")?;
    let group = find_group(&state, target).await?;

    if group.is_global {
        return Err(ApiError::BadRequest(
            "global IP groups cannot be associated with specific sites"
                .to_string(),
        ));
    }

    let site_ids: Vec<Uuid> = payload
        .site_ids
        .iter()
        .map(|s| parse_uuid(s, "site id"))
        .collect::<Result<Vec<_>, _>>()?;

    for sid in &site_ids {
        load_site_read(&state.db, *sid, &_current).await?;
    }

    ip_group_sites::Entity::delete_many()
        .filter(ip_group_sites::Column::IpGroupId.eq(target))
        .exec(&state.db)
        .await?;

    let timestamp = chrono::Utc::now();
    for sid in &site_ids {
        let assoc = ip_group_sites::ActiveModel {
            ip_group_id: Set(target),
            site_id: Set(*sid),
            created_at: Set(timestamp),
        };
        assoc.insert(&state.db).await?;
    }

    tracing::info!(
        group_id = %target,
        site_count = payload.site_ids.len(),
        "IP group sites updated"
    );

    for sid in &site_ids {
        touch_site(&state, *sid).await?;
        notify_config_changed(&state, *sid).await;
    }

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `POST /api/v1/ip-groups/{group_id}/sync` — trigger a manual sync from the
/// subscription source. Currently a stub that just updates `last_synced_at`.
async fn sync_now(
    State(state): State<AppState>,
    _current: AuthUser,
    Path(group_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let target = parse_uuid(&group_id, "IP group id")?;
    let group = find_group(&state, target).await?;

    if group.source_url.is_none() {
        return Err(ApiError::BadRequest(
            "IP group has no subscription source URL".to_string(),
        ));
    }

    let mut active: ip_groups::ActiveModel = group.into();
    active.last_synced_at = Set(Some(chrono::Utc::now()));
    active.updated_at = Set(chrono::Utc::now());
    let updated = active.update(&state.db).await?;

    tracing::info!(group_id = %target, "IP group sync triggered");

    Ok(Json(serde_json::json!({
        "id": updated.id,
        "synced_at": updated.last_synced_at,
        "ip_count": updated.ip_ranges.len(),
    })))
}

async fn find_group(
    state: &AppState,
    group_id: Uuid,
) -> Result<ip_groups::Model, ApiError> {
    ip_groups::Entity::find_by_id(group_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| {
            ApiError::NotFound(format!("IP group {group_id} not found"))
        })
}
