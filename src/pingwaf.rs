// Copyright 2024-2025 Tree xie.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! PingWAF mode startup logic.
//!
//! Handles the three operating modes: Server (control plane only),
//! Agent (data plane only), and AllInOne (both in a single process).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::certificates::{new_certificate_provider, try_update_certificates};
use crate::cli::{
    AgentOpts, AllInOneOpts, PingWafCli, PingWafCommand, ServerOpts,
};
use crate::config_manager::try_init_memory_config_manager;
use crate::locations::{new_location_provider, try_init_locations};
use crate::plugin::new_plugin_provider;
use crate::server_locations::{
    new_server_locations_provider, try_init_server_locations,
};
use crate::upstreams::{new_upstream_provider, try_init_upstreams};
use pingap_config::{
    BasicConf, CertificateConf, LocationConf, PingapConfig,
    ServerConf as PingapServerConf, UpstreamConf,
};
use pingap_proxy::{
    AppContext, Server as ProxyServer, ServerConf, parse_from_conf,
};
use pingap_upstream::new_upstream_health_check_task;
use pingora::server;
use pingora::server::configuration::Opt;
use pingora::services::background::background_service;
use pingwaf_agent::cache::{CachedRules, RuleCache};
use pingwaf_agent::config::AgentConfig;
use pingwaf_server::ServerConfig;
use tracing::{error, info, warn};

/// The operating mode for PingWAF.
pub enum RunMode {
    /// Control plane only: REST API + gRPC server + PostgreSQL
    Server(ServerConfig),
    /// Data plane only: connects to a remote control plane
    Agent(AgentConfig),
    /// Both control plane and data plane in a single process
    AllInOne(ServerConfig, AgentConfig),
}

/// Convert CLI server options into a `ServerConfig`.
fn server_config_from_opts(opts: &ServerOpts) -> ServerConfig {
    let mut config = ServerConfig::from_env();
    // CLI flags override environment
    config.db_url = opts.common.db_url.clone();
    config.http_addr = opts.common.admin_addr.clone();
    config.grpc_addr = opts.common.grpc_addr.clone();
    config.jwt_secret = opts.jwt_secret.clone();
    config.default_admin_email = opts.admin_email.clone();
    config.default_admin_password = opts.admin_password.clone();
    config
}

/// Convert CLI all-in-one options into a `ServerConfig`.
fn server_config_from_all_in_one(opts: &AllInOneOpts) -> ServerConfig {
    let mut config = ServerConfig::from_env();
    config.db_url = opts.db_url.clone();
    config.http_addr = opts.admin_addr.clone();
    config.grpc_addr = opts.grpc_addr.clone();
    config.jwt_secret = opts.jwt_secret.clone();
    config.default_admin_email = opts.admin_email.clone();
    config.default_admin_password = opts.admin_password.clone();
    config
}

/// Convert CLI agent options into an `AgentConfig`.
fn agent_config_from_opts(opts: &AgentOpts) -> AgentConfig {
    AgentConfig {
        server_url: opts.server_url.clone(),
        api_key: opts.api_key.clone(),
        agent_id: String::new(),
        heartbeat_interval_secs: opts.heartbeat_interval_secs,
        cache_dir: opts.cache_dir.clone(),
        log_batch_size: opts.log_batch_size,
        log_flush_interval_secs: opts.log_flush_interval_secs,
        max_body_log_size: opts.max_body_log_size,
        reconnect_initial_delay_ms: 1000,
        reconnect_max_delay_ms: 60000,
        fail_open: opts.fail_open,
        probe_interval_secs: pingwaf_agent::probe::DEFAULT_INTERVAL_SECS,
        probe_disk_path: "/".to_string(),
    }
}

/// Convert CLI all-in-one options into an `AgentConfig`.
///
/// In all-in-one mode the agent connects to the local server via loopback.
fn agent_config_from_all_in_one(opts: &AllInOneOpts) -> AgentConfig {
    // Derive the loopback gRPC URL from the configured gRPC address
    let server_url =
        format!("http://127.0.0.1:{}", extract_port(&opts.grpc_addr));
    AgentConfig {
        server_url,
        api_key: opts.api_key.clone(),
        agent_id: String::new(),
        heartbeat_interval_secs: opts.heartbeat_interval_secs,
        cache_dir: opts.cache_dir.clone(),
        log_batch_size: opts.log_batch_size,
        log_flush_interval_secs: opts.log_flush_interval_secs,
        max_body_log_size: opts.max_body_log_size,
        reconnect_initial_delay_ms: 1000,
        reconnect_max_delay_ms: 60000,
        fail_open: opts.fail_open,
        probe_interval_secs: pingwaf_agent::probe::DEFAULT_INTERVAL_SECS,
        probe_disk_path: "/".to_string(),
    }
}

