//! Implementation of the `pingwaf.ControlPlane` gRPC service.
//!
//! Authentication on this plane is API-key based for registration and JWT based
//! afterwards: `register_agent` trades a long-lived API key for an agent token,
//! and every other RPC verifies that token before touching the database.

use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use pingwaf_proto::control_plane::{
    AgentHeartbeat, GetSiteConfigRequest, LogAck, LogEntry, MetricAck, MetricBatch,
    RegisterAgentRequest, RegisterAgentResponse, RuleBundle, ServerCommand, SiteConfig,
    SyncRulesRequest, control_plane_server::ControlPlane as ControlPlaneTrait,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set,
};
use tokio::sync::mpsc;
use tokio_stream::{Stream, StreamExt, wrappers::ReceiverStream};
use tonic::{Request, Response, Status, Streaming};
use uuid::Uuid;

use crate::api::keys::{authenticate_api_key, key_allows_agent};
use crate::auth::jwt::{create_agent_token, verify_agent_token};
use crate::config::ServerConfig;
use crate::es::{AccessLogDocument, ElasticsearchClient, SecurityEventDocument};
use crate::grpc::cache_status::CacheStatusRegistry;
use crate::grpc::config::{build_rule_bundle, build_site_config, from_timestamp};
use crate::grpc::registry::{AgentRegistry, COMMAND_CHANNEL_CAPACITY};
use crate::models::{
    action, agent, agent_status, api_key, security_event, site, access_log,
};

/// Stream type returned by the server-streaming RPCs.
type BoxedStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

/// gRPC status codes are deliberately coarse: agents must never learn whether an
/// API key exists but is wrong, or does not exist at all.
const INVALID_CREDENTIALS: &str = "invalid credentials";

/// Column widths from the migrations; longer values are truncated instead of
/// failing the whole batch.
const MAX_REQUEST_ID: usize = 36;
const MAX_CLIENT_IP: usize = 45;
const MAX_METHOD: usize = 10;
const MAX_HOST: usize = 255;
const MAX_RULE_ID: usize = 100;
const MAX_RULE_NAME: usize = 200;
const MAX_ACTION: usize = 30;
const MAX_COUNTRY: usize = 2;
const MAX_CACHE_STATUS: usize = 20;
const MAX_TLS_VERSION: usize = 10;
const MAX_UPSTREAM: usize = 255;

/// The gRPC control plane service.
pub struct ControlPlaneService {
    db: DatabaseConnection,
    config: Arc<ServerConfig>,
    agents: AgentRegistry,
    /// Optional Elasticsearch shipper. When present, every streamed log entry is
    /// also indexed for full-text search and long-term retention; the
    /// PostgreSQL insert always remains the source of truth for recent logs.
    es: Option<Arc<ElasticsearchClient>>,
    /// Latest per-site edge cache usage, fed from the heartbeat stream and read
    /// by `/api/v1/cache/status`.
    cache_status: CacheStatusRegistry,
}

impl ControlPlaneService {
    pub fn new(
        db: DatabaseConnection,
        config: Arc<ServerConfig>,
        agents: AgentRegistry,
        es: Option<Arc<ElasticsearchClient>>,
        cache_status: CacheStatusRegistry,
    ) -> Self {
        Self {
            db,
            config,
            agents,
            es,
            cache_status,
        }
    }

    /// Verifies an agent token and returns the matching row.
    async fn authorize(&self, token: &str) -> Result<agent::Model, Status> {
        if token.trim().is_empty() {
            return Err(Status::unauthenticated(INVALID_CREDENTIALS));
        }
        let claims = verify_agent_token(token, &self.config.jwt_secret)
            .map_err(|err| Status::unauthenticated(err.to_string()))?;
        let agent_id = claims
            .subject_id()
            .map_err(|_| Status::unauthenticated(INVALID_CREDENTIALS))?;

        agent::Entity::find_by_id(agent_id)
            .one(&self.db)
            .await
            .map_err(db_status)?
            .ok_or_else(|| Status::unauthenticated("agent is no longer registered"))
    }

