//! Global custom error page management.
//!
//! The templates are deployment-wide: every site's bundle carries the same
//! list and the data plane answers with the page matching the status code it
//! is about to return. Writes therefore push to every registered agent through
//! [`notify_all_config_changed`], not just to one site's nodes.

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

use crate::api::common::{non_empty, parse_uuid, Page, Pagination};
use crate::api::error::ApiError;
use crate::api::state::AppState;
use crate::auth::AuthUser;
use crate::grpc::notify_all_config_changed;
use crate::models::error_pages;

fn default_content_type() -> String {
    "text/html".to_string()
}

fn default_true() -> bool {
    true
}

fn default_scope() -> String {
    crate::models::error_pages::SCOPE_SITE.to_string()
}

/// Valid scopes, in the spelling the API accepts.
const VALID_SCOPES: [&str; 2] = [
    crate::models::error_pages::SCOPE_SITE,
    crate::models::error_pages::SCOPE_UNMATCHED_HOST,
];

/// Status codes an `unmatched_host` page may use.
const UNMATCHED_HOST_STATUSES: [i32; 3] = [404, 421, 503];

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(flatten)]
    pub pagination: Pagination,
    /// Optional scope filter (`site` / `unmatched_host`).
    #[serde(default)]
    pub scope: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    pub status_code: i32,
    #[serde(default = "default_scope")]
    pub scope: String,
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
    pub scope: Option<String>,
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
        .route("/error-pages", get(list).post(create))
        .route(
            "/error-pages/{page_id}",
            axum::routing::put(update).delete(remove),
        )
}

fn validate_status_code(scope: &str, code: i32) -> Result<(), ApiError> {
    if !(400..=599).contains(&code) {
        return Err(ApiError::BadRequest(
            "status_code must be between 400 and 599".to_string(),
        ));
    }
    if scope == crate::models::error_pages::SCOPE_UNMATCHED_HOST
        && !UNMATCHED_HOST_STATUSES.contains(&code)
    {
        return Err(ApiError::BadRequest(format!(
            "status_code {code} is not allowed for scope \
             'unmatched_host' (allowed: 404, 421, 503)"
        )));
    }
    Ok(())
}

fn validate_scope(scope: &str) -> Result<(), ApiError> {
    if VALID_SCOPES.contains(&scope) {
        Ok(())
    } else {
        Err(ApiError::BadRequest(format!(
            "scope must be one of: {}",
            VALID_SCOPES.map(|s| format!("'{s}'")).join(", ")
        )))
    }
}

/// Whether another row already occupies `(scope, status_code)`.
async fn scope_status_taken(
    state: &AppState,
    scope: &str,
    status_code: i32,
    exclude_id: Option<Uuid>,
) -> Result<bool, ApiError> {
    let mut query = error_pages::Entity::find()
        .filter(error_pages::Column::Scope.eq(scope))
        .filter(error_pages::Column::StatusCode.eq(status_code));
    if let Some(id) = exclude_id {
        query = query.filter(error_pages::Column::Id.ne(id));
    }
    Ok(query.one(&state.db).await?.is_some())
}

/// `GET /api/v1/error-pages` — administrators only.
async fn list(
    State(state): State<AppState>,
    current: AuthUser,
    Query(query): Query<ListQuery>,
) -> Result<Json<Page<error_pages::Model>>, ApiError> {
    current.require_admin().map_err(ApiError::from)?;
    let pagination = query.pagination.normalise();

    let mut find = error_pages::Entity::find();
    if let Some(scope) = &query.scope {
        validate_scope(scope)?;
        find = find.filter(error_pages::Column::Scope.eq(scope));
    }
    let paginator = find
        .order_by_asc(error_pages::Column::Scope)
        .order_by_asc(error_pages::Column::StatusCode)
        .paginate(&state.db, pagination.limit());

    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(pagination.index()).await?;
    Ok(Json(Page::new(rows, total, pagination)))
}

/// `POST /api/v1/error-pages` — administrators only.
async fn create(
    State(state): State<AppState>,
    current: AuthUser,
    Json(payload): Json<CreateRequest>,
) -> Result<Response, ApiError> {
    current.require_admin().map_err(ApiError::from)?;

    validate_scope(&payload.scope)?;
    validate_status_code(&payload.scope, payload.status_code)?;
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
    if scope_status_taken(&state, &payload.scope, payload.status_code, None)
        .await?
    {
        return Err(ApiError::Conflict(format!(
            "an error page for scope '{}' and status {} already exists",
            payload.scope, payload.status_code
        )));
    }

    let timestamp = chrono::Utc::now();
    let model = error_pages::ActiveModel {
        id: Set(Uuid::new_v4()),
        status_code: Set(payload.status_code),
        scope: Set(payload.scope.clone()),
        name: Set(payload.name.trim().to_string()),
        content_type: Set(payload.content_type),
        body_template: Set(payload.body_template),
        enabled: Set(payload.enabled),
        created_at: Set(timestamp),
        updated_at: Set(timestamp),
    }
    .insert(&state.db)
    .await?;

    tracing::info!(page_id = %model.id, status_code = model.status_code, "error page created");
    notify_all_config_changed(&state, Some(&current.email)).await;

    Ok((StatusCode::CREATED, Json(model)).into_response())
}