/// Extract port from an address string like "0.0.0.0:9090".
fn extract_port(addr: &str) -> &str {
    addr.rsplit(':').next().unwrap_or("9090")
}

/// Build the `RunMode` from the parsed CLI.
pub fn build_run_mode(cli: PingWafCli) -> RunMode {
    match cli.command {
        PingWafCommand::Server(ref opts) => {
            RunMode::Server(server_config_from_opts(opts))
        },
        PingWafCommand::Agent(ref opts) => {
            RunMode::Agent(agent_config_from_opts(opts))
        },
        PingWafCommand::AllInOne(ref opts) => {
            let server_config = server_config_from_all_in_one(opts);
            let agent_config = agent_config_from_all_in_one(opts);
            RunMode::AllInOne(server_config, agent_config)
        },
    }
}

/// Sanitize a control-plane LB algorithm into a pingap `algo` value.
///
/// pingap accepts only `round_robin` (its default) and
/// `hash:<type>[:<key>]` with type in {ip, url, path, header, cookie,
/// query}; anything else — including legacy enum Debug names such as
/// `lbconsistenthash` — is a hard error at upstream construction, so it
/// must collapse to the default here.
fn sanitize_algo(algo: &str) -> Option<String> {
    let algo = algo.trim();
    if algo.is_empty() || algo == "round_robin" {
        return None;
    }
    let spec = algo.strip_prefix("hash:")?;
    let hash_type = spec.split(':').next().unwrap_or_default();
    if matches!(
        hash_type,
        "ip" | "url" | "path" | "header" | "cookie" | "query"
    ) {
        Some(algo.to_string())
    } else {
        None
    }
}