    /// Sites this agent is allowed to see: its own binding, or every site owned
    /// by the user that issued its API key.
    async fn visible_sites(&self, row: &agent::Model) -> Result<Vec<site::Model>, Status> {
        if let Some(site_id) = row.site_id {
            let owned = site::Entity::find_by_id(site_id)
                .one(&self.db)
                .await
                .map_err(db_status)?;
            return Ok(owned.into_iter().collect());
        }

        let Some(key_id) = row.api_key_id else {
            return Ok(Vec::new());
        };
        let key = api_key::Entity::find_by_id(key_id)
            .one(&self.db)
            .await
            .map_err(db_status)?
            .ok_or_else(|| Status::unauthenticated("API key was revoked"))?;

        site::Entity::find()
            .filter(site::Column::UserId.eq(key.user_id))
            .all(&self.db)
            .await
            .map_err(db_status)
    }

    /// Builds the configuration an agent should be running right now.
    async fn site_config_for(&self, row: &agent::Model) -> Result<SiteConfig, Status> {
        let sites = self.visible_sites(row).await?;
        let ids: Vec<Uuid> = sites.iter().map(|site| site.id).collect();
        build_site_config(&self.db, Some(&ids)).await.map_err(db_status)
    }
}

#[tonic::async_trait]
impl ControlPlaneTrait for ControlPlaneService {
    async fn register_agent(
        &self,
        request: Request<RegisterAgentRequest>,
    ) -> Result<Response<RegisterAgentResponse>, Status> {
        let payload = request.into_inner();
        if payload.hostname.trim().is_empty() {
            return Err(Status::invalid_argument("hostname is required"));
        }

        // The API key is only ever presented here; everything afterwards uses the
        // agent token minted below.
        let (key, owner) = authenticate_api_key(&self.db, &payload.api_key)
            .await
            .map_err(|err| {
                tracing::warn!(error = %err, hostname = %payload.hostname, "agent registration rejected");
                Status::unauthenticated(INVALID_CREDENTIALS)
            })?;
        if !key_allows_agent(&key) {
            return Err(Status::permission_denied(
                "this API key is not allowed to register agents",
            ));
        }

        // Re-registering the same host updates the existing row instead of
        // creating a new one, so a restart does not inflate the inventory.
        let existing = agent::Entity::find()
            .filter(agent::Column::Hostname.eq(payload.hostname.clone()))
            .filter(agent::Column::IpAddress.eq(payload.ip_address.clone()))
            .one(&self.db)
            .await
            .map_err(db_status)?;

        let now = Utc::now();
        let status = agent_status::ONLINE.to_string();
        let site_id = match existing.as_ref().and_then(|row| row.site_id) {
            Some(id) => Some(id),
            None => resolve_default_site(&self.db, owner.id).await?,
        };

        let row = match existing {
            Some(row) => {
                let agent_id = row.id;
                let mut active: agent::ActiveModel = row.into();
                active.site_id = Set(site_id);
                active.version = Set(optional(&payload.version));
                active.os_info = Set(optional(&payload.os_info));
                active.cpu_cores = Set(positive_i32(payload.cpu_cores as i64));
                active.memory_bytes = Set(positive_i64(payload.memory_bytes as i64));
                active.status = Set(status);
                active.api_key_id = Set(Some(key.id));
                active.last_heartbeat = Set(Some(now));
                let updated = active.update(&self.db).await.map_err(db_status)?;
                tracing::info!(%agent_id, hostname = %payload.hostname, "agent re-registered");
                updated
            }
            None => {
                let agent_id = Uuid::new_v4();
                let created = agent::ActiveModel {
                    id: Set(agent_id),
                    site_id: Set(site_id),
                    hostname: Set(payload.hostname.clone()),
                    ip_address: Set(payload.ip_address.clone()),
                    version: Set(optional(&payload.version)),
                    os_info: Set(optional(&payload.os_info)),
                    cpu_cores: Set(positive_i32(payload.cpu_cores as i64)),
                    memory_bytes: Set(positive_i64(payload.memory_bytes as i64)),
                    status: Set(status),
                    api_key_id: Set(Some(key.id)),
                    config_hash: Set(None),
                    last_heartbeat: Set(Some(now)),
                    registered_at: Set(now),
                }
                .insert(&self.db)
                .await
                .map_err(db_status)?;
                tracing::info!(%agent_id, hostname = %payload.hostname, "agent registered");
                created
            }
        };

        let token = create_agent_token(
            row.id,
            &self.config.jwt_secret,
            self.config.refresh_token_expiration_hours,
        )
        .map_err(|err| Status::internal(err.to_string()))?;

        let initial_config = self.site_config_for(&row).await?;

        Ok(Response::new(RegisterAgentResponse {
            agent_id: row.id.to_string(),
            agent_token: token,
            heartbeat_interval_seconds: self.config.heartbeat_interval_seconds,
            initial_config: Some(initial_config),
        }))
    }

