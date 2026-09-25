//! PingWAF control plane server.
//!
//! This crate provides the central management plane for PingWAF:
//! - A REST API (Axum) consumed by the React dashboard
//! - A gRPC service (tonic) consumed by the edge agents
//! - PostgreSQL persistence via SeaORM with automatic migrations
//!
//! The public entry point is [`start_server`], which boots both listeners and
//! blocks until a shutdown signal is received.

pub mod api;
pub mod auth;
pub mod config;
pub mod es;
pub mod frontend;
pub mod grpc;
pub mod migration;
pub mod models;
pub mod monitoring;

pub use config::ServerConfig;

use std::net::SocketAddr;
use std::sync::Arc;

use pingwaf_proto::control_plane::control_plane_server::ControlPlaneServer;
use sea_orm::{ConnectOptions, Database};
use sea_orm_migration::MigratorTrait;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;

use crate::api::state::AppState;
use crate::es::{ElasticsearchClient, ensure_index_template};
use crate::grpc::{AgentRegistry, ControlPlaneService};
use crate::migration::Migrator;
use crate::models::{role, user};

/// Starts the PingWAF control plane server.
///
/// This function:
/// 1. Validates the configuration
/// 2. Connects to PostgreSQL
/// 3. Runs pending schema migrations
/// 4. Seeds the default administrator account when the database is empty
/// 5. Boots the HTTP REST API and gRPC control plane in parallel
/// 6. Blocks until `SIGINT` / `SIGTERM`
pub async fn start_server(config: ServerConfig) -> anyhow::Result<()> {
    // ── 1. Validate ──────────────────────────────────────────────────────────
    config.validate()?;
    if config.uses_default_jwt_secret() {
        tracing::warn!(
            "using the built-in JWT secret — set PINGWAF_JWT_SECRET for production"
        );
    }

    tracing::info!(
        http_addr = %config.http_addr,
        grpc_addr = %config.grpc_addr,
        db = %config.db_url_redacted(),
        "starting PingWAF control plane"
    );

    // ── 2. Connect to PostgreSQL ─────────────────────────────────────────────
    let mut opts = ConnectOptions::new(config.db_url.as_str());
    opts.max_connections(config.db_max_connections)
        .min_connections(config.db_min_connections)
        .sqlx_logging(true);

    let db = Database::connect(opts).await.map_err(|err| {
        anyhow::anyhow!("failed to connect to PostgreSQL: {err}")
    })?;
    tracing::info!("database connection established");

    // ── 3. Run migrations ────────────────────────────────────────────────────
    Migrator::up(&db, None).await.map_err(|err| {
        anyhow::anyhow!("migration failed: {err}")
    })?;
    tracing::info!("schema migrations applied");

    // ── 4. Seed default admin ────────────────────────────────────────────────
    seed_admin(&db, &config).await?;

    // ── 5. Parse addresses early (before config moves into Arc) ─────────────
    let http_addr: SocketAddr = config
        .http_addr
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid http_addr '{}': {e}", config.http_addr))?;
    let grpc_addr: SocketAddr = config
        .grpc_addr
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid grpc_addr '{}': {e}", config.grpc_addr))?;

    // ── 6. Build shared state ────────────────────────────────────────────────
    let agents = AgentRegistry::new();
    let shared_config = Arc::new(config);

    // ── 6b. Optional Elasticsearch log shipper ──────────────────────────────
    // Started only when configured and enabled. Any failure here is logged and
    // swallowed: the control plane must boot (PostgreSQL-only) even if ES is
    // down or misconfigured.
    let es_client = match shared_config.elasticsearch.as_ref() {
        Some(es_config) if es_config.is_active() => {
            match ElasticsearchClient::new(es_config.clone()).await {
                Ok(client) => {
                    let client = Arc::new(client);
                    if let Err(err) = ensure_index_template(&client).await {
                        tracing::warn!(
                            error = %err,
                            "could not install the elasticsearch index template"
                        );
                    }
                    Some(client)
                }
                Err(err) => {
                    tracing::error!(
                        error = %err,
                        "failed to start the elasticsearch shipper, continuing without it"
                    );
                    None
                }
            }
        }
        _ => None,
    };

    // Shared by the gRPC service (which fills it from agent heartbeats) and the
    // REST API (which serves `/api/v1/cache/status` from it).
    let cache_status = crate::grpc::CacheStatusRegistry::new();

    let state = AppState {
        db: db.clone(),
        config: shared_config.clone(),
        agents: agents.clone(),
        es: es_client.clone(),
        cache_status: cache_status.clone(),
    };

    // ── 7. HTTP (Axum) ───────────────────────────────────────────────────────
    let http_listener = TcpListener::bind(http_addr).await.map_err(|err| {
        anyhow::anyhow!("failed to bind HTTP address {http_addr}: {err}")
    })?;
    let router = api::build_router(state);
    tracing::info!(%http_addr, "REST API listening");

    // ── 8. gRPC (tonic) ──────────────────────────────────────────────────────
    let grpc_listener = TcpListener::bind(grpc_addr).await.map_err(|err| {
        anyhow::anyhow!("failed to bind gRPC address {grpc_addr}: {err}")
    })?;
    tracing::info!(%grpc_addr, "gRPC control plane listening");

    let control_plane_service = ControlPlaneService::new(
        db.clone(),
        shared_config.clone(),
        agents.clone(),
        es_client.clone(),
        cache_status,
    );
    let grpc_server = ControlPlaneServer::new(control_plane_service);

    // ── 9. Start agent health monitor ───────────────────────────────────────
    let monitor_config = monitoring::MonitorConfig::from_server_config(&shared_config);
    let _monitor_handle = monitoring::start_health_monitor(
        db.clone(),
        shared_config.clone(),
        monitor_config,
        agents.clone(),
    );

    // ── 10. Spawn and wait for shutdown ─────────────────────────────────────
    tokio::select! {
        result = axum::serve(http_listener, router)
            .with_graceful_shutdown(shutdown_wait()) =>
        {
            if let Err(err) = result {
                tracing::error!(error = %err, "HTTP server error");
            }
        }
        result = tonic::transport::Server::builder()
            .add_service(grpc_server)
            .serve_with_incoming_shutdown(
                TcpListenerStream::new(grpc_listener),
                shutdown_signal(),
            ) =>
        {
            if let Err(err) = result {
                tracing::error!(error = %err, "gRPC server error");
            }
        }
    }

    // ── 11. Graceful Elasticsearch shutdown ─────────────────────────────────
    // Flush any buffered documents and stop the background tasks before exit.
    if let Some(client) = es_client {
        if let Err(err) = client.shutdown().await {
            tracing::warn!(error = %err, "elasticsearch shipper did not shut down cleanly");
        }
    }

    tracing::info!("PingWAF control plane shut down");
    Ok(())
}

