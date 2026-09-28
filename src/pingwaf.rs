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
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::certificates::{new_certificate_provider, try_update_certificates};
use crate::cli::{
    AgentOpts, AllInOneOpts, PingWafCli, PingWafCommand, ServerOpts,
};
use crate::config_manager::try_init_memory_config_manager;
use crate::locations::{new_location_provider, try_init_locations};
use crate::plugin::{new_plugin_provider, try_init_plugins};
use crate::server_locations::{
    new_server_locations_provider, try_init_server_locations,
};
use crate::upstreams::{new_upstream_provider, try_init_upstreams};
use pingap_acme::new_lets_encrypt_service;
use pingap_config::{
    BasicConf, CertificateConf, LocationConf, PingapConfig, PingapTomlConfig,
    PluginConf, ServerConf as PingapServerConf, UpstreamConf,
    normalize_dns_provider,
};
use pingap_core::BackgroundTaskService;
use pingap_proxy::{
    AppContext, Server as ProxyServer, ServerConf, parse_from_conf,
};
use pingap_upstream::new_upstream_health_check_task;
use pingora::server;
use pingora::server::configuration::Opt;
use pingora::services::background::background_service;
use pingwaf_agent::cache::{CachedRules, RuleCache, SslConfig};
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

/// Name under which the WAF plugin is registered and referenced from
/// every generated location.
const WAF_PLUGIN_NAME: &str = "pingwaf:waf";

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
    // The WAF plugin is referenced from every location: this is what makes
    // the data plane enforce rules and emit access/security events.
    let mut plugins: HashMap<String, PluginConf> = HashMap::new();
    plugins.insert(
        WAF_PLUGIN_NAME.to_string(),
        toml::from_str::<PluginConf>(r###"category = "waf""###)
            .unwrap_or_default(),
    );

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
                    plugins: Some(vec![WAF_PLUGIN_NAME.to_string()]),
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
                    plugins: Some(vec![WAF_PLUGIN_NAME.to_string()]),
                    ..Default::default()
                },
            );
            location_names.push(loc_name);
        }

        // Convert SSL certificate: an uploaded PEM is served as-is, while a
        // site asking for ACME without one gets a certificate entry the
        // lets-encrypt task issues against — and later fills in — inside
        // this config.
        if let Some(ref ssl) = site.ssl_config
            && ssl.enabled
        {
            let domains = if site.alternate_domains.is_empty() {
                site.domain.clone()
            } else {
                let mut d = vec![site.domain.clone()];
                d.extend(site.alternate_domains.clone());
                d.join(",")
            };
            let cert = if !ssl.cert_pem.is_empty() {
                Some(CertificateConf {
                    domains: Some(domains),
                    tls_cert: Some(ssl.cert_pem.clone()),
                    tls_key: Some(ssl.key_pem.clone()),
                    ..Default::default()
                })
            } else if ssl.acme_enabled && !ssl.acme_email.is_empty() {
                Some(acme_certificate_conf(ssl, domains))
            } else {
                None
            };
            if let Some(cert) = cert {
                certificates.insert(format!("{site_id}_cert"), cert);
            }
        }
    }

    if location_names.is_empty() {
        return None;
    }

    // TLS is a per-server switch in pingap: `global_certificates` turns it on
    // for every address that server listens on, so 80 and 443 cannot share
    // one server — a merged listener ran plaintext HTTP through TLS
    // handshakes, which also broke ACME HTTP-01 validation on port 80.
    let has_certs = !certificates.is_empty();
    let mut servers: HashMap<String, PingapServerConf> = HashMap::new();
    if has_certs {
        servers.insert(
            "pingwaf".to_string(),
            PingapServerConf {
                addr: "0.0.0.0:80".to_string(),
                locations: Some(location_names.clone()),
                global_certificates: Some(false),
                ..Default::default()
            },
        );
        servers.insert(
            "pingwaf_tls".to_string(),
            PingapServerConf {
                addr: "0.0.0.0:443".to_string(),
                locations: Some(location_names),
                global_certificates: Some(true),
                ..Default::default()
            },
        );
    } else {
        // Nothing to serve over TLS yet; both ports stay plaintext until the
        // first certificate lands and the next reload splits the listeners.
        servers.insert(
            "pingwaf".to_string(),
            PingapServerConf {
                addr: "0.0.0.0:80,0.0.0.0:443".to_string(),
                locations: Some(location_names),
                ..Default::default()
            },
        );
    }

    Some(PingapConfig {
        basic: BasicConf::default(),
        upstreams,
        locations,
        servers,
        certificates,
        plugins,
        ..Default::default()
    })
}