    type HeartbeatStream = BoxedStream<ServerCommand>;

    async fn heartbeat(
        &self,
        request: Request<Streaming<AgentHeartbeat>>,
    ) -> Result<Response<Self::HeartbeatStream>, Status> {
        let mut inbound = request.into_inner();

        // The first message identifies the agent; refusing to open a stream
        // before authentication keeps unauthenticated peers cheap.
        let first = inbound
            .message()
            .await?
            .ok_or_else(|| Status::invalid_argument("heartbeat stream closed immediately"))?;

        let claims = verify_agent_token(&first.agent_token, &self.config.jwt_secret)
            .map_err(|err| Status::unauthenticated(err.to_string()))?;
        let agent_id = claims
            .subject_id()
            .map_err(|_| Status::unauthenticated(INVALID_CREDENTIALS))?;
        if !first.agent_id.is_empty() && first.agent_id != agent_id.to_string() {
            return Err(Status::permission_denied(
                "agent_id does not match the presented token",
            ));
        }
        if agent::Entity::find_by_id(agent_id)
            .one(&self.db)
            .await
            .map_err(db_status)?
            .is_none()
        {
            return Err(Status::unauthenticated("agent is no longer registered"));
        }

        let (sender, receiver) = mpsc::channel::<ServerCommand>(COMMAND_CHANNEL_CAPACITY);
        let queued = self.agents.connect(agent_id, sender).await;
        for command in queued {
            // The registry owns the sender now, so replay through it.
            self.agents.send_command(&agent_id, command).await;
        }

        let db = self.db.clone();
        let registry = self.agents.clone();
        let cache_status = self.cache_status.clone();
        // The opening message already carries metrics; recording it means a
        // freshly connected edge shows up before its second heartbeat.
        cache_status.record_heartbeat(agent_id, &first.site_statuses).await;
        tokio::spawn(async move {
            persist_heartbeat(&db, agent_id, &first).await;
            while let Some(message) = match inbound.message().await {
                Ok(Some(message)) => Some(message),
                Ok(None) => None,
                Err(err) => {
                    tracing::debug!(%agent_id, error = %err, "heartbeat stream error");
                    None
                }
            } {
                cache_status
                    .record_heartbeat(agent_id, &message.site_statuses)
                    .await;
                persist_heartbeat(&db, agent_id, &message).await;
            }
            mark_offline(&db, agent_id).await;
            registry.disconnect(&agent_id).await;
            tracing::info!(%agent_id, "agent heartbeat stream ended");
        });

        tracing::info!(%agent_id, "agent heartbeat stream opened");
        Ok(Response::new(
            Box::pin(ReceiverStream::new(receiver).map(Ok)) as Self::HeartbeatStream
        ))
    }

    type SyncRulesStream = BoxedStream<RuleBundle>;

    async fn sync_rules(
        &self,
        request: Request<SyncRulesRequest>,
    ) -> Result<Response<Self::SyncRulesStream>, Status> {
        let payload = request.into_inner();
        let row = self.authorize(&payload.agent_token).await?;

        let sites = self.visible_sites(&row).await?;
        let requested = payload.site_id.trim();
        let selected: Vec<site::Model> = if requested.is_empty() {
            sites
        } else {
            let id = Uuid::parse_str(requested)
                .map_err(|_| Status::invalid_argument("site_id is not a UUID"))?;
            if !sites.iter().any(|site| site.id == id) {
                return Err(Status::permission_denied(
                    "agent is not allowed to sync this site",
                ));
            }
            sites.into_iter().filter(|site| site.id == id).collect()
        };

        let current_hash = payload.current_config_hash.trim().to_string();
        let mut bundles = Vec::with_capacity(selected.len());
        let mut skipped = 0usize;
        for site_row in &selected {
            match build_rule_bundle(&self.db, site_row).await {
                Ok(bundle) => {
                    // Delta sync: the agent already runs this exact configuration.
                    if !current_hash.is_empty() && bundle.config_hash == current_hash {
                        skipped += 1;
                        continue;
                    }
                    bundles.push(Ok(bundle));
                }
                Err(err) => {
                    tracing::error!(site_id = %site_row.id, error = %err, "could not build rule bundle");
                    bundles.push(Err(Status::internal(format!(
                        "could not build the rule bundle for site {}: {err}",
                        site_row.id
                    ))));
                }
            }
        }

        tracing::info!(
            agent_id = %row.id,
            sites = selected.len(),
            delivered = bundles.len(),
            skipped,
            "rule sync served"
        );

        // Record the hash the agent claims to run so the dashboard can show drift.
        if !current_hash.is_empty() {
            let mut active: agent::ActiveModel = row.into();
            active.config_hash = Set(Some(current_hash));
            active.last_heartbeat = Set(Some(Utc::now()));
            if let Err(err) = active.update(&self.db).await {
                tracing::warn!(error = %err, "could not persist agent config hash");
            }
        }

        Ok(Response::new(
            Box::pin(tokio_stream::iter(bundles)) as Self::SyncRulesStream
        ))
    }

