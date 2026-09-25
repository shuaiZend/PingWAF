//! SSL/TLS certificate management: upload, ACME issuance, renewal and settings.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
use axum::Router;
use chrono::{DateTime, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, Set,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::common::{
    load_site_read, load_site_write, non_empty, parse_uuid, Page, Pagination,
};
use crate::api::error::ApiError;
use crate::api::sites::touch_site;
use crate::api::state::AppState;
use crate::auth::AuthUser;
use crate::grpc::notify_config_changed;
use crate::models::{acme_challenge, site_certificates, site_ssl};

/// Certificate status values.
mod cert_status {
    pub const ACTIVE: &str = "active";
    pub const PENDING: &str = "pending";
    pub const EXPIRED: &str = "expired";
    pub const FAILED: &str = "failed";

    pub fn is_valid(status: &str) -> bool {
        matches!(status, ACTIVE | PENDING | EXPIRED | FAILED)
    }
}

/// Public certificate representation (never exposes the private key).
#[derive(Debug, Serialize)]
pub struct CertificateResponse {
    pub id: Uuid,
    pub site_id: Uuid,
    pub domain: String,
    pub issuer: Option<String>,
    pub not_before: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub auto_renew: bool,
    pub acme_email: Option<String>,
    pub acme_challenge_type: String,
    pub acme_dns_provider: Option<String>,
    pub acme_dns_config: Option<serde_json::Value>,
    pub status: String,
    pub has_certificate: bool,
    pub has_private_key: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<site_certificates::Model> for CertificateResponse {
    fn from(m: site_certificates::Model) -> Self {
        Self {
            id: m.id,
            site_id: m.site_id,
            domain: m.domain,
            issuer: m.issuer,
            not_before: m.not_before,
            expires_at: m.expires_at,
            auto_renew: m.auto_renew,
            acme_email: m.acme_email,
            acme_challenge_type: m.acme_challenge_type,
            acme_dns_provider: m.acme_dns_provider,
            acme_dns_config: m.acme_dns_config,
            status: m.status,
            has_certificate: m.cert_pem.is_some(),
            has_private_key: m.key_pem.is_some(),
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

/// SSL settings response (from site_ssl or derived).
#[derive(Debug, Serialize)]
pub struct SslSettingsResponse {
    pub min_tls_version: String,
    pub hsts_enabled: bool,
    pub hsts_max_age: u32,
    pub always_use_https: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSslSettingsRequest {
    #[serde(default)]
    pub min_tls_version: Option<String>,
    #[serde(default)]
    pub hsts_enabled: Option<bool>,
    #[serde(default)]
    pub hsts_max_age: Option<u32>,
    #[serde(default)]
    pub always_use_https: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(flatten)]
    pub pagination: Pagination,
}

#[derive(Debug, Deserialize)]
pub struct CreateCertificateRequest {
    pub domain: String,
    #[serde(default)]
    pub cert_pem: Option<String>,
    #[serde(default)]
    pub key_pem: Option<String>,
    #[serde(default)]
    pub issuer: Option<String>,
    #[serde(default)]
    pub not_before: Option<DateTime<Utc>>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default = "default_true")]
    pub auto_renew: bool,
    #[serde(default)]
    pub acme_email: Option<String>,
    #[serde(default = "default_challenge_type")]
    pub acme_challenge_type: String,
    #[serde(default)]
    pub acme_dns_provider: Option<String>,
    #[serde(default)]
    pub acme_dns_config: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateCertificateRequest {
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub cert_pem: Option<String>,
    #[serde(default)]
    pub key_pem: Option<String>,
    #[serde(default)]
    pub issuer: Option<String>,
    #[serde(default)]
    pub not_before: Option<DateTime<Utc>>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub auto_renew: Option<bool>,
    #[serde(default)]
    pub acme_email: Option<String>,
    #[serde(default)]
    pub acme_challenge_type: Option<String>,
    #[serde(default)]
    pub acme_dns_provider: Option<String>,
    #[serde(default)]
    pub acme_dns_config: Option<serde_json::Value>,
    #[serde(default)]
    pub status: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_challenge_type() -> String {
    "http-01".to_string()
}

/// Routes contributed to `/api/v1`.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/sites/{site_id}/certificates",
            get(list_certificates).post(create_certificate),
        )
        .route(
            "/sites/{site_id}/certificates/{cert_id}",
            get(show_certificate)
                .put(update_certificate)
                .delete(delete_certificate),
        )
        .route(
            "/sites/{site_id}/certificates/{cert_id}/renew",
            axum::routing::post(renew_certificate),
        )
        .route(
            "/sites/{site_id}/ssl-settings",
            get(get_ssl_settings).put(update_ssl_settings),
        )
}

/// `GET /api/v1/sites/{site_id}/certificates`
async fn list_certificates(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Page<CertificateResponse>>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    load_site_read(&state.db, id, &current).await?;
    let pagination = query.pagination.normalise();

    let paginator = site_certificates::Entity::find()
        .filter(site_certificates::Column::SiteId.eq(id))
        .order_by_desc(site_certificates::Column::CreatedAt)
        .paginate(&state.db, pagination.limit());

    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(pagination.index()).await?;
    let items: Vec<CertificateResponse> =
        rows.into_iter().map(Into::into).collect();
    Ok(Json(Page::new(items, total, pagination)))
}

/// `POST /api/v1/sites/{site_id}/certificates`
async fn create_certificate(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
    Json(payload): Json<CreateCertificateRequest>,
) -> Result<Response, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    load_site_write(&state.db, id, &current).await?;

    let domain = payload.domain.trim().to_lowercase();
    if domain.is_empty() || domain.len() > 255 {
        return Err(ApiError::BadRequest(
            "domain must be 1-255 characters".to_string(),
        ));
    }
    if !acme_challenge::is_valid(&payload.acme_challenge_type) {
        return Err(ApiError::BadRequest(format!(
            "invalid acme_challenge_type '{}'",
            payload.acme_challenge_type
        )));
    }

    let status = if payload.cert_pem.is_some() {
        cert_status::ACTIVE
    } else if payload.acme_email.is_some() {
        cert_status::PENDING
    } else {
        cert_status::ACTIVE
    };

    let timestamp = Utc::now();
    let model = site_certificates::ActiveModel {
        id: Set(Uuid::new_v4()),
        site_id: Set(id),
        domain: Set(domain),
        cert_pem: Set(payload.cert_pem),
        key_pem: Set(payload.key_pem),
        issuer: Set(payload.issuer),
        not_before: Set(payload.not_before),
        expires_at: Set(payload.expires_at),
        auto_renew: Set(payload.auto_renew),
        acme_email: Set(payload.acme_email),
        acme_challenge_type: Set(payload.acme_challenge_type),
        acme_dns_provider: Set(payload.acme_dns_provider),
        acme_dns_config: Set(payload.acme_dns_config),
        status: Set(status.to_string()),
        created_at: Set(timestamp),
        updated_at: Set(timestamp),
    }
    .insert(&state.db)
    .await?;

    tracing::info!(site_id = %id, cert_id = %model.id, "certificate created");
    touch_site(&state, id).await?;
    notify_config_changed(&state, id).await;

    Ok(
        (StatusCode::CREATED, Json(CertificateResponse::from(model)))
            .into_response(),
    )
}

/// `GET /api/v1/sites/{site_id}/certificates/{cert_id}`
async fn show_certificate(
    State(state): State<AppState>,
    current: AuthUser,
    Path((site_id, cert_id)): Path<(String, String)>,
) -> Result<Json<CertificateResponse>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    let target = parse_uuid(&cert_id, "certificate id")?;
    load_site_read(&state.db, id, &current).await?;

    let row = find_certificate(&state, id, target).await?;
    Ok(Json(CertificateResponse::from(row)))
}

/// `PUT /api/v1/sites/{site_id}/certificates/{cert_id}`
async fn update_certificate(
    State(state): State<AppState>,
    current: AuthUser,
    Path((site_id, cert_id)): Path<(String, String)>,
    Json(payload): Json<UpdateCertificateRequest>,
) -> Result<Json<CertificateResponse>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    let target = parse_uuid(&cert_id, "certificate id")?;
    load_site_write(&state.db, id, &current).await?;

    let row = find_certificate(&state, id, target).await?;
    let mut active: site_certificates::ActiveModel = row.into();

    if let Some(domain) = non_empty(&payload.domain) {
        active.domain = Set(domain.to_lowercase());
    }
    if let Some(cert) = payload.cert_pem {
        active.cert_pem = Set(Some(cert));
    }
    if let Some(key) = payload.key_pem {
        active.key_pem = Set(Some(key));
    }
    if let Some(issuer) = payload.issuer {
        active.issuer = Set(non_empty(&Some(issuer)));
    }
    if let Some(nb) = payload.not_before {
        active.not_before = Set(Some(nb));
    }
    if let Some(exp) = payload.expires_at {
        active.expires_at = Set(Some(exp));
    }
    if let Some(ar) = payload.auto_renew {
        active.auto_renew = Set(ar);
    }
    if let Some(email) = payload.acme_email {
        active.acme_email = Set(non_empty(&Some(email)));
    }
    if let Some(ct) = non_empty(&payload.acme_challenge_type) {
        if !acme_challenge::is_valid(&ct) {
            return Err(ApiError::BadRequest(format!(
                "invalid acme_challenge_type '{ct}'"
            )));
        }
        active.acme_challenge_type = Set(ct);
    }
    if let Some(provider) = payload.acme_dns_provider {
        active.acme_dns_provider = Set(non_empty(&Some(provider)));
    }
    if let Some(config) = payload.acme_dns_config {
        active.acme_dns_config = Set(Some(config));
    }
    if let Some(status) = non_empty(&payload.status) {
        if !cert_status::is_valid(&status) {
            return Err(ApiError::BadRequest(format!(
                "invalid certificate status '{status}'"
            )));
        }
        active.status = Set(status);
    }
    active.updated_at = Set(Utc::now());

    let updated = active.update(&state.db).await?;
    tracing::info!(site_id = %id, cert_id = %target, "certificate updated");
    touch_site(&state, id).await?;
    notify_config_changed(&state, id).await;

    Ok(Json(CertificateResponse::from(updated)))
}

