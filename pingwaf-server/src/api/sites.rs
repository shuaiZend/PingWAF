//! Site management: CRUD for sites plus their origin servers and TLS material.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::Router;
use chrono::{DateTime, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    Set,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::common::{
    Page, Pagination, load_site_read, load_site_write, non_empty, normalise_domain, parse_uuid,
    require_write,
};
use crate::api::error::ApiError;
use crate::api::state::AppState;
use crate::auth::AuthUser;
use crate::grpc::notify_config_changed;
use crate::models::{
    acme_challenge, cache_rules, rate_limit_rules, rule, rule_groups, site, site_ssl, site_status,
    site_upstreams,
};

/// Public representation of a site.
#[derive(Debug, Clone, Serialize)]
pub struct SiteResponse {
    pub id: Uuid,
    pub name: String,
    pub domain: String,
    pub status: String,
    pub plan: String,
    pub user_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<site::Model> for SiteResponse {
    fn from(model: site::Model) -> Self {
        Self {
            id: model.id,
            name: model.name,
            domain: model.domain,
            status: model.status,
            plan: model.plan,
            user_id: model.user_id,
            created_at: model.created_at,
            updated_at: model.updated_at,
        }
    }
}

/// A site together with everything the dashboard shows on its detail page.
#[derive(Debug, Serialize)]
pub struct SiteDetail {
    pub site: SiteResponse,
    pub upstreams: Vec<site_upstreams::Model>,
    pub ssl: Option<SslResponse>,
    pub rule_count: u64,
    pub rule_group_count: u64,
    pub rate_limit_count: u64,
    pub cache_rule_count: u64,
}

/// TLS material as shown in the dashboard — never includes the private key.
#[derive(Debug, Serialize)]
pub struct SslResponse {
    pub id: Uuid,
    pub site_id: Uuid,
    pub domain: String,
    pub issuer: Option<String>,
    pub has_certificate: bool,
    pub has_private_key: bool,
    pub expires_at: Option<DateTime<Utc>>,
    pub auto_renew: bool,
    pub acme_email: Option<String>,
    pub acme_challenge_type: Option<String>,
    pub acme_dns_provider: Option<String>,
    pub acme_dns_config: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

impl From<site_ssl::Model> for SslResponse {
    fn from(model: site_ssl::Model) -> Self {
        Self {
            id: model.id,
            site_id: model.site_id,
            domain: model.domain,
            issuer: model.issuer,
            has_certificate: model.cert_pem.as_ref().is_some_and(|v| !v.is_empty()),
            has_private_key: model.key_pem.as_ref().is_some_and(|v| !v.is_empty()),
            expires_at: model.expires_at,
            auto_renew: model.auto_renew,
            acme_email: model.acme_email,
            acme_challenge_type: model.acme_challenge_type,
            acme_dns_provider: model.acme_dns_provider,
            acme_dns_config: model.acme_dns_config,
            created_at: model.created_at,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(flatten)]
    pub pagination: Pagination,
    pub search: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSiteRequest {
    pub name: String,
    pub domain: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub plan: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateSiteRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub plan: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateUpstreamRequest {
    pub name: String,
    pub address: String,
    #[serde(default = "default_weight")]
    pub weight: i32,
    #[serde(default)]
    pub tls: bool,
}

fn default_weight() -> i32 {
    1
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateUpstreamRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub weight: Option<i32>,
    #[serde(default)]
    pub tls: Option<bool>,
    #[serde(default)]
    pub health_status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpsertSslRequest {
    pub domain: String,
    #[serde(default)]
    pub cert_pem: Option<String>,
    #[serde(default)]
    pub key_pem: Option<String>,
    #[serde(default)]
    pub issuer: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default = "default_true")]
    pub auto_renew: bool,
    #[serde(default)]
    pub acme_email: Option<String>,
    #[serde(default)]
    pub acme_challenge_type: Option<String>,
    #[serde(default)]
    pub acme_dns_provider: Option<String>,
    #[serde(default)]
    pub acme_dns_config: Option<serde_json::Value>,
}

fn default_true() -> bool {
    true
}

/// Routes contributed to `/api/v1`.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/sites", get(list).post(create))
        .route("/sites/{site_id}", get(show).put(update).delete(remove))
        .route(
            "/sites/{site_id}/upstreams",
            get(list_upstreams).post(create_upstream),
        )
        .route(
            "/sites/{site_id}/upstreams/{upstream_id}",
            put(update_upstream).delete(delete_upstream),
        )
        .route(
            "/sites/{site_id}/ssl",
            get(show_ssl).put(upsert_ssl).delete(delete_ssl),
        )
}

/// `GET /api/v1/sites`
async fn list(
    State(state): State<AppState>,
    current: AuthUser,
    Query(query): Query<ListQuery>,
) -> Result<Json<Page<SiteResponse>>, ApiError> {
    let pagination = query.pagination.normalise();

    let mut condition = Condition::all();
    if !current.is_admin() {
        condition = condition.add(site::Column::UserId.eq(current.id));
    }
    if let Some(status) = non_empty(&query.status) {
        if !site_status::is_valid(&status) {
            return Err(ApiError::BadRequest(format!("unknown site status '{status}'")));
        }
        condition = condition.add(site::Column::Status.eq(status));
    }
    if let Some(search) = non_empty(&query.search) {
        let pattern = format!("%{}%", search.to_lowercase());
        condition = condition.add(
            Condition::any()
                .add(site::Column::Domain.like(&pattern))
                .add(site::Column::Name.like(&pattern)),
        );
    }

    let paginator = site::Entity::find()
        .filter(condition)
        .order_by_desc(site::Column::CreatedAt)
        .paginate(&state.db, pagination.limit());

    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(pagination.index()).await?;

    Ok(Json(Page::new(
        rows.into_iter().map(SiteResponse::from).collect(),
        total,
        pagination,
    )))
}

/// `POST /api/v1/sites`
async fn create(
    State(state): State<AppState>,
    current: AuthUser,
    Json(payload): Json<CreateSiteRequest>,
) -> Result<Response, ApiError> {
    require_write(&current)?;

    let name = payload.name.trim().to_string();
    if name.is_empty() || name.len() > 100 {
        return Err(ApiError::BadRequest(
            "name must be 1-100 characters".to_string(),
        ));
    }
    let domain = normalise_domain(&payload.domain)?;
    let status = payload.status.unwrap_or_else(|| site_status::PENDING.to_string());
    if !site_status::is_valid(&status) {
        return Err(ApiError::BadRequest(format!("unknown site status '{status}'")));
    }
    let plan = payload.plan.unwrap_or_else(|| "free".to_string());
    if plan.len() > 20 {
        return Err(ApiError::BadRequest("plan must be at most 20 characters".to_string()));
    }

    let timestamp = Utc::now();
    let id = Uuid::new_v4();
    let model = site::ActiveModel {
        id: Set(id),
        user_id: Set(current.id),
        name: Set(name),
        domain: Set(domain.clone()),
        status: Set(status),
        plan: Set(plan),
        created_at: Set(timestamp),
        updated_at: Set(timestamp),
    }
    .insert(&state.db)
    .await?;

    tracing::info!(%id, %domain, owner = %current.id, "site created");
    notify_config_changed(&state, id).await;

    Ok((StatusCode::CREATED, Json(SiteResponse::from(model))).into_response())
}

/// `GET /api/v1/sites/{site_id}`
async fn show(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
) -> Result<Json<SiteDetail>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    let model = load_site_read(&state.db, id, &current).await?;

    let upstreams = site_upstreams::Entity::find()
        .filter(site_upstreams::Column::SiteId.eq(id))
        .order_by_desc(site_upstreams::Column::Weight)
        .all(&state.db)
        .await?;
    let ssl = site_ssl::Entity::find()
        .filter(site_ssl::Column::SiteId.eq(id))
        .one(&state.db)
        .await?
        .map(SslResponse::from);

    let rule_count = rule::Entity::find()
        .filter(rule::Column::SiteId.eq(id))
        .count(&state.db)
        .await?;
    let rule_group_count = rule_groups::Entity::find()
        .filter(rule_groups::Column::SiteId.eq(id))
        .count(&state.db)
        .await?;
    let rate_limit_count = rate_limit_rules::Entity::find()
        .filter(rate_limit_rules::Column::SiteId.eq(id))
        .count(&state.db)
        .await?;
    let cache_rule_count = cache_rules::Entity::find()
        .filter(cache_rules::Column::SiteId.eq(id))
        .count(&state.db)
        .await?;

    Ok(Json(SiteDetail {
        site: model.into(),
        upstreams,
        ssl,
        rule_count,
        rule_group_count,
        rate_limit_count,
        cache_rule_count,
    }))
}

/// `PUT /api/v1/sites/{site_id}`
async fn update(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
    Json(payload): Json<UpdateSiteRequest>,
) -> Result<Json<SiteResponse>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    let model = load_site_write(&state.db, id, &current).await?;

    let mut active: site::ActiveModel = model.into();
    if let Some(name) = non_empty(&payload.name) {
        if name.len() > 100 {
            return Err(ApiError::BadRequest(
                "name must be at most 100 characters".to_string(),
            ));
        }
        active.name = Set(name);
    }
    if let Some(domain) = non_empty(&payload.domain) {
        active.domain = Set(normalise_domain(&domain)?);
    }
    if let Some(status) = non_empty(&payload.status) {
        if !site_status::is_valid(&status) {
            return Err(ApiError::BadRequest(format!("unknown site status '{status}'")));
        }
        active.status = Set(status);
    }
    if let Some(plan) = non_empty(&payload.plan) {
        if plan.len() > 20 {
            return Err(ApiError::BadRequest(
                "plan must be at most 20 characters".to_string(),
            ));
        }
        active.plan = Set(plan);
    }
    active.updated_at = Set(Utc::now());

    let updated = active.update(&state.db).await?;
    tracing::info!(%id, "site updated");
    notify_config_changed(&state, id).await;

    Ok(Json(updated.into()))
}

/// `DELETE /api/v1/sites/{site_id}`
async fn remove(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
) -> Result<Response, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    // Verify ownership first so that the ON DELETE CASCADE rules never run for a
    // site the caller must not see.
    load_site_write(&state.db, id, &current).await?;
    site::Entity::delete_by_id(id).exec(&state.db).await?;

    tracing::info!(%id, owner = %current.id, "site deleted");
    notify_config_changed(&state, id).await;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `GET /api/v1/sites/{site_id}/upstreams`
async fn list_upstreams(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
) -> Result<Json<Vec<site_upstreams::Model>>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    load_site_read(&state.db, id, &current).await?;

    let rows = site_upstreams::Entity::find()
        .filter(site_upstreams::Column::SiteId.eq(id))
        .order_by_desc(site_upstreams::Column::Weight)
        .all(&state.db)
        .await?;
    Ok(Json(rows))
}

/// `POST /api/v1/sites/{site_id}/upstreams`
async fn create_upstream(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
    Json(payload): Json<CreateUpstreamRequest>,
) -> Result<Response, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    load_site_write(&state.db, id, &current).await?;

    let name = payload.name.trim().to_string();
    let address = payload.address.trim().to_string();
    validate_upstream(&name, &address, payload.weight)?;

    let model = site_upstreams::ActiveModel {
        id: Set(Uuid::new_v4()),
        site_id: Set(id),
        name: Set(name),
        address: Set(address),
        weight: Set(payload.weight),
        tls: Set(payload.tls),
        health_status: Set("unknown".to_string()),
        created_at: Set(Utc::now()),
    }
    .insert(&state.db)
    .await?;

    tracing::info!(site_id = %id, upstream = %model.id, "upstream added");
    notify_config_changed(&state, id).await;

    Ok((StatusCode::CREATED, Json(model)).into_response())
}