    async fn ship_logs(
        &self,
        request: Request<Streaming<LogEntry>>,
    ) -> Result<Response<LogAck>, Status> {
        let mut inbound = request.into_inner();
        let batch_size = self.config.log_batch_size.max(1);

        let mut received: u64 = 0;
        let mut access_batch: Vec<access_log::ActiveModel> = Vec::with_capacity(batch_size);
        let mut event_batch: Vec<security_event::ActiveModel> = Vec::with_capacity(batch_size);

        loop {
            let message = match inbound.message().await {
                Ok(Some(message)) => message,
                Ok(None) => break,
                Err(err) => {
                    tracing::warn!(error = %err, received, "log stream aborted");
                    return Ok(Response::new(LogAck {
                        received_count: received,
                        success: false,
                        error_message: err.to_string(),
                    }));
                }
            };
            received += 1;

            let site_id = parse_optional_uuid(&message.site_id);
            let agent_id = parse_optional_uuid(&message.agent_id);
            let timestamp = from_timestamp(message.timestamp.as_ref());

            access_batch.push(access_log_row(&message, site_id, agent_id, timestamp));
            if let Some(event) = security_event_row(&message, site_id, agent_id, timestamp) {
                event_batch.push(event);
            }

            // Mirror the entry into Elasticsearch for full-text search and long
            // retention. Both calls are non-blocking `try_send`s, so a slow or
            // down ES cluster can never apply back-pressure to the agent stream.
            if let Some(es) = &self.es {
                es.index_access_log(access_log_document(es, &message, timestamp));
                if let Some(event) = security_event_document(es, &message, timestamp) {
                    es.index_security_event(event);
                }
            }

            if access_batch.len() >= batch_size {
                flush(&self.db, &mut access_batch, &mut event_batch).await?;
            }
        }

        flush(&self.db, &mut access_batch, &mut event_batch).await?;

        tracing::debug!(received, "log batch persisted");
        Ok(Response::new(LogAck {
            received_count: received,
            success: true,
            error_message: String::new(),
        }))
    }

    async fn ship_metrics(
        &self,
        request: Request<Streaming<MetricBatch>>,
    ) -> Result<Response<MetricAck>, Status> {
        let mut inbound = request.into_inner();
        let mut batches: u64 = 0;
        let mut metrics: u64 = 0;

        while let Some(batch) = inbound.message().await? {
            batches += 1;
            metrics += batch.metrics.len() as u64;

            // Metrics double as liveness proof: touching the heartbeat keeps an
            // agent that only ships metrics from being marked offline.
            if let Some(agent_id) = parse_optional_uuid(&batch.agent_id) {
                agent::Entity::update_many()
                    .col_expr(
                        agent::Column::LastHeartbeat,
                        sea_orm::sea_query::Expr::value(from_timestamp(batch.timestamp.as_ref())),
                    )
                    .filter(agent::Column::Id.eq(agent_id))
                    .exec(&self.db)
                    .await
                    .map_err(db_status)?;
            }
        }

        tracing::trace!(batches, metrics, "metrics received (not persisted yet)");
        Ok(Response::new(MetricAck {
            received_count: metrics,
            success: true,
        }))
    }

