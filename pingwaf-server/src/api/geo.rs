//! Geo restriction management: per-site geographic blocking/allowing.

use axum::extract::{Path, State};
use axum::routing::get;
use axum::Json;
use axum::Router;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::Deserialize;
use uuid::Uuid;

use crate::api::common::{load_site_read, load_site_write, parse_uuid};
use crate::api::error::ApiError;
use crate::api::sites::touch_site;
use crate::api::state::AppState;
use crate::auth::AuthUser;
use crate::grpc::notify_config_changed;
use crate::models::{action, geo_rules};

/// Valid geo modes.
mod geo_mode {
    pub const BLOCK_LIST: &str = "block_list";
    pub const ALLOW_LIST: &str = "allow_list";

    pub fn is_valid(mode: &str) -> bool {
        matches!(mode, BLOCK_LIST | ALLOW_LIST)
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateGeoRequest {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub countries: Option<Vec<String>>,
    #[serde(default)]
    pub blocked_asns: Option<Vec<String>>,
    #[serde(default)]
    pub block_unknown: Option<bool>,
    #[serde(default)]
    pub action: Option<String>,
}

/// Routes contributed to `/api/v1`.
pub fn routes() -> Router<AppState> {
    Router::new().route("/sites/{site_id}/geo", get(get_geo).put(update_geo))
}

/// `GET /api/v1/sites/{site_id}/geo`
async fn get_geo(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
) -> Result<Json<geo_rules::Model>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    load_site_read(&state.db, id, &current).await?;

    let row = find_or_create(&state, id).await?;
    Ok(Json(row))
}

/// `PUT /api/v1/sites/{site_id}/geo`
async fn update_geo(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
    Json(payload): Json<UpdateGeoRequest>,
) -> Result<Json<geo_rules::Model>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    load_site_write(&state.db, id, &current).await?;

    let row = find_or_create(&state, id).await?;
    let mut active: geo_rules::ActiveModel = row.into();

    if let Some(enabled) = payload.enabled {
        active.enabled = Set(enabled);
    }
    if let Some(mode) = payload.mode {
        if !geo_mode::is_valid(&mode) {
            return Err(ApiError::BadRequest(format!(
                "invalid geo mode '{mode}'; expected block_list or allow_list"
            )));
        }
        active.mode = Set(mode);
    }
    if let Some(countries) = payload.countries {
        let normalised: Vec<String> = countries
            .iter()
            .map(|c| c.trim().to_uppercase())
            .filter(|c| {
                c.len() == 2 && c.chars().all(|ch| ch.is_ascii_alphabetic())
            })
            .collect();
        active.countries = Set(normalised);
    }
    if let Some(asns) = payload.blocked_asns {
        let normalised: Vec<String> = asns
            .iter()
            .map(|a| a.trim().to_uppercase())
            .filter(|a| !a.is_empty())
            .collect();
        active.blocked_asns = Set(normalised);
    }
    if let Some(block_unknown) = payload.block_unknown {
        active.block_unknown = Set(block_unknown);
    }
    if let Some(action_str) = payload.action {
        if !action::is_valid(&action_str) {
            return Err(ApiError::BadRequest(format!(
                "unknown action '{action_str}'"
            )));
        }
        active.action = Set(action_str);
    }
    active.updated_at = Set(chrono::Utc::now());

    let updated = active.update(&state.db).await?;
    tracing::info!(site_id = %id, "geo rules updated");
    touch_site(&state, id).await?;
    notify_config_changed(&state, id).await;

    Ok(Json(updated))
}

/// Finds the geo_rules row for a site, creating a default one if absent.
async fn find_or_create(
    state: &AppState,
    site_id: Uuid,
) -> Result<geo_rules::Model, ApiError> {
    if let Some(row) = geo_rules::Entity::find()
        .filter(geo_rules::Column::SiteId.eq(site_id))
        .one(&state.db)
        .await?
    {
        return Ok(row);
    }

    let now = chrono::Utc::now();
    let model = geo_rules::ActiveModel {
        id: Set(Uuid::new_v4()),
        site_id: Set(site_id),
        enabled: Set(false),
        mode: Set(geo_mode::BLOCK_LIST.to_string()),
        countries: Set(Vec::new()),
        blocked_asns: Set(Vec::new()),
        block_unknown: Set(false),
        action: Set(action::BLOCK.to_string()),
        updated_at: Set(now),
    }
    .insert(&state.db)
    .await?;

    Ok(model)
}