/// Build a `PingapConfig` from the agent's cached site rules.
///
/// Each origin pool becomes one upstream keyed by its pool id (or the
/// legacy per-site name for caches written before pools existed). The
/// default pool — or the only pool, when no routes exist — receives the
/// site's `/` fallback location, and every enabled route becomes an
/// additional location carrying the pingap path marker for its match
/// type. All sites share a single server listening on ports 80 and 443
/// with TLS enabled via the global certificate store.
fn cached_rules_to_pingap_config(cached: &CachedRules) -> Option<PingapConfig> {
    if cached.sites.is_empty() {
        return None;
    }

    let mut upstreams: HashMap<String, UpstreamConf> = HashMap::new();
    let mut locations: HashMap<String, LocationConf> = HashMap::new();
    let mut certificates: HashMap<String, CertificateConf> = HashMap::new();
    let mut location_names: Vec<String> = Vec::new();

    for (site_id, site) in &cached.sites {
        if site.domain.is_empty() {
            continue;
        }

        // One upstream per origin pool. Pools without peers are skipped:
        // an address-less upstream cannot be constructed.
        let mut pool_keys: Vec<String> = Vec::new();
        let mut default_key: Option<String> = None;
        for (i, up) in site.upstreams.iter().enumerate() {
            if up.peers.is_empty() {
                continue;
            }
            let name = if up.pool_id.is_empty() {
                if site.upstreams.len() == 1 {
                    format!("{site_id}_upstream")
                } else {
                    format!("{site_id}_upstream_{i}")
                }
            } else {
                up.pool_id.clone()
            };

            let addrs: Vec<String> = up
                .peers
                .iter()
                .map(|p| {
                    let addr = p
                        .address
                        .strip_prefix("https://")
                        .or_else(|| p.address.strip_prefix("http://"))
                        .unwrap_or(&p.address);
                    if p.weight > 1 {
                        format!("{} {}", addr, p.weight)
                    } else {
                        addr.to_string()
                    }
                })
                .collect();

            let conf = UpstreamConf {
                addrs,
                algo: sanitize_algo(&up.algo),
                sni: if up.sni.is_empty() {
                    None
                } else {
                    Some(up.sni.clone())
                },
                verify_cert: up.verify_cert,
                connection_timeout: if up.connection_timeout_ms > 0 {
                    Some(Duration::from_millis(up.connection_timeout_ms as u64))
                } else {
                    None
                },
                read_timeout: if up.read_timeout_ms > 0 {
                    Some(Duration::from_millis(up.read_timeout_ms as u64))
                } else {
                    None
                },
                write_timeout: if up.write_timeout_ms > 0 {
                    Some(Duration::from_millis(up.write_timeout_ms as u64))
                } else {
                    None
                },
                health_check: up
                    .health_check
                    .as_ref()
                    .filter(|h| h.enabled)
                    .map(|h| h.path.clone()),
                ..Default::default()
            };
            if up.is_default {
                default_key = Some(name.clone());
            }
            pool_keys.push(name.clone());
            upstreams.insert(name, conf);
        }

        // The `/` fallback location points at the default pool. Caches
        // written before pools existed mark nothing default, so the only
        // pool serves when no routes exist.
        let fallback_key = default_key.or_else(|| {
            if site.routes.is_empty() {
                pool_keys.first().cloned()
            } else {
                None
            }
        });

        let host = if site.alternate_domains.is_empty() {
            site.domain.clone()
        } else {
            let mut hosts = vec![site.domain.clone()];
            hosts.extend(site.alternate_domains.clone());
            hosts.join(",")
        };

        if let Some(key) = fallback_key {
            let loc_name = format!("{site_id}_loc");
            locations.insert(
                loc_name.clone(),
                LocationConf {
                    upstream: Some(key),
                    host: Some(host.clone()),
                    path: Some("/".to_string()),
                    ..Default::default()
                },
            );
            location_names.push(loc_name);
        }

        for route in &site.routes {
            // Route only to pools this site actually built — an empty
            // pool has no upstream to receive traffic.
            if !route.enabled || !pool_keys.contains(&route.pool_id) {
                continue;
            }
            let path = match route.match_type.as_str() {
                "exact" => format!("={}", route.path),
                "regex" => format!("~{}", route.path),
                _ => route.path.clone(),
            };
            let loc_name = format!("{site_id}_route_{}", route.id);
            locations.insert(
                loc_name.clone(),
                LocationConf {
                    upstream: Some(route.pool_id.clone()),
                    host: Some(host.clone()),
                    path: Some(path),
                    weight: route
                        .priority
                        .map(|p| p.clamp(1, u16::MAX as i32) as u16),
                    ..Default::default()
                },
            );
            location_names.push(loc_name);
        }

        // Convert SSL certificate
        if let Some(ref ssl) = site.ssl_config
            && ssl.enabled
            && !ssl.cert_pem.is_empty()
        {
            let cert_name = format!("{site_id}_cert");
            let domains = if site.alternate_domains.is_empty() {
                site.domain.clone()
            } else {
                let mut d = vec![site.domain.clone()];
                d.extend(site.alternate_domains.clone());
                d.join(",")
            };
            let cert = CertificateConf {
                domains: Some(domains),
                tls_cert: Some(ssl.cert_pem.clone()),
                tls_key: Some(ssl.key_pem.clone()),
                ..Default::default()
            };
            certificates.insert(cert_name, cert);
        }
    }

    if location_names.is_empty() {
        return None;
    }

    // Single server listening on 80 and 443
    let has_certs = !certificates.is_empty();
    let mut servers: HashMap<String, PingapServerConf> = HashMap::new();
    let server_conf = PingapServerConf {
        addr: "0.0.0.0:80,0.0.0.0:443".to_string(),
        locations: Some(location_names),
        global_certificates: Some(has_certs),
        ..Default::default()
    };
    servers.insert("pingwaf".to_string(), server_conf);

    Some(PingapConfig {
        basic: BasicConf::default(),
        upstreams,
        locations,
        servers,
        certificates,
        ..Default::default()
    })
}

/// Build a `PingapConfig` from the agent's cached rules.
fn build_pingap_config(rule_cache: &RuleCache) -> Option<PingapConfig> {
    cached_rules_to_pingap_config(&rule_cache.all_sites())
}