    async fn get_site_config(
        &self,
        request: Request<GetSiteConfigRequest>,
    ) -> Result<Response<SiteConfig>, Status> {
        let payload = request.into_inner();
        let row = self.authorize(&payload.agent_token).await?;

        let requested = payload.site_id.trim();
        let config = if requested.is_empty() {
            self.site_config_for(&row).await?
        } else {
            let id = Uuid::parse_str(requested)
                .map_err(|_| Status::invalid_argument("site_id is not a UUID"))?;
            let visible = self.visible_sites(&row).await?;
            if !visible.iter().any(|site| site.id == id) {
                return Err(Status::permission_denied(
                    "agent is not allowed to read this site",
                ));
            }
            build_site_config(&self.db, Some(&[id])).await.map_err(db_status)?
        };

        tracing::debug!(
            agent_id = %row.id,
            sites = config.sites.len(),
            config_hash = %config.config_hash,
            "site configuration served"
        );

        // The caller echoed no hash here, but recording the served one lets the
        // dashboard detect drift on the next heartbeat.
        let mut active: agent::ActiveModel = row.into();
        active.config_hash = Set(Some(config.config_hash.clone()));
        active.last_heartbeat = Set(Some(Utc::now()));
        if let Err(err) = active.update(&self.db).await {
            tracing::warn!(error = %err, "could not persist served config hash");
        }

        Ok(Response::new(config))
    }
}

/// Persists the runtime counters of one heartbeat.
async fn persist_heartbeat(db: &DatabaseConnection, agent_id: Uuid, message: &AgentHeartbeat) {
    let reported = from_timestamp(message.timestamp.as_ref());
    let health = agent_status::from_proto_health(message.health);

    let mut active = agent::ActiveModel {
        id: Set(agent_id),
        ..Default::default()
    };
    active.status = Set(health.to_string());
    active.last_heartbeat = Set(Some(reported));
    if !message.config_hash.trim().is_empty() {
        active.config_hash = Set(Some(message.config_hash.clone()));
    }
    if message.memory_usage_bytes > 0 {
        active.memory_bytes = Set(Some(message.memory_usage_bytes as i64));
    }

    if let Err(err) = active.update(db).await {
        tracing::warn!(%agent_id, error = %err, "could not persist heartbeat");
    }
}

/// Flips an agent to `offline` when its stream ends, unless a newer heartbeat has
/// already arrived (which happens when the agent reconnects quickly).
async fn mark_offline(db: &DatabaseConnection, agent_id: Uuid) {
    let row = match agent::Entity::find_by_id(agent_id).one(db).await {
        Ok(Some(row)) => row,
        Ok(None) => return,
        Err(err) => {
            tracing::warn!(%agent_id, error = %err, "could not load agent to mark offline");
            return;
        }
    };
    if row.last_heartbeat.is_some_and(|ts| ts > Utc::now() - chrono::Duration::seconds(2)) {
        return;
    }
    let mut active: agent::ActiveModel = row.into();
    active.status = Set(agent_status::OFFLINE.to_string());
    if let Err(err) = active.update(db).await {
        tracing::warn!(%agent_id, error = %err, "could not mark agent offline");
    }
}

/// Writes the accumulated rows, clearing both buffers.
async fn flush(
    db: &DatabaseConnection,
    access: &mut Vec<access_log::ActiveModel>,
    events: &mut Vec<security_event::ActiveModel>,
) -> Result<(), Status> {
    if !access.is_empty() {
        let rows = std::mem::take(access);
        if let Err(err) = access_log::Entity::insert_many(rows).exec(db).await {
            tracing::error!(error = %err, "failed to persist access logs");
            return Err(db_status(err));
        }
    }
    if !events.is_empty() {
        let rows = std::mem::take(events);
        if let Err(err) = security_event::Entity::insert_many(rows).exec(db).await {
            tracing::error!(error = %err, "failed to persist security events");
            return Err(db_status(err));
        }
    }
    Ok(())
}

