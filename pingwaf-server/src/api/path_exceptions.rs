//! Per-site path exceptions: request paths that skip selected protection
//! layers (WAF, bot protection, access rules, ...).
//!
//! Safety rule: the site must be in observation mode (`waf_settings.mode =
//! monitor`) to configure them. A bypass that silently skips enforcement on a
//! blocking site is exactly the kind of change that never gets removed again;
//! requiring monitor keeps the site's traffic logged and unenforced while the
//! operator narrows down what the exception must cover. The data plane re-checks
//! the mode before honouring a rule, so a stale bundle cannot punch a hole after
//! the site returns to `block`.

use axum::extract::{Path, State};
use axum::routing::get;
use axum::Json;
use axum::Router;
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::common::{load_site_read, load_site_write, parse_uuid};
use crate::api::error::ApiError;
use crate::api::sites::touch_site;
use crate::api::state::AppState;
use crate::auth::AuthUser;
use crate::grpc::notify_config_changed;
use crate::models::{path_exceptions, waf_settings};

/// Per-site cap: small by nature, and the whole list ships in every bundle.
pub const MAX_PATH_EXCEPTIONS: usize = 64;
pub const MAX_PATH_LEN: usize = 512;
pub const MAX_NOTE_LEN: usize = 200;

/// Layer tokens the data plane understands. `*` means every layer.
pub const SKIP_LAYERS: [&str; 9] = [
    "*",
    "waf",
    "bot",
    "access",
    "basic_auth",
    "mtls",
    "ai_bot",
    "rate_limit",
    "challenge",
];

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/sites/{site_id}/path-exceptions",
            get(list_exceptions).post(create_exception),
        )
        .route(
            "/sites/{site_id}/path-exceptions/{id}",
            get(show_exception)
                .put(update_exception)
                .delete(delete_exception),
        )
}

#[derive(Debug, Deserialize)]
pub struct PathExceptionRequest {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub match_type: Option<String>,
    #[serde(default)]
    pub skip_layers: Option<Vec<String>>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub log_hits: Option<bool>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct PathExceptionView {
    pub id: Uuid,
    pub site_id: Uuid,
    pub path: String,
    pub match_type: String,
    pub skip_layers: Vec<String>,
    pub note: Option<String>,
    pub log_hits: bool,
    pub enabled: bool,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
}

impl From<path_exceptions::Model> for PathExceptionView {
    fn from(model: path_exceptions::Model) -> Self {
        Self {
            id: model.id,
            site_id: model.site_id,
            path: model.path,
            match_type: model.match_type,
            skip_layers: model.skip_layers,
            note: model.note,
            log_hits: model.log_hits,
            enabled: model.enabled,
            created_at: model.created_at,
            updated_at: model.updated_at,
        }
    }
}

/// Validates and normalises the match type (`prefix` is the default).
fn validate_match_type(raw: Option<&str>) -> Result<String, ApiError> {
    let value = raw
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("prefix");
    match value {
        "prefix" | "exact" | "wildcard" => Ok(value.to_string()),
        other => Err(ApiError::BadRequest(format!(
            "unknown match_type '{other}'; expected one of prefix, exact, wildcard"
        ))),
    }
}

/// A path must be a request path: leading `/`, no whitespace or control
/// characters, and `wildcard` only supports a single trailing `*`.
fn validate_path(raw: &str, match_type: &str) -> Result<String, ApiError> {
    let path = raw.trim();
    if path.is_empty() {
        return Err(ApiError::BadRequest("path must not be empty".to_string()));
    }
    if !path.starts_with('/') {
        return Err(ApiError::BadRequest(format!(
            "invalid path '{path}': must start with '/'"
        )));
    }
    if path.len() > MAX_PATH_LEN {
        return Err(ApiError::BadRequest(format!(
            "invalid path: at most {MAX_PATH_LEN} characters"
        )));
    }
    if path.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(ApiError::BadRequest(format!(
            "invalid path '{path}': whitespace and control characters are not allowed"
        )));
    }
    if match_type == "wildcard" {
        if !path.ends_with('*') || path.matches('*').count() != 1 {
            return Err(ApiError::BadRequest(format!(
                "invalid wildcard path '{path}': 'wildcard' only supports one trailing '*'"
            )));
        }
        if path == "*" {
            return Err(ApiError::BadRequest(
                "use path '/' with match_type prefix instead of '*'"
                    .to_string(),
            ));
        }
    } else if path.contains('*') {
        return Err(ApiError::BadRequest(format!(
            "invalid path '{path}': '*' is only allowed with match_type 'wildcard'"
        )));
    }
    Ok(path.to_string())
}