/// Start the Pingora-based data plane proxy from the agent's cached rules.
///
/// Waits for the agent to receive its initial configuration from the
/// control plane, then builds a PingapConfig and starts the reverse proxy
/// on ports 80 and 443.
pub async fn start_data_plane(
    rule_cache: Arc<RuleCache>,
) -> anyhow::Result<()> {
    // Wait for the agent to receive initial config from the control plane
    for i in 0..30 {
        let cached = rule_cache.all_sites();
        if !cached.sites.is_empty() {
            break;
        }
        if i == 0 {
            info!("data plane: waiting for control plane configuration...");
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    let config = match build_pingap_config(&rule_cache) {
        Some(config) => config,
        None => {
            warn!("data plane: no sites configured, proxy not started");
            // Keep waiting — the agent may receive config later
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                if let Some(config) = build_pingap_config(&rule_cache) {
                    break config;
                }
            }
        },
    };

    let site_count = config
        .servers
        .values()
        .flat_map(|s| s.locations.as_deref().unwrap_or_default())
        .count();
    info!(
        upstreams = config.upstreams.len(),
        locations = config.locations.len(),
        certificates = config.certificates.len(),
        "data plane: starting reverse proxy with {} site(s)",
        site_count,
    );

    // Serialize to TOML and create a memory-backed config manager
    let toml_str = toml::to_string_pretty(&config).map_err(|e| {
        anyhow::anyhow!("failed to serialize proxy config: {}", e)
    })?;
    let config_manager = try_init_memory_config_manager(&toml_str, None)
        .map_err(|e| anyhow::anyhow!("failed to init config manager: {}", e))?;

    // Initialize providers from the converted config
    try_init_upstreams(&config.upstreams, None)
        .map_err(|e| anyhow::anyhow!("failed to init upstreams: {}", e))?;
    try_init_locations(&config.locations)
        .map_err(|e| anyhow::anyhow!("failed to init locations: {}", e))?;
    try_init_server_locations(&config.servers, &config.locations).map_err(
        |e| anyhow::anyhow!("failed to init server locations: {}", e),
    )?;

    // Initialize certificates
    let cert_provider = new_certificate_provider();
    if !config.certificates.is_empty() {
        let (updated, errors) = try_update_certificates(&config.certificates);
        if !updated.is_empty() {
            info!(certs = updated.join(","), "data plane: certificates loaded");
        }
        if !errors.is_empty() {
            error!(error = errors, "data plane: certificate parse errors");
        }
    }

    // Create the Pingora server
    let opt = Opt::default();
    let mut my_server = server::Server::new(Some(opt))?;
    let bootstrap_handle = my_server.bootstrap_as_a_service();

    // Parse server configs and start proxy servers
    let server_conf_list: Vec<ServerConf> = parse_from_conf(config);

    for server_conf in server_conf_list {
        let ctx = AppContext {
            server_locations_provider: new_server_locations_provider(),
            location_provider: new_location_provider(),
            upstream_provider: new_upstream_provider(),
            plugin_provider: new_plugin_provider(),
            certificate_provider: cert_provider.clone(),
            config_manager: config_manager.clone(),
            logger: None,
        };
        let ps = ProxyServer::new(&server_conf, ctx)?;
        let services = ps.run(my_server.configuration.clone())?;
        my_server
            .add_service(services.lb)
            .add_dependency(&bootstrap_handle);
    }

    info!("data plane: proxy server is running on 0.0.0.0:80,0.0.0.0:443");

    // Start the upstream health check background task.
    // This also drives periodic DNS discovery updates — without it,
    // DNS-based backends are never resolved and requests get 503.
    let upstream_health_check_task = new_upstream_health_check_task(
        new_upstream_provider(),
        Duration::from_secs(10),
        None,
    );
    let hc_name = upstream_health_check_task.name().to_string();
    info!(
        service_name = %hc_name,
        "data plane: registering upstream health check background service"
    );
    my_server
        .add_service(background_service(&hc_name, upstream_health_check_task));

    // Run Pingora in a dedicated OS thread. Do NOT join it here —
    // joining would block the tokio worker thread permanently and starve
    // the control-plane REST/gRPC servers (especially on small machines).
    // The thread exits when the process shuts down.
    std::thread::spawn(move || {
        my_server.run_forever();
    });

    // Keep the async task alive until the tokio runtime shuts down.
    // The caller (run()) aborts this task on shutdown signal.
    std::future::pending::<()>().await;
    Ok(())
}

/// Run PingWAF in the specified mode.
///
/// This function blocks until shutdown is signalled (SIGINT / SIGTERM).
pub async fn run(mode: RunMode) -> anyhow::Result<()> {
    match mode {
        RunMode::Server(config) => {
            info!(
                http_addr = %config.http_addr,
                grpc_addr = %config.grpc_addr,
                "starting PingWAF in Server mode (control plane only)"
            );
            pingwaf_server::start_server(config).await
        },
        RunMode::Agent(config) => {
            info!(
                server_url = %config.server_url,
                cache_dir = %config.cache_dir,
                "starting PingWAF in Agent mode (data plane only)"
            );
            let agent = pingwaf_agent::start_agent(config).await?;
            info!("PingWAF agent is running, starting data plane proxy");

            // Start the data plane proxy in the background
            let rule_cache = Arc::clone(&agent.rule_cache);
            let proxy_handle = tokio::spawn(async move {
                if let Err(e) = start_data_plane(rule_cache).await {
                    error!(error = %e, "data plane proxy exited with error");
                }
            });

            // Wait for shutdown signal
            shutdown_signal().await;
            agent.shutdown().await;
            proxy_handle.abort();
            Ok(())
        },
        RunMode::AllInOne(server_config, mut agent_config) => {
            info!(
                http_addr = %server_config.http_addr,
                grpc_addr = %server_config.grpc_addr,
                "starting PingWAF in All-in-One mode (control plane + data plane)"
            );

            // Auto-provision a bootstrap API key when none is configured so the
            // embedded agent can authenticate with the local gRPC server.
            if agent_config.api_key.is_empty() {
                match bootstrap_agent_key(&server_config).await {
                    Ok(key) => {
                        agent_config.api_key = key;
                    },
                    Err(e) => {
                        error!(error = %e, "failed to create bootstrap API key for embedded agent");
                    },
                }
            }

            // Start the agent in the background first.
            // It will retry connecting to the server until it comes up.
            let agent = match pingwaf_agent::start_agent(agent_config).await {
                Ok(agent) => Some(agent),
                Err(e) => {
                    error!(error = %e, "failed to start agent, continuing with server only");
                    None
                },
            };

            // Start the data plane proxy if the agent is running
            let proxy_handle = if let Some(ref agent) = agent {
                let rule_cache = Arc::clone(&agent.rule_cache);
                Some(tokio::spawn(async move {
                    if let Err(e) = start_data_plane(rule_cache).await {
                        error!(error = %e, "data plane proxy exited with error");
                    }
                }))
            } else {
                None
            };

            // Run the server (blocks until shutdown signal)
            let result = pingwaf_server::start_server(server_config).await;

            // Gracefully shut down the agent
            if let Some(agent) = agent {
                agent.shutdown().await;
            }
            if let Some(handle) = proxy_handle {
                handle.abort();
            }

            result
        },
    }
}

/// Connect to the database, run migrations, and create a bootstrap API key
/// for the embedded agent in all-in-one mode.
async fn bootstrap_agent_key(
    config: &pingwaf_server::ServerConfig,
) -> anyhow::Result<String> {
    pingwaf_server::bootstrap_and_seed_api_key(config).await
}

/// Wait for SIGINT or SIGTERM.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::terminate(),
        )
        .expect("failed to install SIGTERM handler")
        .recv()
        .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => info!("received SIGINT"),
        _ = terminate => info!("received SIGTERM"),
    }
}

