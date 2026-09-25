use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwapOption;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tonic::transport::{Channel, Endpoint};
use tracing::{debug, error, info, warn};

use pingwaf_proto::control_plane::{
    self as proto, control_plane_client::ControlPlaneClient as ProtoClient,
};

use crate::cache::RuleCache;
use crate::config::AgentConfig;
use crate::heartbeat::{MetricsCollector, SystemMetrics};

/// Log entry that can be queued for shipping to the control plane.
/// This is a simplified local type that gets converted to proto at send time.
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub site_id: String,
    pub request_id: String,
    pub client_ip: String,
    pub method: String,
    pub scheme: String,
    pub host: String,
    pub path: String,
    pub query_string: String,
    pub protocol: String,
    pub request_headers: std::collections::HashMap<String, String>,
    pub request_body: Option<Vec<u8>>,
    pub request_body_size: u64,
    pub request_body_truncated: bool,
    pub response_status: u32,
    pub response_headers: std::collections::HashMap<String, String>,
    pub upstream_addr: String,
    pub upstream_latency_ms: u64,
    pub waf_score: u32,
    pub waf_action: String,
    pub waf_rule_id: String,
    pub waf_details: String,
    pub waf_matched_tags: Vec<String>,
    pub total_latency_ms: u64,
    pub cache_status: String,
    pub tls_version: String,
    pub ja3_hash: String,
    pub country_code: String,
    pub city: String,
    pub asn: u32,
    pub user_agent: String,
    pub referer: String,
}

/// Host callback invoked when the server requests a cache purge:
/// `(site_id, urls, tags)`.
type PurgeCacheFn = Box<dyn Fn(&str, &[String], &[String]) + Send + Sync>;

/// Callback types for server commands that affect the host application.
#[derive(Default)]
pub struct CommandHandlers {
    /// Called when the server requests a cache purge.
    pub on_purge_cache: Option<PurgeCacheFn>,
    /// Called when the server requests a config reload.
    pub on_config_reload: Option<Box<dyn Fn() + Send + Sync>>,
    /// Called when the server requests agent restart.
    pub on_restart: Option<Box<dyn Fn(bool) + Send + Sync>>,
}

/// The main gRPC client that manages the connection to the control plane server.
///
/// Responsibilities:
/// - Establish and maintain the gRPC channel
/// - Register the agent and keep the token fresh
/// - Send periodic heartbeats with system metrics
/// - Ship access/security logs in batches
/// - Receive and apply server commands (rule updates, IP blocks, etc.)
/// - Handle disconnection with exponential backoff reconnection
pub struct ControlPlaneClient {
    config: AgentConfig,
    /// Resolved agent ID (may be auto-generated)
    agent_id: String,
    /// Agent authentication token received during registration
    agent_token: ArcSwapOption<String>,
    /// Whether we are currently connected to the server
    connected: AtomicBool,
    /// Shutdown signal
    shutdown_signal: Arc<AtomicBool>,
    /// Shared rule cache
    rule_cache: Arc<RuleCache>,
    /// Shared metrics collector
    metrics: Arc<MetricsCollector>,
    /// Channel for queuing log entries (bounded to prevent memory exhaustion)
    log_sender: mpsc::Sender<LogEntry>,
    /// Log receiver (consumed by the log shipper task)
    log_receiver: Arc<tokio::sync::Mutex<mpsc::Receiver<LogEntry>>>,
    /// Optional command handlers
    command_handlers: Arc<tokio::sync::RwLock<CommandHandlers>>,
}