/// `*` collapses the list; unknown tokens are rejected rather than ignored.
fn validate_skip_layers(raw: &[String]) -> Result<Vec<String>, ApiError> {
    if raw.is_empty() {
        return Err(ApiError::BadRequest(
            "skip_layers must not be empty; use [\"*\"] to skip every layer"
                .to_string(),
        ));
    }
    let mut layers = Vec::with_capacity(raw.len());
    for layer in raw {
        let layer = layer.trim();
        if layer.is_empty() {
            continue;
        }
        if !SKIP_LAYERS.contains(&layer) {
            return Err(ApiError::BadRequest(format!(
                "unknown skip layer '{layer}'; expected one of {}",
                SKIP_LAYERS.join(", ")
            )));
        }
        if layer == "*" {
            return Ok(vec!["*".to_string()]);
        }
        if !layers.iter().any(|item| item == layer) {
            layers.push(layer.to_string());
        }
    }
    if layers.is_empty() {
        return Err(ApiError::BadRequest(
            "skip_layers must name at least one layer or '*'".to_string(),
        ));
    }
    Ok(layers)
}

fn validate_note(raw: Option<&str>) -> Result<Option<String>, ApiError> {
    let note = raw.map(str::trim).filter(|value| !value.is_empty());
    if let Some(note) = note {
        if note.chars().count() > MAX_NOTE_LEN {
            return Err(ApiError::BadRequest(format!(
                "note accepts at most {MAX_NOTE_LEN} characters"
            )));
        }
    }
    Ok(note.map(str::to_string))
}

/// The safety gate. A missing `waf_settings` row means the data plane falls
/// back to monitor (`grpc/config.rs` floors a missing mode to monitor), so it
/// is treated the same as an explicit `monitor`.
async fn require_observation_mode(
    state: &AppState,
    site_id: Uuid,
) -> Result<(), ApiError> {
    let settings = waf_settings::Entity::find()
        .filter(waf_settings::Column::SiteId.eq(site_id))
        .one(&state.db)
        .await?;
    let mode = settings
        .map(|row| row.mode)
        .unwrap_or_else(|| "monitor".to_string());
    if mode != "monitor" {
        return Err(ApiError::Conflict(format!(
            "path exceptions require the site to be in observation mode; \
             current WAF mode is '{mode}' (set it to 'monitor' first)"
        )));
    }
    Ok(())
}

/// `GET /api/v1/sites/{site_id}/path-exceptions`
async fn list_exceptions(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
) -> Result<Json<Vec<PathExceptionView>>, ApiError> {
    let site_id = parse_uuid(&site_id, "site id")?;
    load_site_read(&state.db, site_id, &current).await?;
    let rows = path_exceptions::Entity::find()
        .filter(path_exceptions::Column::SiteId.eq(site_id))
        .order_by_asc(path_exceptions::Column::Path)
        .all(&state.db)
        .await?;
    Ok(Json(
        rows.into_iter().map(PathExceptionView::from).collect(),
    ))
}

/// `GET /api/v1/sites/{site_id}/path-exceptions/{id}`
async fn show_exception(
    State(state): State<AppState>,
    current: AuthUser,
    Path((site_id, id)): Path<(String, String)>,
) -> Result<Json<PathExceptionView>, ApiError> {
    let site_id = parse_uuid(&site_id, "site id")?;
    let id = parse_uuid(&id, "path exception id")?;
    load_site_read(&state.db, site_id, &current).await?;
    let row = find_exception(&state, site_id, id).await?;
    Ok(Json(PathExceptionView::from(row)))
}