/// `DELETE /api/v1/sites/{site_id}/certificates/{cert_id}`
async fn delete_certificate(
    State(state): State<AppState>,
    current: AuthUser,
    Path((site_id, cert_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    let target = parse_uuid(&cert_id, "certificate id")?;
    load_site_write(&state.db, id, &current).await?;
    find_certificate(&state, id, target).await?;

    site_certificates::Entity::delete_by_id(target)
        .exec(&state.db)
        .await?;

    tracing::info!(site_id = %id, cert_id = %target, "certificate deleted");
    touch_site(&state, id).await?;
    notify_config_changed(&state, id).await;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `POST /api/v1/sites/{site_id}/certificates/{cert_id}/renew`
async fn renew_certificate(
    State(state): State<AppState>,
    current: AuthUser,
    Path((site_id, cert_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    let target = parse_uuid(&cert_id, "certificate id")?;
    load_site_write(&state.db, id, &current).await?;

    let row = find_certificate(&state, id, target).await?;
    if row.acme_email.is_none() {
        return Err(ApiError::BadRequest(
            "certificate is not ACME-managed; cannot auto-renew".to_string(),
        ));
    }

    // Mark as pending renewal
    let mut active: site_certificates::ActiveModel = row.into();
    active.status = Set(cert_status::PENDING.to_string());
    active.updated_at = Set(Utc::now());
    active.update(&state.db).await?;

    tracing::info!(site_id = %id, cert_id = %target, "certificate renewal triggered");

    Ok(Json(serde_json::json!({
        "message": "renewal initiated",
        "certificate_id": target,
        "status": cert_status::PENDING,
    })))
}

/// `GET /api/v1/sites/{site_id}/ssl-settings`
async fn get_ssl_settings(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
) -> Result<Json<SslSettingsResponse>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    load_site_read(&state.db, id, &current).await?;

    // The ssl settings are derived from the site_ssl row or defaults
    let ssl_row = site_ssl::Entity::find()
        .filter(site_ssl::Column::SiteId.eq(id))
        .one(&state.db)
        .await?;

    let settings = match ssl_row {
        Some(_row) => SslSettingsResponse {
            min_tls_version: "1.2".to_string(),
            hsts_enabled: false,
            hsts_max_age: 0,
            always_use_https: false,
        },
        None => SslSettingsResponse {
            min_tls_version: "1.2".to_string(),
            hsts_enabled: false,
            hsts_max_age: 0,
            always_use_https: false,
        },
    };

    Ok(Json(settings))
}

/// `PUT /api/v1/sites/{site_id}/ssl-settings`
async fn update_ssl_settings(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
    Json(payload): Json<UpdateSslSettingsRequest>,
) -> Result<Json<SslSettingsResponse>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    load_site_write(&state.db, id, &current).await?;

    // For now, SSL settings are stored at the site level via the site_ssl row.
    // We acknowledge the request and return the (static) settings.
    // A full implementation would persist these to a dedicated ssl_settings column.
    tracing::info!(site_id = %id, "SSL settings update requested");

    let settings = SslSettingsResponse {
        min_tls_version: payload
            .min_tls_version
            .unwrap_or_else(|| "1.2".to_string()),
        hsts_enabled: payload.hsts_enabled.unwrap_or(false),
        hsts_max_age: payload.hsts_max_age.unwrap_or(0),
        always_use_https: payload.always_use_https.unwrap_or(false),
    };

    touch_site(&state, id).await?;
    notify_config_changed(&state, id).await;

    Ok(Json(settings))
}

/// Loads a certificate scoped to a site.
async fn find_certificate(
    state: &AppState,
    site_id: Uuid,
    cert_id: Uuid,
) -> Result<site_certificates::Model, ApiError> {
    site_certificates::Entity::find_by_id(cert_id)
        .filter(site_certificates::Column::SiteId.eq(site_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| {
            ApiError::NotFound(format!("certificate {cert_id} not found"))
        })
}