/// Maps a shipped log entry onto the `access_logs` row.
fn access_log_row(
    entry: &LogEntry,
    site_id: Option<Uuid>,
    agent_id: Option<Uuid>,
    timestamp: DateTime<Utc>,
) -> access_log::ActiveModel {
    access_log::ActiveModel {
        // Auto-increment primary key: leave it unset.
        site_id: Set(site_id),
        agent_id: Set(agent_id),
        request_id: Set(truncate(&entry.request_id, MAX_REQUEST_ID)),
        timestamp: Set(timestamp),
        client_ip: Set(truncate(&entry.client_ip, MAX_CLIENT_IP).unwrap_or_else(|| "0.0.0.0".to_string())),
        method: Set(truncate(&entry.method, MAX_METHOD).unwrap_or_else(|| "GET".to_string())),
        host: Set(truncate(&entry.host, MAX_HOST)),
        path: Set(optional(&entry.path)),
        query_string: Set(optional(&entry.query_string)),
        status_code: Set(if entry.response_status == 0 {
            None
        } else {
            Some(entry.response_status as i32)
        }),
        response_size: Set(positive_i64(entry.response_body_size as i64)),
        upstream_addr: Set(truncate(&entry.upstream_addr, MAX_UPSTREAM)),
        upstream_latency_ms: Set(positive_i64(entry.upstream_latency_ms as i64)),
        total_latency_ms: Set(positive_i64(entry.total_latency_ms as i64)),
        cache_status: Set(truncate(&entry.cache_status, MAX_CACHE_STATUS)),
        user_agent: Set(optional(&entry.user_agent)),
        referer: Set(optional(&entry.referer)),
        country_code: Set(truncate(&entry.country_code, MAX_COUNTRY)),
        tls_version: Set(truncate(&entry.tls_version, MAX_TLS_VERSION)),
        ..Default::default()
    }
}

/// Maps a shipped log entry onto a `security_events` row, or `None` when the
/// request was clean and produced no WAF decision.
fn security_event_row(
    entry: &LogEntry,
    site_id: Option<Uuid>,
    agent_id: Option<Uuid>,
    timestamp: DateTime<Utc>,
) -> Option<security_event::ActiveModel> {
    let waf_action = entry.waf_action.trim();
    if waf_action.is_empty() && entry.waf_score == 0 && entry.waf_rule_id.trim().is_empty() {
        return None;
    }

    Some(security_event::ActiveModel {
        site_id: Set(site_id),
        agent_id: Set(agent_id),
        request_id: Set(truncate(&entry.request_id, MAX_REQUEST_ID)),
        timestamp: Set(timestamp),
        client_ip: Set(
            truncate(&entry.client_ip, MAX_CLIENT_IP).unwrap_or_else(|| "0.0.0.0".to_string()),
        ),
        method: Set(truncate(&entry.method, MAX_METHOD).unwrap_or_else(|| "GET".to_string())),
        host: Set(truncate(&entry.host, MAX_HOST)),
        path: Set(optional(&entry.path)),
        rule_id: Set(truncate(&entry.waf_rule_id, MAX_RULE_ID)),
        rule_name: Set(None),
        action: Set(truncate(waf_action, MAX_ACTION).unwrap_or_else(|| action::LOG.to_string())),
        score: Set(if entry.waf_score == 0 {
            None
        } else {
            Some(entry.waf_score as i32)
        }),
        waf_details: Set(optional(&entry.waf_details)),
        country_code: Set(truncate(&entry.country_code, MAX_COUNTRY)),
        user_agent: Set(optional(&entry.user_agent)),
        created_at: Set(Utc::now()),
        ..Default::default()
    })
}