/// Build a `PingapConfig` from the agent's cached rules.
fn build_pingap_config(rule_cache: &RuleCache) -> Option<PingapConfig> {
    cached_rules_to_pingap_config(&rule_cache.all_sites())
}

/// Build the certificate entry a lets-encrypt issuance is driven by.
///
/// `acme` is only a marker (never parsed); the email keeps the convention
/// legacy pingap used so operators recognize it in a config dump. DNS-01
/// carries the provider plus credentials: the endpoint defaults to the
/// provider's API host and every `acme_dns_config` entry becomes a query
/// pair — the layout pingap's DNS tasks parse.
fn acme_certificate_conf(ssl: &SslConfig, domains: String) -> CertificateConf {
    let mut cert = CertificateConf {
        domains: Some(domains),
        acme: Some(format!("http://{}", ssl.acme_email)),
        ..Default::default()
    };
    if ssl.acme_challenge_type != "AcmeDns01" {
        return cert;
    }
    let Some(provider) = normalize_dns_provider(&ssl.acme_dns_provider) else {
        // Unknown provider: stay on HTTP-01 rather than emit an entry the
        // config validation rejects outright.
        return cert;
    };
    if provider == "manual" {
        return cert;
    }
    cert.dns_challenge = Some(true);
    cert.dns_provider = Some(provider.to_string());
    if let Some(url) = acme_dns_service_url(provider, &ssl.acme_dns_config) {
        cert.dns_service_url = Some(url);
    }
    cert
}

/// The DNS provider endpoint with credentials appended as query pairs.
///
/// An explicit `endpoint` entry (full URL, or host getting the default
/// scheme) wins; `region` is only used to build the Huawei cloud host.
/// Without either, the provider's documented API host applies — Huawei has
/// per-region hosts and cannot have one. Every other entry becomes
/// `key=value` — names match what the provider task expects, e.g.
/// `access_key_id` / `access_key_secret` / `token`.
fn acme_dns_service_url(
    provider: &str,
    config: &HashMap<String, String>,
) -> Option<String> {
    let base = match config.get("endpoint").filter(|v| !v.is_empty()) {
        Some(v) if v.contains("://") => v.clone(),
        Some(v) => format!("https://{v}"),
        None => {
            if let Some(region) = config.get("region").filter(|r| !r.is_empty())
            {
                format!("https://dns.{region}.myhuaweicloud.com")
            } else if provider == "huawei" {
                warn!(
                    "acme dns-01: huawei needs a region or endpoint, \
                     issuance will need manual DNS"
                );
                return None;
            } else {
                // Providers accepted by acme_certificate_conf all have a
                // documented host; the tasks error out on an empty URL.
                let host = match provider {
                    "ali" => "alidns.aliyuncs.com",
                    "cf" => "api.cloudflare.com",
                    "tencent" => "dnspod.tencentcloudapi.com",
                    _ => {
                        warn!(
                            "acme dns-01: no endpoint configured for \
                             {provider}, issuance will need manual DNS"
                        );
                        return None;
                    },
                };
                format!("https://{host}")
            }
        },
    };
    let mut pairs: Vec<String> = config
        .iter()
        .filter(|(k, _)| !matches!(k.as_str(), "endpoint" | "region"))
        .map(|(k, v)| {
            format!("{}={}", urlencoding::encode(k), urlencoding::encode(v))
        })
        .collect();
    pairs.sort();
    if pairs.is_empty() {
        return Some(base);
    }
    Some(format!("{base}?{}", pairs.join("&")))
}

/// The agent's ACME state file: the memory config manager mirrors every
/// write there, so issued certificates and challenge tokens survive a
/// restart (Let's Encrypt caps duplicate certificates per week, and a
/// stateless restart would burn that quota).
fn acme_state_path(rule_cache: &RuleCache) -> PathBuf {
    rule_cache.cache_dir().join("acme_state.toml")
}