impl ControlPlaneClient {
    /// Create a new control plane client.
    ///
    /// The client does not connect immediately — call `connect()` or `start()` to begin.
    pub fn new(
        config: AgentConfig,
        rule_cache: Arc<RuleCache>,
        metrics: Arc<MetricsCollector>,
    ) -> Self {
        let agent_id = config.resolved_agent_id();
        let (log_tx, log_rx) = mpsc::channel(4096);

        // A purge is actionable without the host application: the edge cache
        // and its quota ledger live in the rule cache, so the default handler
        // is wired here rather than left to `set_command_handlers`. An
        // application that does register handlers replaces this one.
        let purge_cache = Arc::clone(&rule_cache);
        let handlers = CommandHandlers {
            on_purge_cache: Some(Box::new(move |site_id, urls, tags| {
                let cache = Arc::clone(&purge_cache);
                let site_id = site_id.to_string();
                let urls = urls.to_vec();
                if !tags.is_empty() {
                    // Cached objects carry no tags, so there is nothing to
                    // match them against. Say so rather than report a purge
                    // that silently removed nothing.
                    warn!(site_id = %site_id, tags = tags.len(),
                        "cache purge by tag is not supported, ignoring tags");
                }
                // Off the command loop: a full-site purge walks the disk.
                tokio::spawn(async move {
                    if let Err(e) = cache.purge_site(&site_id, &urls).await {
                        error!(error = %e, site_id = %site_id, "cache purge failed");
                    }
                });
            })),
            ..CommandHandlers::default()
        };

        Self {
            config,
            agent_id,
            agent_token: ArcSwapOption::empty(),
            connected: AtomicBool::new(false),
            shutdown_signal: Arc::new(AtomicBool::new(false)),
            rule_cache,
            metrics,
            log_sender: log_tx,
            log_receiver: Arc::new(tokio::sync::Mutex::new(log_rx)),
            command_handlers: Arc::new(tokio::sync::RwLock::new(handlers)),
        }
    }

    /// Get the resolved agent ID.
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    /// Check if the client is currently connected.
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    /// Set command handlers for server-initiated actions.
    pub async fn set_command_handlers(&self, handlers: CommandHandlers) {
        let mut h = self.command_handlers.write().await;
        *h = handlers;
    }

    /// Queue a log entry for shipping to the server (non-blocking).
    ///
    /// If the log buffer is full, the entry is dropped with a warning.
    pub fn send_log(&self, entry: LogEntry) {
        if let Err(e) = self.log_sender.try_send(entry) {
            match e {
                mpsc::error::TrySendError::Full(_) => {
                    warn!("Log buffer full, dropping entry");
                },
                mpsc::error::TrySendError::Closed(_) => {
                    debug!("Log channel closed, dropping entry");
                },
            }
        }
    }

    /// Establish the gRPC channel to the control plane server.
    async fn create_channel(&self) -> anyhow::Result<Channel> {
        let endpoint = Endpoint::from_shared(self.config.server_url.clone())
            .map_err(|e| anyhow::anyhow!("Invalid server URL: {}", e))?
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .keep_alive_while_idle(true)
            .http2_keep_alive_interval(Duration::from_secs(30))
            .tcp_nodelay(true);

        let channel = endpoint.connect().await?;
        Ok(channel)
    }