/// `PUT /api/v1/sites/{site_id}/upstreams/{upstream_id}`
async fn update_upstream(
    State(state): State<AppState>,
    current: AuthUser,
    Path((site_id, upstream_id)): Path<(String, String)>,
    Json(payload): Json<UpdateUpstreamRequest>,
) -> Result<Json<site_upstreams::Model>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    let target = parse_uuid(&upstream_id, "upstream id")?;
    load_site_write(&state.db, id, &current).await?;

    let row = site_upstreams::Entity::find_by_id(target)
        .filter(site_upstreams::Column::SiteId.eq(id))
        .one(&state.db)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("upstream {target} not found")))?;

    let mut active: site_upstreams::ActiveModel = row.into();
    if let Some(name) = non_empty(&payload.name) {
        active.name = Set(name);
    }
    if let Some(address) = non_empty(&payload.address) {
        active.address = Set(address);
    }
    if let Some(weight) = payload.weight {
        active.weight = Set(weight);
    }
    if let Some(tls) = payload.tls {
        active.tls = Set(tls);
    }
    if let Some(health) = non_empty(&payload.health_status) {
        if health.len() > 20 {
            return Err(ApiError::BadRequest(
                "health_status must be at most 20 characters".to_string(),
            ));
        }
        active.health_status = Set(health);
    }

    let updated = active.update(&state.db).await?;
    validate_upstream(&updated.name, &updated.address, updated.weight)?;

    tracing::info!(site_id = %id, upstream = %target, "upstream updated");
    notify_config_changed(&state, id).await;

    Ok(Json(updated))
}

