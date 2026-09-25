//! Custom error pages management per site.

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
use crate::models::error_pages;

fn default_content_type() -> String {
    "text/html".to_string()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(flatten)]
    pub pagination: Pagination,
}

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    pub status_code: i32,
    pub name: String,
    #[serde(default = "default_content_type")]
    pub content_type: String,
    pub body_template: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateRequest {
    #[serde(default)]
    pub status_code: Option<i32>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub body_template: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

/// Routes contributed to `/api/v1`.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/sites/{site_id}/error-pages", get(list).post(create))
        .route(
            "/sites/{site_id}/error-pages/{page_id}",
            axum::routing::put(update).delete(remove),
        )
}

fn validate_status_code(code: i32) -> Result<(), ApiError> {
    if !(400..=599).contains(&code) {
        return Err(ApiError::BadRequest(
            "status_code must be between 400 and 599".to_string(),
        ));
    }
    Ok(())
}

/// `GET /api/v1/sites/{site_id}/error-pages`
async fn list(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Page<error_pages::Model>>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    load_site_read(&state.db, id, &current).await?;
    let pagination = query.pagination.normalise();

    let paginator = error_pages::Entity::find()
        .filter(error_pages::Column::SiteId.eq(id))
        .order_by_asc(error_pages::Column::StatusCode)
        .paginate(&state.db, pagination.limit());

    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(pagination.index()).await?;
    Ok(Json(Page::new(rows, total, pagination)))
}

/// `POST /api/v1/sites/{site_id}/error-pages`
async fn create(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
    Json(payload): Json<CreateRequest>,
) -> Result<Response, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    load_site_write(&state.db, id, &current).await?;

    validate_status_code(payload.status_code)?;
    if payload.name.trim().is_empty() || payload.name.len() > 200 {
        return Err(ApiError::BadRequest(
            "name must be 1-200 characters".to_string(),
        ));
    }
    if payload.body_template.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "body_template must not be empty".to_string(),
        ));
    }

    let timestamp = chrono::Utc::now();
    let model = error_pages::ActiveModel {
        id: Set(Uuid::new_v4()),
        site_id: Set(id),
        status_code: Set(payload.status_code),
        name: Set(payload.name.trim().to_string()),
        content_type: Set(payload.content_type),
        body_template: Set(payload.body_template),
        enabled: Set(payload.enabled),
        created_at: Set(timestamp),
        updated_at: Set(timestamp),
    }
    .insert(&state.db)
    .await?;

    tracing::info!(site_id = %id, page_id = %model.id, "error page created");
    touch_site(&state, id).await?;
    notify_config_changed(&state, id).await;

    Ok((StatusCode::CREATED, Json(model)).into_response())
}

/// `PUT /api/v1/sites/{site_id}/error-pages/{page_id}`
async fn update(
    State(state): State<AppState>,
    current: AuthUser,
    Path((site_id, page_id)): Path<(String, String)>,
    Json(payload): Json<UpdateRequest>,
) -> Result<Json<error_pages::Model>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    let target = parse_uuid(&page_id, "error page id")?;
    load_site_write(&state.db, id, &current).await?;

    let row = find(&state, id, target).await?;
    let mut active: error_pages::ActiveModel = row.into();

    if let Some(code) = payload.status_code {
        validate_status_code(code)?;
        active.status_code = Set(code);
    }
    if let Some(name) = non_empty(&payload.name) {
        if name.len() > 200 {
            return Err(ApiError::BadRequest(
                "name must be at most 200 characters".to_string(),
            ));
        }
        active.name = Set(name);
    }
    if let Some(ct) = non_empty(&payload.content_type) {
        active.content_type = Set(ct);
    }
    if let Some(body) = payload.body_template {
        if body.trim().is_empty() {
            return Err(ApiError::BadRequest(
                "body_template must not be empty".to_string(),
            ));
        }
        active.body_template = Set(body);
    }
    if let Some(enabled) = payload.enabled {
        active.enabled = Set(enabled);
    }
    active.updated_at = Set(chrono::Utc::now());

    let updated = active.update(&state.db).await?;
    tracing::info!(site_id = %id, page_id = %target, "error page updated");
    touch_site(&state, id).await?;
    notify_config_changed(&state, id).await;

    Ok(Json(updated))
}

/// `DELETE /api/v1/sites/{site_id}/error-pages/{page_id}`
async fn remove(
    State(state): State<AppState>,
    current: AuthUser,
    Path((site_id, page_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    let target = parse_uuid(&page_id, "error page id")?;
    load_site_write(&state.db, id, &current).await?;
    find(&state, id, target).await?;

    error_pages::Entity::delete_by_id(target)
        .exec(&state.db)
        .await?;

    tracing::info!(site_id = %id, page_id = %target, "error page deleted");
    touch_site(&state, id).await?;
    notify_config_changed(&state, id).await;

    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn find(
    state: &AppState,
    site_id: Uuid,
    page_id: Uuid,
) -> Result<error_pages::Model, ApiError> {
    error_pages::Entity::find_by_id(page_id)
        .filter(error_pages::Column::SiteId.eq(site_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| {
            ApiError::NotFound(format!("error page {page_id} not found"))
        })
}