    /// Register this agent with the control plane.
    ///
    /// Returns the agent token and initial configuration if provided.
    async fn register(
        &self,
        channel: Channel,
    ) -> anyhow::Result<proto::RegisterAgentResponse> {
        let mut client = ProtoClient::new(channel);

        let hostname = get_hostname();
        let (cpu_cores, memory_bytes, os_info) = get_system_info();

        let request = proto::RegisterAgentRequest {
            api_key: self.config.api_key.clone(),
            hostname,
            ip_address: get_local_ip(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            os_info,
            cpu_cores,
            memory_bytes,
        };

        let response = client.register_agent(request).await?.into_inner();

        // Store the agent token
        self.agent_token
            .store(Some(Arc::new(response.agent_token.clone())));

        info!(
            agent_id = %response.agent_id,
            heartbeat_interval = response.heartbeat_interval_seconds,
            "Registered with control plane"
        );

        // Apply initial config if provided
        if let Some(ref site_config) = response.initial_config {
            if !site_config.sites.is_empty() {
                if let Err(e) =
                    self.rule_cache.update_from_site_config(site_config)
                {
                    error!(error = %e, "Failed to apply initial site config");
                }
            }
        }

        Ok(response)
    }

    /// Start the client: connect, register, and spawn background tasks.
    ///
    /// Returns join handles for the spawned tasks (heartbeat, log shipper, rule sync).
    pub async fn start(
        self: &Arc<Self>,
    ) -> anyhow::Result<Vec<JoinHandle<()>>> {
        let mut handles = Vec::new();

        // Spawn the connection manager with reconnection logic
        let this = Arc::clone(self);
        let handle = tokio::spawn(async move {
            this.connection_loop().await;
        });
        handles.push(handle);

        Ok(handles)
    }

    /// Main connection loop with exponential backoff reconnection.
    async fn connection_loop(self: &Arc<Self>) {
        let mut retry_delay =
            Duration::from_millis(self.config.reconnect_initial_delay_ms);
        let max_delay =
            Duration::from_millis(self.config.reconnect_max_delay_ms);

        loop {
            if self.shutdown_signal.load(Ordering::Relaxed) {
                info!("Agent shutting down");
                break;
            }

            match self.try_connect_and_run().await {
                Ok(()) => {
                    // Clean disconnect (server closed connection gracefully)
                    info!("Disconnected from control plane, will reconnect");
                    retry_delay = Duration::from_millis(
                        self.config.reconnect_initial_delay_ms,
                    );
                },
                Err(e) => {
                    warn!(
                        error = %e,
                        delay_ms = retry_delay.as_millis() as u64,
                        "Connection to control plane failed"
                    );
                },
            }

            self.connected.store(false, Ordering::Relaxed);

            // Wait before reconnecting with exponential backoff
            tokio::time::sleep(retry_delay).await;
            retry_delay = (retry_delay * 2).min(max_delay);
        }
    }

    /// Try to connect, register, and run the heartbeat + log shipper.
    /// Returns Ok(()) on clean disconnect, Err on connection failure.
    async fn try_connect_and_run(&self) -> anyhow::Result<()> {
        let channel = self.create_channel().await?;

        // Register
        let reg_response = self.register(channel.clone()).await?;
        self.connected.store(true, Ordering::Relaxed);
        info!("Connected to control plane at {}", self.config.server_url);

        // Run heartbeat and log shipping concurrently
        let heartbeat_fut = self.run_heartbeat(channel.clone(), &reg_response);
        let log_shipper_fut = self.run_log_shipper(channel.clone());

        // Wait for either to complete (usually means disconnection)
        tokio::select! {
            result = heartbeat_fut => {
                if let Err(e) = result {
                    error!(error = %e, "Heartbeat stream ended");
                    return Err(e);
                }
            }
            result = log_shipper_fut => {
                if let Err(e) = result {
                    error!(error = %e, "Log shipper ended");
                    return Err(e);
                }
            }
            _ = self.wait_for_shutdown() => {
                info!("Shutdown signal received");
            }
        }

        Ok(())
    }

    /// Run the bidirectional heartbeat stream.
    ///
    /// Sends periodic heartbeats and processes server commands from the response stream.
    async fn run_heartbeat(
        &self,
        channel: Channel,
        _reg_response: &proto::RegisterAgentResponse,
    ) -> anyhow::Result<()> {
        let mut client = ProtoClient::new(channel);

        let heartbeat_interval =
            Duration::from_secs(self.config.heartbeat_interval_secs.max(5));

        // Create a channel for sending heartbeats to the stream
        let (hb_tx, hb_rx) = mpsc::channel::<proto::AgentHeartbeat>(4);
        let outbound = tokio_stream::wrappers::ReceiverStream::new(hb_rx);

        // Start the bidirectional stream
        let response = client.heartbeat(outbound).await?;
        let mut inbound = response.into_inner();

        // Spawn a task to send heartbeats periodically
        let agent_id = self.agent_id.clone();
        let agent_token = self
            .agent_token
            .load_full()
            .map(|t| (*t).clone())
            .unwrap_or_default();
        let metrics = Arc::clone(&self.metrics);
        let rule_cache = Arc::clone(&self.rule_cache);
        let shutdown = Arc::clone(&self.shutdown_signal);

        let sender_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(heartbeat_interval);
            interval.set_missed_tick_behavior(
                tokio::time::MissedTickBehavior::Delay,
            );

            loop {
                interval.tick().await;

                if shutdown.load(Ordering::Relaxed) {
                    break;
                }

                let system_metrics = metrics.collect();
                let hb = proto::AgentHeartbeat {
                    agent_id: agent_id.clone(),
                    agent_token: agent_token.clone(),
                    timestamp: Some(prost_types::Timestamp {
                        seconds: chrono::Utc::now().timestamp(),
                        nanos: 0,
                    }),
                    cpu_usage_percent: system_metrics.cpu_usage_percent,
                    memory_usage_bytes: system_metrics.memory_usage_bytes,
                    active_connections: system_metrics.active_connections,
                    requests_per_second: system_metrics.requests_per_second,
                    blocked_requests_total: system_metrics
                        .blocked_requests_total,
                    health: proto::AgentHealthStatus::AgentHealthHealthy as i32,
                    config_hash: rule_cache.config_hash(),
                    // Per-site edge cache disk usage, straight from the quota
                    // ledger; this is what the control plane's
                    // `/api/v1/cache/status` reports.
                    site_statuses: rule_cache.cache_statuses(),
                };

                if hb_tx.send(hb).await.is_err() {
                    debug!("Heartbeat channel closed");
                    break;
                }
            }
        });

