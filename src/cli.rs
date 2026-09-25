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

//! PingWAF CLI subcommand definitions.
//!
//! These are additive to the existing pingap CLI and are detected before
//! the normal argument parsing runs.

use clap::{Parser, Subcommand};

/// PingWAF top-level CLI wrapper.
///
/// When the first positional argument is one of `server`, `agent`, or
/// `all-in-one`, the binary switches to PingWAF mode.
#[derive(Parser, Debug)]
#[command(
    name = "pingwaf",
    about = "PingWAF — Web Application Firewall control plane and data plane",
    version
)]
pub struct PingWafCli {
    #[command(subcommand)]
    pub command: PingWafCommand,
}

#[derive(Subcommand, Debug)]
pub enum PingWafCommand {
    /// Run the control plane server only (REST API + gRPC + dashboard)
    Server(ServerOpts),
    /// Run the data plane agent only (connects to a remote control plane)
    Agent(AgentOpts),
    /// Run both control plane and data plane in a single process
    AllInOne(AllInOneOpts),
}

/// Options shared by all PingWAF modes.
#[derive(Parser, Debug, Clone)]
pub struct CommonOpts {
    /// PostgreSQL connection string
    #[arg(
        long,
        env = "PINGWAF_DB_URL",
        default_value = "postgres://pingwaf:pingwaf@localhost:5432/pingwaf"
    )]
    pub db_url: String,

    /// HTTP admin/API listen address
    #[arg(long, env = "PINGWAF_ADMIN_ADDR", default_value = "0.0.0.0:9080")]
    pub admin_addr: String,

    /// gRPC listen/connect address
    #[arg(long, env = "PINGWAF_GRPC_ADDR", default_value = "0.0.0.0:9090")]
    pub grpc_addr: String,
}

/// Control plane server options.
#[derive(Parser, Debug, Clone)]
pub struct ServerOpts {
    #[command(flatten)]
    pub common: CommonOpts,

    /// JWT signing secret (must be at least 16 characters)
    #[arg(
        long,
        env = "PINGWAF_JWT_SECRET",
        default_value = "change-me-in-production"
    )]
    pub jwt_secret: String,

    /// Default admin email for initial seeding
    #[arg(
        long,
        env = "PINGWAF_ADMIN_EMAIL",
        default_value = "admin@pingwaf.local"
    )]
    pub admin_email: String,

    /// Default admin password for initial seeding
    #[arg(long, env = "PINGWAF_ADMIN_PASSWORD", default_value = "pingwaf123")]
    pub admin_password: String,

    /// Whether to serve the embedded frontend (SPA)
    #[arg(long, default_value = "true")]
    pub serve_frontend: bool,
}

/// Data plane agent options.
#[derive(Parser, Debug, Clone)]
pub struct AgentOpts {
    /// Control plane gRPC URL to connect to
    #[arg(
        long,
        env = "PINGWAF_SERVER_URL",
        default_value = "http://localhost:9090"
    )]
    pub server_url: String,

    /// API key for agent authentication
    #[arg(long, env = "PINGWAF_API_KEY", default_value = "")]
    pub api_key: String,

    /// Local rule cache directory
    #[arg(long, env = "PINGWAF_CACHE_DIR", default_value = "./data/cache")]
    pub cache_dir: String,

    /// Allow traffic when disconnected from control plane
    #[arg(long, default_value = "true")]
    pub fail_open: bool,

    /// Heartbeat interval in seconds
    #[arg(long, default_value = "30")]
    pub heartbeat_interval_secs: u64,

    /// Maximum log batch size before flush
    #[arg(long, default_value = "100")]
    pub log_batch_size: usize,

    /// Log flush interval in seconds
    #[arg(long, default_value = "5")]
    pub log_flush_interval_secs: u64,

    /// Maximum request body size to log (bytes)
    #[arg(long, default_value = "8192")]
    pub max_body_log_size: usize,
}

