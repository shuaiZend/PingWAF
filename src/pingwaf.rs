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

use crate::cli::{
    AgentOpts, AllInOneOpts, PingWafCli, PingWafCommand, ServerOpts,
};
use pingwaf_agent::config::AgentConfig;
use pingwaf_server::ServerConfig;
use tracing::{error, info};

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
            info!("PingWAF agent is running, waiting for shutdown signal");

            // Wait for shutdown signal
            shutdown_signal().await;
            agent.shutdown().await;
            Ok(())
        },
        RunMode::AllInOne(server_config, agent_config) => {
            info!(
                http_addr = %server_config.http_addr,
                grpc_addr = %server_config.grpc_addr,
                "starting PingWAF in All-in-One mode (control plane + data plane)"
            );

            // Start the agent in the background first.
            // It will retry connecting to the server until it comes up.
            let agent = match pingwaf_agent::start_agent(agent_config).await {
                Ok(agent) => Some(agent),
                Err(e) => {
                    error!(error = %e, "failed to start agent, continuing with server only");
                    None
                },
            };

            // Run the server (blocks until shutdown signal)
            let result = pingwaf_server::start_server(server_config).await;

            // Gracefully shut down the agent
            if let Some(agent) = agent {
                agent.shutdown().await;
            }

            result
        },
    }
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