/// Merge a freshly converted config with a previous one, carrying over the
/// state only the lets-encrypt task produces: issued PEMs (filled into
/// certificate entries the new config left empty) and the whole `storages`
/// section — challenge tokens and ACME account credentials. Uploads always
/// win: a new config that carries its own PEM is never overwritten.
fn merge_config_state(new_toml: &str, prev_toml: &str) -> String {
    if prev_toml.trim().is_empty() {
        return new_toml.to_string();
    }
    let Ok(mut doc) = new_toml.parse::<toml::Table>() else {
        return new_toml.to_string();
    };
    let Ok(prev) = prev_toml.parse::<toml::Table>() else {
        return new_toml.to_string();
    };

    if let Some(prev_certs) =
        prev.get("certificates").and_then(|v| v.as_table())
    {
        let certs = doc
            .entry("certificates")
            .or_insert(toml::Value::Table(toml::Table::new()));
        if let Some(certs) = certs.as_table_mut() {
            for (name, prev_cert) in prev_certs {
                let pem = prev_cert
                    .get("tls_cert")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if pem.is_empty() {
                    continue;
                }
                let Some(cert) = certs.get_mut(name) else {
                    continue;
                };
                let has_pem = cert
                    .get("tls_cert")
                    .and_then(|v| v.as_str())
                    .is_some_and(|v| !v.is_empty());
                if has_pem {
                    continue;
                }
                if let Some(cert) = cert.as_table_mut() {
                    cert.insert(
                        "tls_cert".to_string(),
                        toml::Value::String(pem.to_string()),
                    );
                    if let Some(key) = prev_cert.get("tls_key") {
                        cert.insert("tls_key".to_string(), key.clone());
                    }
                }
            }
        }
    }

    if let Some(storages) = prev.get("storages") {
        doc.insert("storages".to_string(), storages.clone());
    }

    toml::to_string_pretty(&doc).unwrap_or_else(|_| new_toml.to_string())
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

    // Serialize to TOML and create a memory-backed config manager. The
    // freshly converted config is merged with the ACME state left by the
    // previous run first, so already-issued certificates are served (and
    // not reordered) from the first request on.
    let toml_str = toml::to_string_pretty(&config).map_err(|e| {
        anyhow::anyhow!("failed to serialize proxy config: {}", e)
    })?;
    let acme_state = acme_state_path(&rule_cache);
    let toml_str = merge_config_state(
        &toml_str,
        &std::fs::read_to_string(&acme_state).unwrap_or_default(),
    );
    let config_manager =
        try_init_memory_config_manager(&toml_str, Some(acme_state)).map_err(
            |e| anyhow::anyhow!("failed to init config manager: {}", e),
        )?;

    // The lets-encrypt task scans current_config for certificates to issue,
    // so it must reflect the merged state; validation keeps a malformed
    // merge from taking the proxy down (falls back to the fresh config).
    let config = match PingapTomlConfig::from_toml(&toml_str)
        .and_then(|toml_config| toml_config.to_pingap_config(true))
    {
        Ok(merged) => merged,
        Err(e) => {
            warn!(
                error = %e,
                "data plane: merged config failed validation, using freshly converted config"
            );
            config
        },
    };
    config_manager.set_current_config(config.clone());

    // Initialize providers from the (merged) config
    try_init_upstreams(&config.upstreams, None)
        .map_err(|e| anyhow::anyhow!("failed to init upstreams: {}", e))?;
    try_init_locations(&config.locations)
        .map_err(|e| anyhow::anyhow!("failed to init locations: {}", e))?;
    try_init_server_locations(&config.servers, &config.locations).map_err(
        |e| anyhow::anyhow!("failed to init server locations: {}", e),
    )?;

    // Activate the WAF plugin: without it the data plane forwards traffic
    // uninspected and no access/security events are ever emitted.
    let (updated_plugins, plugin_errors) = try_init_plugins(&config.plugins);
    if !updated_plugins.is_empty() {
        info!(
            plugins = updated_plugins.join(","),
            "data plane: plugins initialized"
        );
    }
    if !plugin_errors.is_empty() {
        error!(error = plugin_errors, "data plane: plugin init errors");
    }

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

    // Let the agent report certificate state: the ACME task writes issued PEMs
    // into the provider below, and without this the control plane keeps showing
    // the certificate as pending forever. Certificates generated from site
    // config are named `{site_id}_cert`, which is how a status is attributed
    // back to its site.
    {
        let provider = cert_provider.clone();
        pingwaf_agent::cert_status::set_snapshot(Some(Arc::new(move || {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.as_secs() as i64)
                .unwrap_or_default();
            let mut statuses = HashMap::new();
            for cert in provider.list().values() {
                let Some(site_id) = cert
                    .name
                    .as_deref()
                    .and_then(|name| name.strip_suffix("_cert"))
                else {
                    continue;
                };
                let Some(info) = cert.info.as_ref() else {
                    continue;
                };
                if info.not_after == 0 {
                    continue;
                }
                let remaining = info.not_after - now;
                let status = if remaining <= 0 {
                    "expired"
                } else if remaining <= 2 * 24 * 3600 {
                    "expiring_soon"
                } else {
                    "valid"
                };
                // One certificate is listed under every domain it serves; the
                // entries agree, so the first one wins.
                statuses.entry(site_id.to_string()).or_insert(
                    pingwaf_agent::cert_status::CertStatus {
                        status,
                        expires_at: info.not_after,
                    },
                );
            }
            statuses
        })));
    }

    // Create the Pingora server
    let opt = Opt::default();
    let mut my_server = server::Server::new(Some(opt))?;
    let bootstrap_handle = my_server.bootstrap_as_a_service();

    // ACME (Let's Encrypt): issue and renew certificates for sites whose
    // SSL config carries no uploaded PEM. The task shares the config manager
    // with the proxy — the HTTP-01 challenge handler reads tokens from it,
    // and a renewed PEM lands in both the config and the certificate
    // provider below. PINGAP_DISABLE_ACME opts out entirely.
    let acme_enabled = !config.certificates.is_empty()
        && std::env::var("PINGAP_DISABLE_ACME")
            .unwrap_or_default()
            .is_empty();
    let mut background_tasks = BackgroundTaskService::new(
        "data_plane_background_tasks",
        Duration::from_secs(60),
        vec![],
    );
    background_tasks.set_immediately(true);
    background_tasks.set_initial_delay(Some(Duration::from_secs(3)));
    if acme_enabled {
        background_tasks.add_task(
            "lets_encrypt",
            new_lets_encrypt_service(
                config_manager.clone(),
                cert_provider.clone(),
                None,
            ),
        );
        info!("data plane: ACME certificate management enabled");
    }
    let background_tasks_name = background_tasks.name().to_string();
    my_server.add_service(background_service(
        &background_tasks_name,
        background_tasks,
    ));

    // Parse server configs and start proxy servers
    let server_conf_list: Vec<ServerConf> = parse_from_conf(config.clone());

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
        let mut ps = ProxyServer::new(&server_conf, ctx)?;
        // The HTTP-01 challenge must be reachable on port 80, so every
        // data-plane server whose address list includes :80 intercepts
        // /.well-known/acme-challenge before proxying.
        if acme_enabled
            && server_conf
                .addr
                .split(',')
                .any(|addr| addr.trim().ends_with(":80"))
        {
            ps.enable_lets_encrypt();
        }
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

    // Hot-reload watcher: poll the rule cache config hash and re-init the
    // ArcSwap providers when the control plane pushes new rules. The proxy
    // reads these providers per request, so no restart is needed. The config
    // manager itself is updated in place: on every reload the freshly
    // converted config is merged with the previous document (preserving
    // issued PEMs and ACME state such as challenge tokens and the account
    // key) and set as current, so the ACME renewal task keeps working.
    let watcher_cache = Arc::clone(&rule_cache);
    let watcher_config_manager = Arc::clone(&config_manager);
    let mut last_hash = watcher_cache.config_hash();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            let hash = watcher_cache.config_hash();
            if hash == last_hash {
                continue;
            }
            info!(
                old = %last_hash,
                new = %hash,
                "data plane: rule change detected, reloading"
            );
            let Some(converted) = build_pingap_config(&watcher_cache) else {
                warn!(
                    "data plane: reload produced no usable config, keeping current"
                );
                last_hash = hash;
                continue;
            };
            let Ok(new_toml) = toml::to_string_pretty(&converted) else {
                error!(
                    "data plane: reload config serialization failed, keeping current"
                );
                last_hash = hash;
                continue;
            };
            let prev_toml = watcher_config_manager
                .load_all_raw()
                .await
                .unwrap_or_default();
            let merged_toml = merge_config_state(&new_toml, &prev_toml);
            let config = match PingapTomlConfig::from_toml(&merged_toml)
                .and_then(|toml_config| toml_config.to_pingap_config(true))
            {
                Ok(config) => config,
                Err(e) => {
                    warn!(
                        error = %e,
                        "data plane: merged reload config failed to parse, using freshly converted config"
                    );
                    converted
                },
            };
            if let Ok(toml_config) = PingapTomlConfig::from_toml(&merged_toml)
                && let Err(e) =
                    watcher_config_manager.save_all(&toml_config).await
            {
                warn!(error = %e, "data plane: failed to persist merged config");
            }
            watcher_config_manager.set_current_config(config.clone());
            if let Err(e) = try_init_upstreams(&config.upstreams, None) {
                error!(error = %e, "data plane: reload upstreams failed");
            }
            if let Err(e) = try_init_locations(&config.locations) {
                error!(error = %e, "data plane: reload locations failed");
            }
            if let Err(e) =
                try_init_server_locations(&config.servers, &config.locations)
            {
                error!(error = %e, "data plane: reload server locations failed");
            }
            let (updated, errors) =
                try_update_certificates(&config.certificates);
            if !updated.is_empty() {
                info!(
                    certs = updated.join(","),
                    "data plane: certificates reloaded"
                );
            }
            if !errors.is_empty() {
                error!(error = errors, "data plane: certificate reload errors");
            }
            let (updated_plugins, plugin_errors) =
                try_init_plugins(&config.plugins);
            if !updated_plugins.is_empty() {
                info!(
                    plugins = updated_plugins.join(","),
                    "data plane: plugins reloaded"
                );
            }
            if !plugin_errors.is_empty() {
                error!(
                    error = plugin_errors,
                    "data plane: plugin reload errors"
                );
            }
            last_hash = hash;
        }
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

    // Siphons pingap's raw ACME events into the buffer the agent ships to the
    // control plane, so certificate-issuance logs show up on the admin panel.
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(true))
        .with(pingwaf_agent::cert_events::AcmeCaptureLayer)
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

    #[test]
    fn waf_plugin_is_injected_and_referenced_by_every_location() {
        let mut site = site_rules("site1", "a.example.com");
        let mut default_pool = pool("pool1", vec![peer("10.0.0.1:8080")]);
        default_pool.is_default = true;
        site.upstreams = vec![default_pool];
        site.routes = vec![route("r1", "prefix", "/api", "pool1")];
        let config = cached_rules_to_pingap_config(&one_site_cache(site))
            .expect("config should build");

        let category = config
            .plugins
            .get("pingwaf:waf")
            .and_then(|c| c.get("category"))
            .and_then(|v| v.as_str());
        assert_eq!(category, Some("waf"));

        assert!(config.locations.contains_key("site1_loc"));
        assert!(config.locations.contains_key("site1_route_r1"));
        for (name, loc) in &config.locations {
            assert_eq!(
                loc.plugins.as_deref(),
                Some(["pingwaf:waf".to_string()].as_slice()),
                "location {name} must reference the waf plugin"
            );
        }
    }

    #[test]
    fn certified_sites_split_plaintext_and_tls_servers() {
        let mut site = site_rules("site1", "a.example.com");
        let mut default_pool = pool("pool1", vec![peer("10.0.0.1:8080")]);
        default_pool.is_default = true;
        site.upstreams = vec![default_pool];
        site.ssl_config = Some(acme_ssl());
        let config = cached_rules_to_pingap_config(&one_site_cache(site))
            .expect("config should build");

        assert_eq!(config.servers.len(), 2);
        let http = config.servers.get("pingwaf").expect("http server");
        assert_eq!(http.addr, "0.0.0.0:80");
        assert_eq!(http.global_certificates, Some(false));
        let tls = config.servers.get("pingwaf_tls").expect("tls server");
        assert_eq!(tls.addr, "0.0.0.0:443");
        assert_eq!(tls.global_certificates, Some(true));
        let names = tls.locations.as_deref().expect("tls locations");
        assert_eq!(names, http.locations.as_deref().expect("http locations"));
        assert!(names.contains(&"site1_loc".to_string()));
    }

    #[test]
    fn sites_without_certificates_keep_one_combined_server() {
        let mut site = site_rules("site1", "a.example.com");
        let mut default_pool = pool("pool1", vec![peer("10.0.0.1:8080")]);
        default_pool.is_default = true;
        site.upstreams = vec![default_pool];
        let config = cached_rules_to_pingap_config(&one_site_cache(site))
            .expect("config should build");

        assert_eq!(config.servers.len(), 1);
        let server = config.servers.get("pingwaf").expect("server");
        assert_eq!(server.addr, "0.0.0.0:80,0.0.0.0:443");
        assert_eq!(server.global_certificates, None);
    }

    fn acme_ssl() -> SslConfig {
        SslConfig {
            cert_pem: String::new(),
            key_pem: String::new(),
            acme_enabled: true,
            acme_email: "admin@example.com".to_string(),
            acme_challenge_type: "AcmeHttp01".to_string(),
            acme_dns_provider: String::new(),
            acme_dns_config: HashMap::new(),
            min_tls_version: String::new(),
            hsts_enabled: false,
            hsts_max_age: 0,
            always_use_https: false,
            enabled: true,
            max_tls_version: String::new(),
            self_signed: false,
            mtls_enabled: false,
            mtls_client_ca: String::new(),
            certificate_id: String::new(),
        }
    }

    #[test]
    fn acme_http01_conf_marks_acme_without_dns_fields() {
        let ssl = acme_ssl();
        let cert = acme_certificate_conf(&ssl, "a.example.com".to_string());
        assert_eq!(cert.acme.as_deref(), Some("http://admin@example.com"));
        assert_eq!(cert.domains.as_deref(), Some("a.example.com"));
        assert_eq!(cert.dns_challenge, None);
        assert_eq!(cert.dns_provider, None);
    }

    #[test]
    fn acme_dns01_conf_builds_provider_endpoint_with_credentials() {
        let mut ssl = acme_ssl();
        ssl.acme_challenge_type = "AcmeDns01".to_string();
        // "aliyun" is an accepted alias; the canonical name the config
        // carries is "ali".
        ssl.acme_dns_provider = "aliyun".to_string();
        ssl.acme_dns_config
            .insert("access_key_id".to_string(), "AK 1/2".to_string());
        ssl.acme_dns_config
            .insert("access_key_secret".to_string(), "s3cret".to_string());

        let cert = acme_certificate_conf(&ssl, "a.example.com".to_string());
        assert_eq!(cert.dns_challenge, Some(true));
        assert_eq!(cert.dns_provider.as_deref(), Some("ali"));
        let url = cert.dns_service_url.expect("service url");
        assert!(url.starts_with("https://alidns.aliyuncs.com?"), "{url}");
        assert!(url.contains("access_key_id=AK%201%2F2"), "{url}");
        assert!(url.contains("access_key_secret=s3cret"), "{url}");
    }

    #[test]
    fn acme_dns01_unknown_provider_stays_on_http01() {
        let mut ssl = acme_ssl();
        ssl.acme_challenge_type = "AcmeDns01".to_string();
        ssl.acme_dns_provider = "not-a-provider".to_string();

        let cert = acme_certificate_conf(&ssl, "a.example.com".to_string());
        assert_eq!(cert.dns_challenge, None);
        assert_eq!(cert.dns_provider, None);
    }

    #[test]
    fn merge_config_state_empty_prev_is_identity() {
        let new_toml = "[certificates.c1]\ntls_key = \"k\"\n";
        assert_eq!(merge_config_state(new_toml, ""), new_toml);
    }

    #[test]
    fn merge_config_state_carries_issued_pems_and_storages() {
        let prev_toml = "\
[certificates.c1]
tls_cert = \"ISSUED\"
tls_key = \"KEY\"

[certificates.c2]
tls_cert = \"UPLOAD\"
tls_key = \"UPKEY\"

[storages.acme]
type = \"file\"
path = \"/tmp/acme\"
";
        let new_toml = "\
[certificates.c1]
acme = \"http://a@example.com\"

[certificates.c2]
tls_cert = \"NEWUPLOAD\"
tls_key = \"NEWKEY\"
";
        let merged = merge_config_state(new_toml, prev_toml)
            .parse::<toml::Table>()
            .expect("merged config must parse");

        let c1 = merged
            .get("certificates")
            .and_then(|v| v.get("c1"))
            .expect("c1 entry");
        assert_eq!(
            c1.get("tls_cert").and_then(|v| v.as_str()),
            Some("ISSUED"),
            "issued pem must be carried over"
        );
        assert_eq!(
            c1.get("tls_key").and_then(|v| v.as_str()),
            Some("KEY"),
            "issued key must be carried over"
        );

        let c2 = merged
            .get("certificates")
            .and_then(|v| v.get("c2"))
            .expect("c2 entry");
        assert_eq!(
            c2.get("tls_cert").and_then(|v| v.as_str()),
            Some("NEWUPLOAD"),
            "fresh upload must win over the previous pem"
        );

        let storages = merged.get("storages").expect("storages section");
        assert_eq!(
            storages
                .get("acme")
                .and_then(|v| v.get("path"))
                .and_then(|v| v.as_str()),
            Some("/tmp/acme")
        );
    }
}