/// `DELETE /api/v1/sites/{site_id}/upstreams/{upstream_id}`
async fn delete_upstream(
    State(state): State<AppState>,
    current: AuthUser,
    Path((site_id, upstream_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    let target = parse_uuid(&upstream_id, "upstream id")?;
    load_site_write(&state.db, id, &current).await?;

    let deleted = site_upstreams::Entity::delete_by_id(target)
        .filter(site_upstreams::Column::SiteId.eq(id))
        .exec(&state.db)
        .await?;
    if deleted.rows_affected == 0 {
        return Err(ApiError::NotFound(format!("upstream {target} not found")));
    }

    tracing::info!(site_id = %id, upstream = %target, "upstream removed");
    notify_config_changed(&state, id).await;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `GET /api/v1/sites/{site_id}/ssl`
async fn show_ssl(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
) -> Result<Json<Option<SslResponse>>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    load_site_read(&state.db, id, &current).await?;

    let row = site_ssl::Entity::find()
        .filter(site_ssl::Column::SiteId.eq(id))
        .one(&state.db)
        .await?;
    Ok(Json(row.map(SslResponse::from)))
}

/// `PUT /api/v1/sites/{site_id}/ssl` — creates or replaces the site's TLS entry.
async fn upsert_ssl(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
    Json(payload): Json<UpsertSslRequest>,
) -> Result<Json<SslResponse>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    load_site_write(&state.db, id, &current).await?;

    let domain = normalise_domain(&payload.domain)?;
    if let Some(challenge) = non_empty(&payload.acme_challenge_type) {
        if !acme_challenge::is_valid(&challenge) {
            return Err(ApiError::BadRequest(format!(
                "unknown ACME challenge type '{challenge}'"
            )));
        }
    }
    let expires_at = match non_empty(&payload.expires_at) {
        Some(raw) => Some(crate::api::common::parse_datetime(&raw, "expires_at")?),
        None => None,
    };

    let existing = site_ssl::Entity::find()
        .filter(site_ssl::Column::SiteId.eq(id))
        .one(&state.db)
        .await?;

    let model = match existing {
        Some(row) => {
            let row_id = row.id;
            let mut active: site_ssl::ActiveModel = row.into();
            active.domain = Set(domain);
            if let Some(cert) = payload.cert_pem {
                active.cert_pem = Set(non_empty(&Some(cert)));
            }
            if let Some(key) = payload.key_pem {
                active.key_pem = Set(non_empty(&Some(key)));
            }
            active.issuer = Set(payload.issuer);
            active.expires_at = Set(expires_at);
            active.auto_renew = Set(payload.auto_renew);
            active.acme_email = Set(non_empty(&payload.acme_email));
            active.acme_challenge_type = Set(non_empty(&payload.acme_challenge_type));
            active.acme_dns_provider = Set(non_empty(&payload.acme_dns_provider));
            if let Some(config) = payload.acme_dns_config {
                active.acme_dns_config = Set(Some(config));
            }
            let updated = active.update(&state.db).await?;
            tracing::info!(site_id = %id, ssl = %row_id, "SSL configuration updated");
            updated
        }
        None => {
            let row_id = Uuid::new_v4();
            let active = site_ssl::ActiveModel {
                id: Set(row_id),
                site_id: Set(id),
                cert_pem: Set(non_empty(&payload.cert_pem)),
                key_pem: Set(non_empty(&payload.key_pem)),
                issuer: Set(non_empty(&payload.issuer)),
                domain: Set(domain),
                expires_at: Set(expires_at),
                auto_renew: Set(payload.auto_renew),
                acme_email: Set(non_empty(&payload.acme_email)),
                acme_challenge_type: Set(non_empty(&payload.acme_challenge_type)),
                acme_dns_provider: Set(non_empty(&payload.acme_dns_provider)),
                acme_dns_config: Set(payload.acme_dns_config),
                created_at: Set(Utc::now()),
            };
            let inserted = active.insert(&state.db).await?;
            tracing::info!(site_id = %id, ssl = %row_id, "SSL configuration created");
            inserted
        }
    };

    // Bump the site so that agents notice the configuration changed.
    touch_site(&state, id).await?;
    notify_config_changed(&state, id).await;

    Ok(Json(model.into()))
}

/// `DELETE /api/v1/sites/{site_id}/ssl`
async fn delete_ssl(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
) -> Result<Response, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    load_site_write(&state.db, id, &current).await?;

    site_ssl::Entity::delete_many()
        .filter(site_ssl::Column::SiteId.eq(id))
        .exec(&state.db)
        .await?;

    tracing::info!(site_id = %id, "SSL configuration removed");
    touch_site(&state, id).await?;
    notify_config_changed(&state, id).await;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Validates the fields shared by upstream create/update.
fn validate_upstream(name: &str, address: &str, weight: i32) -> Result<(), ApiError> {
    if name.is_empty() || name.len() > 100 {
        return Err(ApiError::BadRequest(
            "upstream name must be 1-100 characters".to_string(),
        ));
    }
    if address.is_empty() || address.len() > 255 {
        return Err(ApiError::BadRequest(
            "upstream address must be 1-255 characters".to_string(),
        ));
    }
    if !(1..=10_000).contains(&weight) {
        return Err(ApiError::BadRequest(
            "upstream weight must be between 1 and 10000".to_string(),
        ));
    }
    Ok(())
}

/// Updates `sites.updated_at` so agents see a fresh configuration fingerprint.
pub async fn touch_site(state: &AppState, site_id: Uuid) -> Result<(), ApiError> {
    site::Entity::update_many()
        .col_expr(site::Column::UpdatedAt, sea_orm::sea_query::Expr::value(Utc::now()))
        .filter(site::Column::Id.eq(site_id))
        .exec(&state.db)
        .await?;
    Ok(())
}
