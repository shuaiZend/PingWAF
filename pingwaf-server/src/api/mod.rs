//! REST API composition.
//!
//! Every route module contributes a `Router<AppState>` with paths relative to
//! `/api/v1`; this module merges them, attaches the shared middleware stack and
//! finally bakes the [`AppState`] in.

pub mod agents;
pub mod analytics;
pub mod auth;
pub mod bot;
pub mod cache;
pub mod challenge;
pub mod common;
pub mod error;
pub mod error_pages;
pub mod geo;
pub mod ip_rules;
pub mod keys;
pub mod logs;
pub mod rate_limiting;
pub mod rewrite;
pub mod rules;
pub mod settings;
pub mod sites;
pub mod ssl;
pub mod state;

use crate::frontend::serve_frontend;
use axum::extract::State;
use axum::http::{HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
use axum::Router;
use serde_json::json;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;

pub use error::{error_response, ApiError};
pub use state::AppState;

/// Prefix every REST route lives under.
pub const API_PREFIX: &str = "/api/v1";

/// Builds the complete HTTP router, including CORS, compression and tracing.
pub fn build_router(state: AppState) -> Router {
    let config = state.config.clone();

    let api = Router::new()
        .merge(auth::routes())
        .merge(sites::routes())
        .merge(keys::routes())
        .merge(agents::routes())
        .merge(rules::routes())
        .merge(rate_limiting::routes())
        .merge(cache::routes())
        .merge(logs::routes())
        .merge(analytics::routes())
        .merge(settings::routes())
        .merge(ssl::routes())
        .merge(ip_rules::routes())
        .merge(geo::routes())
        .merge(bot::routes())
        .merge(challenge::routes())
        .merge(rewrite::routes())
        .merge(error_pages::routes())
        .route("/health", get(health))
        .route("/version", get(version))
        .fallback(api_not_found);

    Router::new()
        .nest(API_PREFIX, api)
        // Load balancers and container probes usually hit the root path.
        .route("/healthz", get(health))
        // Serve the embedded React frontend for all non-API routes (SPA)
        .fallback(serve_frontend)
        .layer(build_cors(&config.cors_origins))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http().make_span_with(
            |request: &axum::http::Request<_>| {
                tracing::info_span!(
                    "http_request",
                    method = %request.method(),
                    path = %request.uri().path(),
                )
            },
        ))
        .with_state(state)
}

/// Permissive by default (the dashboard is served from the same origin in
/// production); an explicit `cors_origins` list restricts it.
fn build_cors(origins: &[String]) -> CorsLayer {
    let layer = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            axum::http::header::AUTHORIZATION,
            axum::http::header::CONTENT_TYPE,
            axum::http::header::ACCEPT,
        ])
        .max_age(std::time::Duration::from_secs(3600));

    let parsed: Vec<HeaderValue> = origins
        .iter()
        .filter_map(|origin| match origin.parse::<HeaderValue>() {
            Ok(value) => Some(value),
            Err(err) => {
                tracing::warn!(%origin, error = %err, "ignoring invalid CORS origin");
                None
            }
        })
        .collect();

    if parsed.is_empty() {
        tracing::warn!("no CORS origins configured, allowing any origin");
        layer.allow_origin(AllowOrigin::any())
    } else {
        tracing::info!(origins = ?parsed, "CORS restricted to the configured origins");
        layer.allow_origin(AllowOrigin::list(parsed))
    }
}

/// `GET /healthz` and `GET /api/v1/health` — verifies the database is reachable.
async fn health(State(state): State<AppState>) -> Response {
    match state.db.ping().await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "status": "ok", "database": "up" })),
        )
            .into_response(),
        Err(err) => {
            tracing::error!(error = %err, "database health check failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "status": "degraded", "database": "down" })),
            )
                .into_response()
        },
    }
}

/// `GET /api/v1/version` — build metadata the frontend shows in the footer.
async fn version(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({
        "name": env!("CARGO_PKG_NAME"),
        "version": env!("CARGO_PKG_VERSION"),
        "api": API_PREFIX,
        "registration_open": state.config.allow_registration,
    }))
}

/// JSON 404 for unmatched API routes, so the frontend never has to
/// parse an HTML error page for API calls.
async fn api_not_found(uri: axum::http::Uri) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": {
                "code": "not_found",
                "message": format!("no route matches {}", uri.path()),
            }
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cors_accepts_explicit_origins() {
        let _ = build_cors(&["http://localhost:5173".to_string()]);
        let _ = build_cors(&[]);
        // An unparsable origin is dropped rather than panicking.
        let _ = build_cors(&["not a header value".to_string()]);
    }
}