/// `PUT /api/v1/error-pages/{page_id}` — administrators only.
async fn update(
    State(state): State<AppState>,
    current: AuthUser,
    Path(page_id): Path<String>,
    Json(payload): Json<UpdateRequest>,
) -> Result<Json<error_pages::Model>, ApiError> {
    current.require_admin().map_err(ApiError::from)?;
    let target = parse_uuid(&page_id, "error page id")?;

    let row = find(&state, target).await?;
    // The effective scope after this update decides which status codes are
    // allowed, so it is computed before any validation.
    let effective_scope =
        payload.scope.clone().unwrap_or_else(|| row.scope.clone());
    let effective_status = payload.status_code.unwrap_or(row.status_code);
    let mut active: error_pages::ActiveModel = row.into();

    if let Some(scope) = &payload.scope {
        validate_scope(scope)?;
        active.scope = Set(scope.clone());
    }
    validate_status_code(&effective_scope, effective_status)?;
    if (payload.status_code.is_some() || payload.scope.is_some())
        && scope_status_taken(
            &state,
            &effective_scope,
            effective_status,
            Some(target),
        )
        .await?
    {
        return Err(ApiError::Conflict(format!(
            "an error page for scope '{effective_scope}' and status \
             {effective_status} already exists"
        )));
    }

    if let Some(code) = payload.status_code {
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
    tracing::info!(page_id = %target, "error page updated");
    notify_all_config_changed(&state, Some(&current.email)).await;

    Ok(Json(updated))
}

/// `DELETE /api/v1/error-pages/{page_id}` — administrators only.
async fn remove(
    State(state): State<AppState>,
    current: AuthUser,
    Path(page_id): Path<String>,
) -> Result<Response, ApiError> {
    current.require_admin().map_err(ApiError::from)?;
    let target = parse_uuid(&page_id, "error page id")?;
    find(&state, target).await?;

    error_pages::Entity::delete_by_id(target)
        .exec(&state.db)
        .await?;

    tracing::info!(page_id = %target, "error page deleted");
    notify_all_config_changed(&state, Some(&current.email)).await;

    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn find(
    state: &AppState,
    page_id: Uuid,
) -> Result<error_pages::Model, ApiError> {
    error_pages::Entity::find_by_id(page_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| {
            ApiError::NotFound(format!("error page {page_id} not found"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::error_pages::{SCOPE_SITE, SCOPE_UNMATCHED_HOST};
    use pretty_assertions::assert_eq;

    #[test]
    fn error_page_scope_validation_accepts_known_values() {
        for scope in [SCOPE_SITE, SCOPE_UNMATCHED_HOST] {
            assert!(validate_scope(scope).is_ok());
        }
        assert!(validate_scope("global").is_err());
        assert!(validate_scope("").is_err());
    }

    #[test]
    fn error_page_unmatched_host_restricted_statuses() {
        for code in UNMATCHED_HOST_STATUSES {
            assert!(
                validate_status_code(SCOPE_UNMATCHED_HOST, code).is_ok(),
                "{code} must be allowed for unmatched_host"
            );
        }
        for code in [400, 403, 429, 502] {
            assert!(
                validate_status_code(SCOPE_UNMATCHED_HOST, code).is_err(),
                "{code} must be rejected for unmatched_host"
            );
        }
        // `site` keeps the historic 400-599 range.
        for code in [400, 403, 429, 502, 599] {
            assert!(
                validate_status_code(SCOPE_SITE, code).is_ok(),
                "{code} must be allowed for site"
            );
        }
        assert!(validate_status_code(SCOPE_SITE, 399).is_err());
        assert!(validate_status_code(SCOPE_SITE, 600).is_err());
    }

    #[test]
    fn error_page_request_scope_defaults_to_site() {
        let payload: CreateRequest = serde_json::from_str(
            r#"{"status_code":502,"name":"n","body_template":"<p>x</p>"}"#,
        )
        .unwrap();
        assert_eq!(SCOPE_SITE.to_string(), payload.scope);
        let payload: UpdateRequest = serde_json::from_str("{}").unwrap();
        assert!(payload.scope.is_none());
    }
}