/// `POST /api/v1/sites/{site_id}/path-exceptions`
async fn create_exception(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
    Json(payload): Json<PathExceptionRequest>,
) -> Result<Json<PathExceptionView>, ApiError> {
    let site_id = parse_uuid(&site_id, "site id")?;
    load_site_write(&state.db, site_id, &current).await?;
    require_observation_mode(&state, site_id).await?;

    let match_type = validate_match_type(payload.match_type.as_deref())?;
    let path = validate_path(
        payload.path.as_deref().ok_or_else(|| {
            ApiError::BadRequest("path is required".to_string())
        })?,
        &match_type,
    )?;
    let skip_layers = validate_skip_layers(
        payload.skip_layers.as_deref().unwrap_or(&["*".to_string()]),
    )?;
    let note = validate_note(payload.note.as_deref())?;

    let count = path_exceptions::Entity::find()
        .filter(path_exceptions::Column::SiteId.eq(site_id))
        .all(&state.db)
        .await?
        .len();
    if count >= MAX_PATH_EXCEPTIONS {
        return Err(ApiError::BadRequest(format!(
            "a site accepts at most {MAX_PATH_EXCEPTIONS} path exceptions"
        )));
    }
    let duplicate = path_exceptions::Entity::find()
        .filter(path_exceptions::Column::SiteId.eq(site_id))
        .filter(path_exceptions::Column::Path.eq(&path))
        .one(&state.db)
        .await?;
    if duplicate.is_some() {
        return Err(ApiError::Conflict(format!(
            "a path exception for '{path}' already exists on this site"
        )));
    }

    let now = Utc::now();
    let model = path_exceptions::ActiveModel {
        id: Set(Uuid::new_v4()),
        site_id: Set(site_id),
        path: Set(path),
        match_type: Set(match_type),
        skip_layers: Set(skip_layers),
        note: Set(note),
        log_hits: Set(payload.log_hits.unwrap_or(false)),
        enabled: Set(payload.enabled.unwrap_or(true)),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&state.db)
    .await?;

    tracing::info!(
        site_id = %site_id,
        exception_id = %model.id,
        actor = %current.email,
        "path exception created"
    );
    touch_site(&state, site_id).await?;
    notify_config_changed(&state, site_id, Some(&current.email)).await;
    Ok(Json(PathExceptionView::from(model)))
}

/// `PUT /api/v1/sites/{site_id}/path-exceptions/{id}`
async fn update_exception(
    State(state): State<AppState>,
    current: AuthUser,
    Path((site_id, id)): Path<(String, String)>,
    Json(payload): Json<PathExceptionRequest>,
) -> Result<Json<PathExceptionView>, ApiError> {
    let site_id = parse_uuid(&site_id, "site id")?;
    let id = parse_uuid(&id, "path exception id")?;
    load_site_write(&state.db, site_id, &current).await?;
    require_observation_mode(&state, site_id).await?;

    let row = find_exception(&state, site_id, id).await?;
    let mut match_type = row.match_type.clone();
    if payload.match_type.is_some() {
        match_type = validate_match_type(payload.match_type.as_deref())?;
    }
    let mut active: path_exceptions::ActiveModel = row.into();
    if let Some(path) = payload.path.as_deref() {
        let path = validate_path(path, &match_type)?;
        let duplicate = path_exceptions::Entity::find()
            .filter(path_exceptions::Column::SiteId.eq(site_id))
            .filter(path_exceptions::Column::Path.eq(&path))
            .filter(path_exceptions::Column::Id.ne(id))
            .one(&state.db)
            .await?;
        if duplicate.is_some() {
            return Err(ApiError::Conflict(format!(
                "a path exception for '{path}' already exists on this site"
            )));
        }
        active.path = Set(path);
    }
    if payload.match_type.is_some() {
        // Re-validate the stored path against the new match type: switching to
        // `wildcard` must not silently keep a plain path (no trailing `*`).
        let stored_path = active.path.clone().unwrap();
        let checked = validate_path(&stored_path, &match_type)?;
        active.path = Set(checked);
        active.match_type = Set(match_type);
    }
    if let Some(layers) = payload.skip_layers.as_deref() {
        active.skip_layers = Set(validate_skip_layers(layers)?);
    }
    if payload.note.is_some() {
        active.note = Set(validate_note(payload.note.as_deref())?);
    }
    if let Some(log_hits) = payload.log_hits {
        active.log_hits = Set(log_hits);
    }
    if let Some(enabled) = payload.enabled {
        active.enabled = Set(enabled);
    }
    active.updated_at = Set(Utc::now());

    let model = active.update(&state.db).await?;
    tracing::info!(
        site_id = %site_id,
        exception_id = %id,
        actor = %current.email,
        "path exception updated"
    );
    touch_site(&state, site_id).await?;
    notify_config_changed(&state, site_id, Some(&current.email)).await;
    Ok(Json(PathExceptionView::from(model)))
}