/// Seeds the default administrator account if the users table is empty.
async fn seed_admin(
    db: &sea_orm::DatabaseConnection,
    config: &ServerConfig,
) -> anyhow::Result<()> {
    use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};

    let count = user::Entity::find().count(db).await?;
    if count > 0 {
        tracing::debug!(users = count, "users already exist, skipping seed");
        return Ok(());
    }

    // Check if the configured admin already exists (idempotent restart).
    let existing = user::Entity::find()
        .filter(user::Column::Email.eq(&config.default_admin_email))
        .one(db)
        .await?;
    if existing.is_some() {
        return Ok(());
    }

    let password_hash = auth::hash_password(&config.default_admin_password)
        .map_err(|err| anyhow::anyhow!("failed to hash admin password: {err}"))?;

    let now = chrono::Utc::now();
    let admin = user::ActiveModel {
        id: sea_orm::Set(uuid::Uuid::new_v4()),
        email: sea_orm::Set(config.default_admin_email.clone()),
        password_hash: sea_orm::Set(password_hash),
        name: sea_orm::Set(Some("Administrator".to_string())),
        role: sea_orm::Set(role::ADMIN.to_string()),
        created_at: sea_orm::Set(now),
        updated_at: sea_orm::Set(now),
    };

    use sea_orm::ActiveModelTrait;
    admin.insert(db).await?;

    tracing::info!(
        email = %config.default_admin_email,
        "default administrator account created"
    );
    Ok(())
}

/// Returns a future that resolves when the process receives SIGINT or SIGTERM.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("received SIGINT"),
        _ = terminate => tracing::info!("received SIGTERM"),
    }
}

/// Helper: a future that resolves on shutdown signal (used by axum's
/// `with_graceful_shutdown`).
async fn shutdown_wait() {
    shutdown_signal().await;
}