/// Maps a shipped log entry onto the Elasticsearch access log document.
///
/// Unlike the PostgreSQL row, this keeps the fields Postgres drops (bodies,
/// headers, JA3, city, ASN), because ES is the full-text/forensic store. Bodies
/// are re-truncated to the server's `max_body_size` and the agent's own
/// truncation flag is preserved so the two never contradict each other.
fn access_log_document(
    es: &ElasticsearchClient,
    entry: &LogEntry,
    timestamp: DateTime<Utc>,
) -> AccessLogDocument {
    let (request_body, mut request_body_truncated) = es.truncate_body_to_string(&entry.request_body);
    request_body_truncated |= entry.request_body_truncated;
    let (response_body, mut response_body_truncated) =
        es.truncate_body_to_string(&entry.response_body);
    response_body_truncated |= entry.response_body_truncated;

    AccessLogDocument {
        timestamp,
        site_id: entry.site_id.clone(),
        site_domain: entry.host.clone(),
        agent_id: entry.agent_id.clone(),
        request_id: entry.request_id.clone(),

        client_ip: entry.client_ip.clone(),
        method: entry.method.clone(),
        scheme: optional(&entry.scheme),
        host: entry.host.clone(),
        path: entry.path.clone(),
        query_string: optional(&entry.query_string),
        protocol: optional(&entry.protocol),
        request_headers: headers_json(&entry.request_headers),
        request_body,
        request_body_size: positive_u64(entry.request_body_size),
        request_body_truncated,

        response_status: if entry.response_status == 0 {
            None
        } else {
            Some(entry.response_status as u16)
        },
        response_headers: headers_json(&entry.response_headers),
        response_body,
        response_body_size: positive_u64(entry.response_body_size),
        response_body_truncated,

        upstream_addr: optional(&entry.upstream_addr),
        upstream_latency_ms: positive_u64(entry.upstream_latency_ms),

        waf_score: positive_u32(entry.waf_score),
        waf_action: optional(&entry.waf_action),
        waf_rule_id: optional(&entry.waf_rule_id),
        waf_details: optional(&entry.waf_details),

        total_latency_ms: positive_u64(entry.total_latency_ms),
        cache_status: optional(&entry.cache_status),

        tls_version: optional(&entry.tls_version),
        ja3_hash: optional(&entry.ja3_hash),
        country_code: optional(&entry.country_code),
        city: optional(&entry.city),
        asn: positive_u32(entry.asn),

        user_agent: optional(&entry.user_agent),
        referer: optional(&entry.referer),
    }
}

/// Maps a shipped log entry onto an Elasticsearch security event, or `None`
/// when the request produced no WAF decision — the same condition that gates the
/// PostgreSQL `security_events` row, so the two stores stay consistent.
fn security_event_document(
    es: &ElasticsearchClient,
    entry: &LogEntry,
    timestamp: DateTime<Utc>,
) -> Option<SecurityEventDocument> {
    let waf_action = entry.waf_action.trim();
    if waf_action.is_empty() && entry.waf_score == 0 && entry.waf_rule_id.trim().is_empty() {
        return None;
    }

    let (request_body, mut request_body_truncated) = es.truncate_body_to_string(&entry.request_body);
    request_body_truncated |= entry.request_body_truncated;

    Some(SecurityEventDocument {
        timestamp,
        site_id: entry.site_id.clone(),
        site_domain: entry.host.clone(),
        agent_id: entry.agent_id.clone(),
        request_id: entry.request_id.clone(),

        client_ip: entry.client_ip.clone(),
        method: entry.method.clone(),
        host: entry.host.clone(),
        path: entry.path.clone(),
        query_string: optional(&entry.query_string),

        rule_id: entry.waf_rule_id.clone(),
        rule_name: None,
        action: if waf_action.is_empty() {
            action::LOG.to_string()
        } else {
            waf_action.to_string()
        },
        score: entry.waf_score,
        attack_category: entry.waf_matched_tags.first().cloned(),
        details: optional(&entry.waf_details),
        severity: None,

        user_agent: optional(&entry.user_agent),
        country_code: optional(&entry.country_code),
        asn: positive_u32(entry.asn),

        request_headers: headers_json(&entry.request_headers),
        request_body,
        request_body_truncated,
    })
}

/// Converts a protocol header map into an indexable JSON object, or `None` when
/// empty. The ES mapping stores these as non-indexed objects, so arbitrary keys
/// cannot cause a mapping explosion.
fn headers_json(
    headers: &std::collections::HashMap<String, String>,
) -> Option<serde_json::Value> {
    if headers.is_empty() {
        return None;
    }
    let map = headers
        .iter()
        .map(|(key, value)| (key.clone(), serde_json::Value::String(value.clone())))
        .collect::<serde_json::Map<String, serde_json::Value>>();
    Some(serde_json::Value::Object(map))
}

fn positive_u64(value: u64) -> Option<u64> {
    if value > 0 {
        Some(value)
    } else {
        None
    }
}

fn positive_u32(value: u32) -> Option<u32> {
    if value > 0 {
        Some(value)
    } else {
        None
    }
}

/// Binds a site to a newly registered agent when the API key owner has exactly
/// one site; otherwise the agent stays unbound and can serve all of them.
async fn resolve_default_site(
    db: &DatabaseConnection,
    user_id: Uuid,
) -> Result<Option<Uuid>, Status> {
    let owned = site::Entity::find()
        .filter(site::Column::UserId.eq(user_id))
        .all(db)
        .await
        .map_err(db_status)?;
    if owned.len() == 1 {
        Ok(Some(owned[0].id))
    } else {
        Ok(None)
    }
}