/// `DELETE /api/v1/sites/{site_id}/path-exceptions/{id}`
async fn delete_exception(
    State(state): State<AppState>,
    current: AuthUser,
    Path((site_id, id)): Path<(String, String)>,
) -> Result<axum::http::StatusCode, ApiError> {
    let site_id = parse_uuid(&site_id, "site id")?;
    let id = parse_uuid(&id, "path exception id")?;
    load_site_write(&state.db, site_id, &current).await?;

    let result = path_exceptions::Entity::delete_many()
        .filter(path_exceptions::Column::SiteId.eq(site_id))
        .filter(path_exceptions::Column::Id.eq(id))
        .exec(&state.db)
        .await?;
    if result.rows_affected == 0 {
        return Err(ApiError::NotFound(format!(
            "path exception {id} not found"
        )));
    }
    tracing::info!(
        site_id = %site_id,
        exception_id = %id,
        actor = %current.email,
        "path exception deleted"
    );
    touch_site(&state, site_id).await?;
    notify_config_changed(&state, site_id, Some(&current.email)).await;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

async fn find_exception(
    state: &AppState,
    site_id: Uuid,
    id: Uuid,
) -> Result<path_exceptions::Model, ApiError> {
    path_exceptions::Entity::find_by_id(id)
        .filter(path_exceptions::Column::SiteId.eq(site_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| {
            ApiError::NotFound(format!("path exception {id} not found"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_and_exact_paths_reject_wildcards() {
        assert!(validate_path("/api/health", "prefix").is_ok());
        assert!(validate_path("/api/*", "prefix").is_err());
    }

    #[test]
    fn wildcard_requires_a_single_trailing_star() {
        assert_eq!(
            validate_path("/api/*", "wildcard").unwrap(),
            "/api/*".to_string()
        );
        assert!(validate_path("/api", "wildcard").is_err());
        assert!(validate_path("/a/*/b", "wildcard").is_err());
        assert!(validate_path("*", "wildcard").is_err());
    }

    #[test]
    fn paths_must_be_request_paths() {
        assert!(validate_path("api/health", "prefix").is_err());
        assert!(validate_path("/api health", "prefix").is_err());
        assert!(
            validate_path(&format!("/{}", "a".repeat(600)), "prefix").is_err()
        );
        assert!(validate_path("/api\u{7}", "prefix").is_err());
    }

    #[test]
    fn skip_layers_collapse_and_reject_unknown_tokens() {
        assert_eq!(
            validate_skip_layers(&["*".to_string()]).unwrap(),
            vec!["*"]
        );
        assert_eq!(
            validate_skip_layers(&[
                "waf".to_string(),
                "bot".to_string(),
                "waf".to_string()
            ])
            .unwrap(),
            vec!["waf", "bot"]
        );
        assert!(validate_skip_layers(&[]).is_err());
        assert!(validate_skip_layers(&["nope".to_string()]).is_err());
    }

    #[test]
    fn match_type_defaults_to_prefix() {
        assert_eq!(validate_match_type(None).unwrap(), "prefix");
        assert!(validate_match_type(Some("regex")).is_err());
    }
}