/// All-in-one mode options (server + agent in one process).
#[derive(Parser, Debug, Clone)]
pub struct AllInOneOpts {
    // ── Common ────────────────────────────────────────────────────────
    /// PostgreSQL connection string
    #[arg(
        long,
        env = "PINGWAF_DB_URL",
        default_value = "postgres://pingwaf:pingwaf@localhost:5432/pingwaf"
    )]
    pub db_url: String,

    /// HTTP admin/API listen address
    #[arg(long, env = "PINGWAF_ADMIN_ADDR", default_value = "0.0.0.0:9080")]
    pub admin_addr: String,

    /// gRPC listen address
    #[arg(long, env = "PINGWAF_GRPC_ADDR", default_value = "0.0.0.0:9090")]
    pub grpc_addr: String,

    // ── Server ────────────────────────────────────────────────────────
    /// JWT signing secret (must be at least 16 characters)
    #[arg(
        long,
        env = "PINGWAF_JWT_SECRET",
        default_value = "change-me-in-production"
    )]
    pub jwt_secret: String,

    /// Default admin email for initial seeding
    #[arg(
        long,
        env = "PINGWAF_ADMIN_EMAIL",
        default_value = "admin@pingwaf.local"
    )]
    pub admin_email: String,

    /// Default admin password for initial seeding
    #[arg(long, env = "PINGWAF_ADMIN_PASSWORD", default_value = "pingwaf123")]
    pub admin_password: String,

    /// Whether to serve the embedded frontend (SPA)
    #[arg(long, default_value = "true")]
    pub serve_frontend: bool,

    // ── Agent ─────────────────────────────────────────────────────────
    /// API key for agent authentication (empty = auto-register via loopback)
    #[arg(long, env = "PINGWAF_API_KEY", default_value = "")]
    pub api_key: String,

    /// Local rule cache directory
    #[arg(long, env = "PINGWAF_CACHE_DIR", default_value = "./data/cache")]
    pub cache_dir: String,

    /// Allow traffic when disconnected from control plane
    #[arg(long, default_value = "true")]
    pub fail_open: bool,

    /// Heartbeat interval in seconds
    #[arg(long, default_value = "30")]
    pub heartbeat_interval_secs: u64,

    /// Maximum log batch size before flush
    #[arg(long, default_value = "100")]
    pub log_batch_size: usize,

    /// Log flush interval in seconds
    #[arg(long, default_value = "5")]
    pub log_flush_interval_secs: u64,

    /// Maximum request body size to log (bytes)
    #[arg(long, default_value = "8192")]
    pub max_body_log_size: usize,
}

/// Check whether the command line invokes a PingWAF subcommand.
///
/// Returns `true` if the first non-binary argument is one of the PingWAF
/// subcommand names, or if `PINGWAF_MODE` environment variable is set.
pub fn is_pingwaf_mode() -> bool {
    // Check env var first
    if let Ok(mode) = std::env::var("PINGWAF_MODE") {
        let mode = mode.trim().to_lowercase();
        if matches!(mode.as_str(), "server" | "agent" | "all-in-one") {
            return true;
        }
    }

    // Check argv
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        matches!(args[1].as_str(), "server" | "agent" | "all-in-one")
    } else {
        false
    }
}

/// Parse the PingWAF CLI from command line arguments.
///
/// If `PINGWAF_MODE` is set but no subcommand is given on the command line,
/// injects the mode as a subcommand so that env-only invocation works.
pub fn parse_pingwaf_cli() -> PingWafCli {
    let args: Vec<String> = std::env::args().collect();

    // If a subcommand is already present, parse directly
    if args.len() > 1
        && matches!(args[1].as_str(), "server" | "agent" | "all-in-one")
    {
        return PingWafCli::parse();
    }

    // Otherwise, inject the mode from the environment variable
    if let Ok(mode) = std::env::var("PINGWAF_MODE") {
        let mode = mode.trim().to_lowercase();
        if matches!(mode.as_str(), "server" | "agent" | "all-in-one") {
            let mut injected = args.clone();
            injected.insert(1, mode);
            return PingWafCli::parse_from(injected);
        }
    }

    // Shouldn't reach here if is_pingwaf_mode() was checked first
    eprintln!("error: no PingWAF subcommand or PINGWAF_MODE specified");
    std::process::exit(1);
}