/// Maps a SeaORM error onto `INTERNAL`, logging the detail server side.
fn db_status(err: sea_orm::DbErr) -> Status {
    tracing::error!(error = %err, "database error in the gRPC control plane");
    match err {
        sea_orm::DbErr::RecordNotFound(msg) => Status::not_found(msg),
        other => Status::internal(other.to_string()),
    }
}

fn parse_optional_uuid(raw: &str) -> Option<Uuid> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    match Uuid::parse_str(raw) {
        Ok(id) => Some(id),
        Err(err) => {
            tracing::debug!(value = %raw, error = %err, "ignoring malformed UUID in log entry");
            None
        }
    }
}

/// Blank protocol strings become SQL NULLs.
fn optional(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Same as [`optional`] but bounded by a column width.
fn truncate(value: &str, max: usize) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.len() <= max {
        return Some(trimmed.to_string());
    }
    let mut end = max;
    while end > 0 && !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    Some(trimmed[..end].to_string())
}

/// Zero and negative protocol integers become SQL NULLs.
fn positive_i64(value: i64) -> Option<i64> {
    if value > 0 {
        Some(value)
    } else {
        None
    }
}

fn positive_i32(value: i64) -> Option<i32> {
    if value > 0 {
        Some(value.min(i64::from(i32::MAX)) as i32)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blanks_become_nulls() {
        assert_eq!(optional("  "), None);
        assert_eq!(optional(" x ").as_deref(), Some("x"));
        assert_eq!(truncate("", 10), None);
        assert_eq!(truncate("12345678", 4).as_deref(), Some("1234"));
    }

    #[test]
    fn truncation_respects_char_boundaries() {
        // Each 'é' is two bytes: a four byte cap must not split one.
        let truncated = truncate(&"é".repeat(10), 5).unwrap();
        assert_eq!(truncated.len(), 4);
        assert_eq!(truncated, "éé");
    }

    #[test]
    fn zero_values_become_nulls() {
        assert_eq!(positive_i64(0), None);
        assert_eq!(positive_i64(-3), None);
        assert_eq!(positive_i64(42), Some(42));
        assert_eq!(positive_i32(0), None);
        assert_eq!(positive_i32(7), Some(7));
        assert_eq!(positive_i32(i64::MAX), Some(i32::MAX));
    }

    #[test]
    fn uuids_are_parsed_leniently() {
        assert_eq!(parse_optional_uuid(""), None);
        assert_eq!(parse_optional_uuid("nope"), None);
        let id = Uuid::new_v4();
        assert_eq!(parse_optional_uuid(&id.to_string()), Some(id));
    }

    #[test]
    fn clean_requests_produce_no_security_event() {
        let entry = LogEntry {
            agent_id: String::new(),
            site_id: String::new(),
            request_id: String::new(),
            timestamp: None,
            client_ip: "10.0.0.1".into(),
            method: "GET".into(),
            host: "example.com".into(),
            path: "/".into(),
            ..Default::default()
        };
        assert!(security_event_row(&entry, None, None, Utc::now()).is_none());
        assert!(access_log_row(&entry, None, None, Utc::now()).timestamp.is_set());

        let mut blocked = entry.clone();
        blocked.waf_action = action::BLOCK.into();
        blocked.waf_score = 42;
        blocked.waf_rule_id = "sqli-942100".into();
        blocked.waf_details = "matched rule".into();
        let event = security_event_row(&blocked, None, None, Utc::now()).expect("event");
        assert!(event.action.is_set());
        assert_eq!(event.score.unwrap(), Some(42));
    }

    #[test]
    fn long_values_are_clamped_to_the_column_widths() {
        let entry = LogEntry {
            client_ip: "x".repeat(200),
            method: "GET".into(),
            country_code: "ZZZZ".into(),
            ..Default::default()
        };
        let row = access_log_row(&entry, None, None, Utc::now());
        assert_eq!(
            row.client_ip.clone().unwrap().len(),
            MAX_CLIENT_IP
        );
        assert_eq!(
            row.country_code.clone().unwrap().unwrap().len(),
            MAX_COUNTRY
        );
    }
}