/// Entry point called from main.rs when PingWAF mode is detected.
pub fn main() {
    // Initialize tracing
    init_tracing();

    let cli = crate::cli::parse_pingwaf_cli();
    let mode = build_run_mode(cli);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to create tokio runtime");

    if let Err(e) = rt.block_on(run(mode)) {
        error!(error = %e, "PingWAF exited with error");
        eprintln!("PingWAF error: {e}");
        std::process::exit(1);
    }
}

/// Initialize tracing subscriber for PingWAF modes.
fn init_tracing() {
    use tracing_subscriber::{EnvFilter, fmt, prelude::*};

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(true))
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pingwaf_agent::cache::{
        CachedRules, RouteConfig, SiteRules, UpstreamConfig, UpstreamPeer,
    };

    fn site_rules(site_id: &str, domain: &str) -> SiteRules {
        SiteRules {
            site_id: site_id.to_string(),
            domain: domain.to_string(),
            alternate_domains: vec![],
            waf_config: None,
            rate_limit_rules: vec![],
            ip_access_rules: vec![],
            geo_config: None,
            cache_rules: vec![],
            challenge_config: None,
            rewrite_rules: vec![],
            error_pages: vec![],
            ssl_config: None,
            upstreams: vec![],
            routes: vec![],
        }
    }

    fn pool(pool_id: &str, peers: Vec<UpstreamPeer>) -> UpstreamConfig {
        UpstreamConfig {
            name: pool_id.to_string(),
            peers,
            algorithm: String::new(),
            health_check: None,
            connection_timeout_ms: 0,
            read_timeout_ms: 0,
            write_timeout_ms: 0,
            pool_id: pool_id.to_string(),
            algo: String::new(),
            sni: String::new(),
            verify_cert: None,
            is_default: false,
        }
    }

    fn peer(address: &str) -> UpstreamPeer {
        UpstreamPeer {
            address: address.to_string(),
            weight: 1,
            tls: false,
        }
    }

    fn route(
        id: &str,
        match_type: &str,
        path: &str,
        pool_id: &str,
    ) -> RouteConfig {
        RouteConfig {
            id: id.to_string(),
            name: id.to_string(),
            match_type: match_type.to_string(),
            path: path.to_string(),
            priority: None,
            enabled: true,
            pool_id: pool_id.to_string(),
        }
    }

    fn one_site_cache(site: SiteRules) -> CachedRules {
        let mut cached = CachedRules::default();
        cached.sites.insert(site.site_id.clone(), Arc::new(site));
        cached
    }

    #[test]
    fn single_pool_without_routes_matches_legacy_layout() {
        let mut site = site_rules("site1", "a.example.com");
        site.upstreams = vec![pool("", vec![peer("10.0.0.1:8080")])];
        let config = cached_rules_to_pingap_config(&one_site_cache(site))
            .expect("config should build");

        let upstream = config
            .upstreams
            .get("site1_upstream")
            .expect("legacy upstream key");
        assert_eq!(upstream.addrs, vec!["10.0.0.1:8080"]);
        assert_eq!(upstream.algo, None);

        let loc = config
            .locations
            .get("site1_loc")
            .expect("fallback location");
        assert_eq!(loc.path.as_deref(), Some("/"));
        assert_eq!(loc.upstream.as_deref(), Some("site1_upstream"));
        assert_eq!(loc.host.as_deref(), Some("a.example.com"));
        assert_eq!(loc.weight, None);
    }

    #[test]
    fn legacy_multi_pool_cache_falls_back_to_first_pool() {
        // Before pools existed each upstream entry overwrote the site's
        // single location; now the first pool deterministically serves.
        let mut site = site_rules("site1", "a.example.com");
        site.upstreams = vec![
            pool("", vec![peer("10.0.0.1:8080")]),
            pool("", vec![peer("10.0.0.2:8080")]),
        ];
        let config = cached_rules_to_pingap_config(&one_site_cache(site))
            .expect("config should build");

        assert!(config.upstreams.contains_key("site1_upstream_0"));
        assert!(config.upstreams.contains_key("site1_upstream_1"));
        let loc = config.locations.get("site1_loc").expect("location");
        assert_eq!(loc.upstream.as_deref(), Some("site1_upstream_0"));
        assert_eq!(config.locations.len(), 1);
    }

    #[test]
    fn pools_and_routes_build_separate_locations() {
        let mut site = site_rules("site1", "a.example.com");
        let mut default_pool = pool(
            "pool-a",
            vec![
                peer("10.0.0.1:8080"),
                UpstreamPeer {
                    address: "10.0.0.2:8080".to_string(),
                    weight: 3,
                    tls: false,
                },
            ],
        );
        default_pool.is_default = true;
        let second_pool = pool("pool-b", vec![peer("10.0.0.3:9090")]);
        // Empty pools are skipped entirely.
        site.upstreams =
            vec![default_pool, second_pool, pool("pool-empty", vec![])];

        let mut r1 = route("r1", "prefix", "/api", "pool-b");
        r1.priority = Some(100);
        let r2 = route("r2", "exact", "/healthz", "pool-a");
        let r3 = route("r3", "regex", r"^/static/.*", "pool-b");
        let mut disabled = route("r4", "prefix", "/gone", "pool-b");
        disabled.enabled = false;
        // Route referencing the skipped empty pool is dropped.
        let orphan = route("r5", "prefix", "/orphan", "pool-empty");
        site.routes = vec![r1, r2, r3, disabled, orphan];

        let config = cached_rules_to_pingap_config(&one_site_cache(site))
            .expect("config should build");

        assert_eq!(config.upstreams.len(), 2);
        let default_upstream =
            config.upstreams.get("pool-a").expect("pool-a upstream");
        assert!(
            default_upstream
                .addrs
                .contains(&"10.0.0.2:8080 3".to_string())
        );

        let fallback = config
            .locations
            .get("site1_loc")
            .expect("default pool fallback location");
        assert_eq!(fallback.upstream.as_deref(), Some("pool-a"));
        assert_eq!(fallback.path.as_deref(), Some("/"));
        assert_eq!(fallback.weight, None);

        let api = config
            .locations
            .get("site1_route_r1")
            .expect("prefix route location");
        assert_eq!(api.path.as_deref(), Some("/api"));
        assert_eq!(api.upstream.as_deref(), Some("pool-b"));
        assert_eq!(api.weight, Some(100));

        let healthz = config
            .locations
            .get("site1_route_r2")
            .expect("exact route location");
        assert_eq!(healthz.path.as_deref(), Some("=/healthz"));
        assert_eq!(healthz.upstream.as_deref(), Some("pool-a"));
        assert_eq!(healthz.weight, None);

        let static_route = config
            .locations
            .get("site1_route_r3")
            .expect("regex route location");
        assert_eq!(static_route.path.as_deref(), Some("~^/static/.*"));

        assert!(!config.locations.contains_key("site1_route_r4"));
        assert!(!config.locations.contains_key("site1_route_r5"));
    }

    #[test]
    fn sni_verify_cert_and_algo_pass_through() {
        let mut site = site_rules("site1", "a.example.com");
        let mut tls_pool = pool("pool-a", vec![peer("https://10.0.0.1:8443")]);
        tls_pool.is_default = true;
        tls_pool.sni = "origin.example.com".to_string();
        tls_pool.verify_cert = Some(false);
        tls_pool.algo = "hash:cookie:session".to_string();
        site.upstreams = vec![tls_pool];

        let mut legacy_pool = pool("pool-b", vec![peer("10.0.0.2:8080")]);
        legacy_pool.algo = "lbconsistenthash".to_string();
        site.upstreams.push(legacy_pool);

        let config = cached_rules_to_pingap_config(&one_site_cache(site))
            .expect("config should build");

        let tls = config.upstreams.get("pool-a").expect("tls pool");
        assert_eq!(tls.sni.as_deref(), Some("origin.example.com"));
        assert_eq!(tls.verify_cert, Some(false));
        assert_eq!(
            tls.algo.as_deref(),
            Some("hash:cookie:session"),
            "valid hash algo passes through"
        );
        assert_eq!(
            tls.addrs,
            vec!["10.0.0.1:8443"],
            "scheme is stripped from peer addresses"
        );

        assert_eq!(
            config.upstreams.get("pool-b").unwrap().algo,
            None,
            "legacy enum debug names collapse to the round_robin default"
        );
    }

    #[test]
    fn sanitize_algo_rejects_unknown_names() {
        assert_eq!(sanitize_algo(""), None);
        assert_eq!(sanitize_algo("round_robin"), None);
        assert_eq!(sanitize_algo("least_connections"), None);
        assert_eq!(sanitize_algo("random"), None);
        assert_eq!(sanitize_algo("lbconsistenthash"), None);
        assert_eq!(sanitize_algo("hash:"), None);
        assert_eq!(sanitize_algo("hash:bogus"), None);
        assert_eq!(sanitize_algo("hash:ip"), Some("hash:ip".to_string()));
        assert_eq!(sanitize_algo("hash:url"), Some("hash:url".to_string()));
        assert_eq!(sanitize_algo("hash:path"), Some("hash:path".to_string()));
        assert_eq!(
            sanitize_algo("hash:header:x-user"),
            Some("hash:header:x-user".to_string())
        );
        assert_eq!(
            sanitize_algo("hash:cookie:session_id"),
            Some("hash:cookie:session_id".to_string())
        );
        assert_eq!(
            sanitize_algo("hash:query:q"),
            Some("hash:query:q".to_string())
        );
    }

    #[test]
    fn empty_cache_and_empty_sites_produce_no_config() {
        assert!(
            cached_rules_to_pingap_config(&CachedRules::default()).is_none()
        );

        let mut site = site_rules("site1", "");
        site.upstreams = vec![pool("", vec![peer("10.0.0.1:8080")])];
        assert!(cached_rules_to_pingap_config(&one_site_cache(site)).is_none());
    }
}
