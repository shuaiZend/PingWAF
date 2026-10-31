//! Global log retention settings.
//!
//! How long the access log and the security events are kept is a deployment
//! wide decision, so the two windows live in one single-row table and are
//! edited from the settings page. The background sweeper reads this row; the
//! manual `DELETE /logs/purge` endpoint stays independent of it.

use axum::extract::State;
use axum::routing::get;
use axum::Json;
use axum::Router;
use chrono::Utc;
use sea_orm::sea_query::OnConflict;
use sea_orm::{ActiveModelTrait, DatabaseConnection, DbErr, EntityTrait, Set};
use serde::Deserialize;

use crate::api::error::ApiError;
use crate::api::state::AppState;
use crate::auth::AuthUser;
use crate::models::log_retention;

/// Primary key of the single row.
const SETTINGS_ID: i32 = 1;

/// Routes contributed to `/api/v1`.
pub fn routes() -> Router<AppState> {
    Router::new().route("/settings/log-retention", get(show).put(update))
}

#[derive(Debug, Deserialize)]
pub struct UpdateLogRetentionRequest {
    /// Days a row of `access_logs` is kept before the sweeper deletes it.
    #[serde(default)]
    pub access_log_retention_days: Option<i32>,
    /// Days a row of `security_events` is kept.
    #[serde(default)]
    pub security_event_retention_days: Option<i32>,
    /// Skip access-log rows for static-asset requests that triggered no
    /// security rule.
    #[serde(default)]
    pub skip_static_access_logs: Option<bool>,
    /// Replacement for the static-suffix list. `None` keeps the current
    /// list; an empty array clears it (the switch then skips nothing).
    #[serde(default)]
    pub static_suffixes: Option<Vec<String>>,
}

/// `GET /api/v1/settings/log-retention` — administrators only.
async fn show(
    State(state): State<AppState>,
    current: AuthUser,
) -> Result<Json<log_retention::Model>, ApiError> {
    current.require_admin().map_err(ApiError::from)?;
    Ok(Json(load(&state.db).await?))
}

/// `PUT /api/v1/settings/log-retention` — administrators only.
async fn update(
    State(state): State<AppState>,
    current: AuthUser,
    Json(payload): Json<UpdateLogRetentionRequest>,
) -> Result<Json<log_retention::Model>, ApiError> {
    current.require_admin().map_err(ApiError::from)?;

    if payload.access_log_retention_days.is_none()
        && payload.security_event_retention_days.is_none()
        && payload.skip_static_access_logs.is_none()
        && payload.static_suffixes.is_none()
    {
        return Err(ApiError::BadRequest(
            "provide at least one retention or filtering setting".to_string(),
        ));
    }

    // Validate before touching the row so a bad suffix list cannot leave a
    // half-applied update behind.
    let suffixes = payload
        .static_suffixes
        .as_deref()
        .map(log_retention::normalize_suffix_list)
        .transpose()
        .map_err(ApiError::BadRequest)?;

    let row = load(&state.db).await?;
    let mut active: log_retention::ActiveModel = row.into();

    if let Some(days) = payload.access_log_retention_days {
        active.access_log_retention_days =
            Set(validate_days(days, "access log")?);
    }
    if let Some(days) = payload.security_event_retention_days {
        active.security_event_retention_days =
            Set(validate_days(days, "security event")?);
    }
    if let Some(skip) = payload.skip_static_access_logs {
        active.skip_static_access_logs = Set(skip);
    }
    if let Some(suffixes) = suffixes {
        active.static_suffixes = Set(suffixes);
    }
    active.updated_at = Set(Utc::now());

    let updated = active.update(&state.db).await?;
    tracing::info!(
        actor = %current.id,
        access_days = updated.access_log_retention_days,
        security_days = updated.security_event_retention_days,
        skip_static = updated.skip_static_access_logs,
        suffixes = updated.static_suffixes.len(),
        "log retention settings updated"
    );

    // The filter rides inside every rule bundle, so each site's fingerprint
    // changes; push the new configuration now instead of waiting for the
    // next periodic sync.
    crate::grpc::notify_all_config_changed(&state, None).await;

    Ok(Json(updated))
}

fn validate_days(days: i32, label: &str) -> Result<i32, ApiError> {
    if !(log_retention::MIN_RETENTION_DAYS..=log_retention::MAX_RETENTION_DAYS)
        .contains(&days)
    {
        return Err(ApiError::BadRequest(format!(
            "{label} retention must be between {} and {} days",
            log_retention::MIN_RETENTION_DAYS,
            log_retention::MAX_RETENTION_DAYS
        )));
    }
    Ok(days)
}

/// Reads the settings row, creating it with the defaults if it is missing.
///
/// `on_conflict_do_nothing` keeps two concurrent first requests from racing
/// each other into a duplicate-key error; whoever lost the insert reads the
/// row the winner wrote.
pub async fn load(
    db: &DatabaseConnection,
) -> Result<log_retention::Model, ApiError> {
    if let Some(row) = log_retention::Entity::find_by_id(SETTINGS_ID)
        .one(db)
        .await?
    {
        return Ok(row);
    }

    let insert = log_retention::ActiveModel {
        id: Set(SETTINGS_ID),
        access_log_retention_days: Set(log_retention::DEFAULT_RETENTION_DAYS),
        security_event_retention_days: Set(
            log_retention::DEFAULT_RETENTION_DAYS,
        ),
        skip_static_access_logs: Set(true),
        static_suffixes: Set(
            log_retention::DEFAULT_STATIC_SUFFIXES
                .iter()
                .map(ToString::to_string)
                .collect(),
        ),
        updated_at: Set(Utc::now()),
    };
    let ignore_conflict = OnConflict::column(log_retention::Column::Id)
        .do_nothing()
        .to_owned();
    match log_retention::Entity::insert(insert)
        .on_conflict(ignore_conflict)
        .exec(db)
        .await
    {
        Ok(_) | Err(DbErr::RecordNotInserted) => {},
        Err(err) => return Err(err.into()),
    }

    log_retention::Entity::find_by_id(SETTINGS_ID)
        .one(db)
        .await?
        .ok_or_else(|| {
            ApiError::Internal(
                "log retention settings row vanished".to_string(),
            )
        })
}