        // Process incoming server commands
        let rule_cache = Arc::clone(&self.rule_cache);
        let command_handlers = Arc::clone(&self.command_handlers);
        let shutdown = Arc::clone(&self.shutdown_signal);

        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }

            tokio::select! {
                msg = inbound.message() => {
                    match msg {
                        Ok(Some(command)) => {
                            Self::handle_server_command(
                                &command,
                                &rule_cache,
                                &command_handlers,
                            ).await;
                        }
                        Ok(None) => {
                            info!("Server closed heartbeat stream");
                            break;
                        }
                        Err(e) => {
                            error!(error = %e, "Heartbeat stream error");
                            return Err(e.into());
                        }
                    }
                }
                _ = self.wait_for_shutdown() => {
                    break;
                }
            }
        }

        sender_task.abort();
        Ok(())
    }

    /// Handle a server command received via the heartbeat stream.
    async fn handle_server_command(
        command: &proto::ServerCommand,
        rule_cache: &Arc<RuleCache>,
        command_handlers: &Arc<tokio::sync::RwLock<CommandHandlers>>,
    ) {
        let command_type = proto::CommandType::try_from(command.r#type)
            .unwrap_or(proto::CommandType::CommandUnknown);

        debug!(
            command_id = %command.command_id,
            command_type = ?command_type,
            "Received server command"
        );

        match &command.payload {
            Some(proto::server_command::Payload::RuleUpdate(update)) => {
                if let Some(ref bundle) = update.rules {
                    if let Err(e) = rule_cache.update_from_bundle(bundle) {
                        error!(error = %e, "Failed to apply rule update");
                    } else {
                        info!(site_id = %update.site_id, "Applied rule update from server");
                    }
                }
            },
            Some(proto::server_command::Payload::BlockIp(block)) => {
                let duration = if block.duration_seconds > 0 {
                    Some(Duration::from_secs(block.duration_seconds as u64))
                } else {
                    None
                };
                for ip in &block.ip_addresses {
                    rule_cache.block_ip(
                        &block.site_id,
                        ip,
                        duration,
                        &block.reason,
                    );
                }
            },
            Some(proto::server_command::Payload::UnblockIp(unblock)) => {
                for ip in &unblock.ip_addresses {
                    rule_cache.unblock_ip(&unblock.site_id, ip);
                }
            },
            Some(proto::server_command::Payload::ConfigReload(reload)) => {
                if let Some(ref config) = reload.config {
                    if let Err(e) = rule_cache.update_from_site_config(config) {
                        error!(error = %e, "Failed to apply config reload");
                    } else {
                        info!("Applied full config reload from server");
                    }
                }
                let handlers = command_handlers.read().await;
                if let Some(ref cb) = handlers.on_config_reload {
                    cb();
                }
            },
            Some(proto::server_command::Payload::PurgeCache(purge)) => {
                info!(
                    site_id = %purge.site_id,
                    urls = purge.urls.len(),
                    tags = purge.tags.len(),
                    "Received cache purge command"
                );
                let handlers = command_handlers.read().await;
                if let Some(ref cb) = handlers.on_purge_cache {
                    cb(&purge.site_id, &purge.urls, &purge.tags);
                }
            },
            Some(proto::server_command::Payload::UpdateSite(update)) => {
                if let Some(ref site_config) = update.site_config {
                    if let Err(e) =
                        rule_cache.update_from_site_config(site_config)
                    {
                        error!(error = %e, "Failed to apply site update");
                    }
                }
            },
            Some(proto::server_command::Payload::RestartAgent(restart)) => {
                warn!(
                    reason = %restart.reason,
                    graceful = restart.graceful,
                    "Server requested agent restart"
                );
                let handlers = command_handlers.read().await;
                if let Some(ref cb) = handlers.on_restart {
                    cb(restart.graceful);
                }
            },
            None => {
                debug!("Received server command with no payload");
            },
        }
    }

    /// Run the log shipping loop.
    ///
    /// Batches log entries and sends them to the server periodically or when
    /// the batch size threshold is reached.
    async fn run_log_shipper(&self, channel: Channel) -> anyhow::Result<()> {
        let mut client = ProtoClient::new(channel);
        let mut receiver = self.log_receiver.lock().await;
        let mut batch: Vec<proto::LogEntry> =
            Vec::with_capacity(self.config.log_batch_size);

        let flush_interval =
            Duration::from_secs(self.config.log_flush_interval_secs.max(1));
        let mut interval = tokio::time::interval(flush_interval);
        interval
            .set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        let agent_id = self.agent_id.clone();

        loop {
            if self.shutdown_signal.load(Ordering::Relaxed) {
                // Flush remaining logs before shutdown
                if !batch.is_empty() {
                    Self::flush_logs(&mut client, &agent_id, &mut batch).await;
                }
                break;
            }

            tokio::select! {
                entry = receiver.recv() => {
                    match entry {
                        Some(log_entry) => {
                            let proto_entry = Self::convert_log_entry(&agent_id, &log_entry);
                            batch.push(proto_entry);

                            // Flush when batch is full
                            if batch.len() >= self.config.log_batch_size {
                                Self::flush_logs(&mut client, &agent_id, &mut batch).await;
                            }
                        }
                        None => {
                            // Channel closed
                            debug!("Log channel closed");
                            break;
                        }
                    }
                }
                _ = interval.tick() => {
                    // Periodic flush
                    if !batch.is_empty() {
                        Self::flush_logs(&mut client, &agent_id, &mut batch).await;
                    }
                }
                _ = self.wait_for_shutdown() => {
                    if !batch.is_empty() {
                        Self::flush_logs(&mut client, &agent_id, &mut batch).await;
                    }
                    break;
                }
            }
        }

        Ok(())
    }

    /// Flush a batch of log entries to the server.
    async fn flush_logs(
        client: &mut ProtoClient<Channel>,
        agent_id: &str,
        batch: &mut Vec<proto::LogEntry>,
    ) {
        if batch.is_empty() {
            return;
        }

        let entries = std::mem::take(batch);
        let count = entries.len();

        // Use client-streaming RPC for log shipping
        let stream = tokio_stream::iter(entries);
        match client.ship_logs(stream).await {
            Ok(response) => {
                let ack = response.into_inner();
                if ack.success {
                    debug!(
                        count,
                        received = ack.received_count,
                        "Flushed logs to server"
                    );
                } else {
                    warn!(
                        error = %ack.error_message,
                        "Server rejected log batch"
                    );
                }
            },
            Err(e) => {
                error!(error = %e, count, "Failed to ship logs");
            },
        }

        let _ = agent_id; // Used in log context
    }

    /// Convert a local LogEntry to the proto LogEntry type.
    fn convert_log_entry(agent_id: &str, entry: &LogEntry) -> proto::LogEntry {
        proto::LogEntry {
            agent_id: agent_id.to_string(),
            site_id: entry.site_id.clone(),
            request_id: entry.request_id.clone(),
            timestamp: Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            }),
            client_ip: entry.client_ip.clone(),
            method: entry.method.clone(),
            scheme: entry.scheme.clone(),
            host: entry.host.clone(),
            path: entry.path.clone(),
            query_string: entry.query_string.clone(),
            protocol: entry.protocol.clone(),
            request_headers: entry.request_headers.clone(),
            request_body: entry.request_body.clone().unwrap_or_default(),
            request_body_size: entry.request_body_size,
            request_body_truncated: entry.request_body_truncated,
            response_status: entry.response_status,
            response_headers: entry.response_headers.clone(),
            response_body: Vec::new(),
            response_body_size: 0,
            response_body_truncated: false,
            upstream_addr: entry.upstream_addr.clone(),
            upstream_latency_ms: entry.upstream_latency_ms,
            waf_score: entry.waf_score,
            waf_action: entry.waf_action.clone(),
            waf_rule_id: entry.waf_rule_id.clone(),
            waf_details: entry.waf_details.clone(),
            waf_matched_tags: entry.waf_matched_tags.clone(),
            total_latency_ms: entry.total_latency_ms,
            cache_status: entry.cache_status.clone(),
            tls_version: entry.tls_version.clone(),
            ja3_hash: entry.ja3_hash.clone(),
            country_code: entry.country_code.clone(),
            city: entry.city.clone(),
            asn: entry.asn,
            user_agent: entry.user_agent.clone(),
            referer: entry.referer.clone(),
        }
    }

    /// Wait until the shutdown signal is set.
    async fn wait_for_shutdown(&self) {
        let shutdown = Arc::clone(&self.shutdown_signal);
        loop {
            if shutdown.load(Ordering::Relaxed) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Signal the client to shut down gracefully.
    pub fn shutdown(&self) {
        info!("Initiating agent shutdown");
        self.shutdown_signal.store(true, Ordering::Relaxed);
    }

    /// Get the current system metrics snapshot.
    pub fn metrics_snapshot(&self) -> SystemMetrics {
        self.metrics.collect()
    }
}

// ─────────────────────────────────────────────────────────────
// Helper functions for system information
// ─────────────────────────────────────────────────────────────

fn get_hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| {
            std::env::var("HOSTNAME")
                .or_else(|_| std::env::var("COMPUTERNAME"))
                .unwrap_or_else(|_| "unknown".to_string())
        })
}

fn get_local_ip() -> String {
    // Attempt to determine local IP by connecting to a public address
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| {
            socket.connect("8.8.8.8:80")?;
            socket.local_addr()
        })
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string())
}

fn get_system_info() -> (u32, u64, String) {
    let cpu_cores = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1);

    let memory_bytes = get_total_memory();

    let os_info =
        format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);

    (cpu_cores, memory_bytes, os_info)
}

fn get_total_memory() -> u64 {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|content| {
                content
                    .lines()
                    .find(|l| l.starts_with("MemTotal:"))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|v| v.parse::<u64>().ok())
                    .map(|kb| kb * 1024)
            })
            .unwrap_or(0)
    }
    #[cfg(target_os = "macos")]
    {
        // Use sysctl to get total memory on macOS
        std::process::Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()
            .and_then(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .parse::<u64>()
                    .ok()
            })
            .unwrap_or(0)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        0
    }
}
