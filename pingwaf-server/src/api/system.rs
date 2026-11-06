//! Read-only process observation: CPU/memory snapshots and the module/method
//! attribution tables.
//!
//! This is the successor of the removed pprof endpoints. Two differences are
//! deliberate:
//!
//! * Nothing here samples stacks or writes profile files. `/system/metrics`
//!   reads counters the OS already keeps; `/system/profile` reads the
//!   accounting tables the data plane and control plane maintain themselves
//!   (`pingwaf-profiler`).
//! * Both are ordinary admin endpoints, so they work on every platform and
//!   in every build — there is no diagnostic-only image to deploy.
//!
//! `/system/nodes` is the fleet view built from the same material: this
//! process's counters plus one row per registered agent. It is what the
//! dashboard's node table renders, and the only endpoint that answers "how
//! loaded is the fleet" in one request.

use std::collections::HashMap;

use axum::extract::State;
use axum::routing::get;
use axum::Json;
use axum::Router;
use chrono::{DateTime, Utc};
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect,
};
use serde::Serialize;
use uuid::Uuid;

use crate::api::common::query_all;
use crate::api::error::ApiError;
use crate::api::state::AppState;
use crate::auth::AdminUser;
use crate::grpc::registry::{AgentRegistry, HeartbeatMetrics};
use crate::models::{agent, api_key, control_plane_boot, site};

/// `GET /api/v1/system/metrics` — process/host CPU and memory snapshots.
#[derive(Debug, Serialize)]
pub struct SystemMetrics {
    /// Process and machine CPU usage.
    pub cpu: pingwaf_system::CpuLiveStats,
    /// Process RSS, host RAM and container memory accounting.
    pub memory: pingwaf_system::MemorySnapshot,
}

/// `GET /api/v1/system/status` — degraded-mode flags for the dashboard.
#[derive(Debug, Serialize)]
pub struct SystemStatus {
    /// False when the gRPC control plane serves plaintext.
    pub grpc_tls: bool,
}

async fn status(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> (
    [(axum::http::HeaderName, &'static str); 1],
    Json<SystemStatus>,
) {
    (
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        Json(SystemStatus {
            grpc_tls: state.config.grpc_tls_enabled(),
        }),
    )
}

async fn metrics(_admin: AdminUser) -> Result<Json<SystemMetrics>, ApiError> {
    // Both snapshots touch /proc (or the platform equivalent): keep that off
    // the async workers.
    let (cpu, memory) = tokio::task::spawn_blocking(|| {
        (
            pingwaf_system::cpu_snapshot(),
            pingwaf_system::memory_snapshot(),
        )
    })
    .await
    .map_err(|err| ApiError::Internal(err.to_string()))?;
    Ok(Json(SystemMetrics { cpu, memory }))
}

/// `GET /api/v1/system/profile` — module and method CPU/memory attribution.
///
/// The response is bounded (seven module rows, at most
/// [`pingwaf_profiler::ProfileSnapshot::MAX_METHODS`] method rows) and
/// computed from atomics, so polling it is cheap.
async fn profile(
    _admin: AdminUser,
    State(_state): State<AppState>,
) -> Json<pingwaf_profiler::ProfileSnapshot> {
    Json(pingwaf_profiler::ProfileSnapshot::collect())
}

/// `GET /api/v1/system/nodes` — this process plus every registered agent.
///
/// One request answers what the dashboard's node view needs: the control
/// plane's own CPU/memory/attribution, and per agent its identity, status,
/// last heartbeat and the edge counters retained from that heartbeat.
#[derive(Debug, Serialize)]
pub struct NodesView {
    /// The control-plane process serving the request.
    pub control: ControlNode,
    /// Registered agents, ordered by hostname so the table does not reshuffle
    /// between polls.
    pub agents: Vec<AgentNode>,
    /// The newest control-plane starts, newest first. One row per restart:
    /// the gap between two is the previous process's uptime, which is how a
    /// container killed in a loop (an OOM kill, a crash) is told from a
    /// planned redeploy.
    pub boots: Vec<ControlPlaneBoot>,
}

/// One recorded control-plane start.
#[derive(Debug, Serialize)]
pub struct ControlPlaneBoot {
    /// Server time of the start.
    pub booted_at: DateTime<Utc>,
    /// `fresh` (no previous boot), `after_stop` (the previous process lived
    /// long enough for a planned stop) or `unclean` (it did not).
    pub reason: String,
    /// The version that came up.
    pub version: String,
    /// The machine it came up on.
    pub hostname: String,
    /// Seconds the previous process lived, when that could be told. `None`
    /// for the first boot and when the clock moved backwards between starts.
    pub previous_uptime_seconds: Option<i64>,
}

fn boot_view(row: &control_plane_boot::Model) -> ControlPlaneBoot {
    ControlPlaneBoot {
        booted_at: row.booted_at,
        reason: row.reason.clone(),
        version: row.version.clone(),
        hostname: row.hostname.clone(),
        previous_uptime_seconds: row.previous_uptime_seconds,
    }
}

/// Resource counters of the control-plane process itself.
#[derive(Debug, Serialize)]
pub struct ControlNode {
    /// CPU used by this process, as a share of the whole machine in percent
    /// (0-100), the same scale as [`Self::host_cpu_percent`].
    pub process_cpu_percent: f32,
    /// Machine-wide CPU usage, in percent.
    pub host_cpu_percent: f32,
    /// Cores on the machine.
    pub cores: usize,
    /// Process RSS in bytes.
    pub rss_bytes: u64,
    /// Peak process RSS since start, in bytes (`0` where the OS does not
    /// expose it).
    pub rss_peak_bytes: u64,
    /// Virtual address space of this process in bytes (`/proc/self/status`
    /// `VmSize`); `0` where the OS does not expose it. The number that stays
    /// large while `rss_bytes` looks harmless.
    pub vm_size_bytes: u64,
    /// System-wide committed address space in bytes (`/proc/meminfo`
    /// `Committed_AS`) — what the kernel has promised, not what is resident.
    pub committed_as_bytes: u64,
    /// The commit ceiling in bytes (`/proc/meminfo` `CommitLimit`:
    /// `RAM * overcommit_ratio + swap`).
    pub commit_limit_bytes: u64,
    /// `committed_as_bytes / commit_limit_bytes * 100`, one decimal; `0.0`
    /// when there is no limit. Above 100 is reported as measured — see
    /// `pingwaf_system::MemorySnapshot::commit_percent`.
    pub commit_percent: f64,
    /// Requests the profiler attributed to the `control` module.
    pub requests: u64,
    /// Mean wall-clock time per attributed control request, in milliseconds.
    pub avg_elapsed_ms: f64,
}

/// One agent as the node view shows it.
///
/// Every field the control plane may not know yet is `null` rather than `0`:
/// a node that has never heartbeated reports no CPU, and rendering that as
/// "0 %" would read as an idle node instead of an unknown one.
#[derive(Debug, Serialize)]
pub struct AgentNode {
    pub id: Uuid,
    /// Display name. The inventory keeps one identity per node — its
    /// `hostname` — so that is what this carries.
    pub name: String,
    /// `online` / `offline` / `degraded`, as last reported.
    pub status: String,
    /// CPU load of the node, in percent. `None` = no sample retained.
    pub cpu_percent: Option<f64>,
    /// Where [`Self::cpu_percent`] came from: `probe`, `heartbeat` or `none`.
    ///
    /// The two sources are not the same measurement, so the figure is not
    /// self-explanatory. A probe sample is a 5-second interval delta; the
    /// heartbeat value only became one in 0.28.3 — before that it is an average
    /// since boot, which a long-running edge dilutes towards zero and which a
    /// client cannot tell apart from a genuinely idle node. Shipping the label
    /// lets a consumer say which figure it is showing, and makes an agent that
    /// has not been upgraded visible instead of silently mixed into the fleet.
    pub cpu_source: String,
    /// Memory in use on the node, in bytes. `None` = no sample retained.
    ///
    /// Kept under its original name for the console; it carries the same value
    /// as `used_memory_bytes`.
    pub memory_bytes: Option<u64>,
    /// Memory in use on the node, in bytes — the unambiguous name. `None` =
    /// no sample retained.
    pub used_memory_bytes: Option<u64>,
    /// RAM the node has, in bytes. The probe's newest `MemTotal` when a sample
    /// is retained, else the capacity recorded at registration. `None` = an
    /// agent that has neither re-registered since migration 000064 nor shipped
    /// a probe sample.
    pub total_memory_bytes: Option<u64>,
    /// Connections currently handled by the edge process. `None` = never
    /// reported.
    pub connections: Option<u64>,
    /// Requests per second handled by the edge. `None` = the agent is not
    /// connected, so nothing retained this counter: unlike CPU and memory it is
    /// stored nowhere else.
    pub rps: Option<u64>,
    /// Sites this agent serves: `1` for a bound agent, and for a global
    /// (unbound) agent every site of its API key's owner — the same
    /// visibility the config push uses.
    pub sites: u64,
    /// Last heartbeat, in server time. `None` = never heard from.
    pub last_seen: Option<DateTime<Utc>>,
}

/// Counter values the control plane has for one agent.
///
/// This is the single place that decides how an observed value becomes an
/// [`AgentNode`] field, so the mapping is testable without a database.
///
/// Two sources feed it:
///
/// 1. the counters of the last heartbeat, retained in [`AgentRegistry`] while
///    the stream is open — at most one heartbeat interval old, and the only
///    source of `rps` (the heartbeat message itself is consumed and dropped);
/// 2. the newest stored samples — `host_samples` for CPU/memory, the
///    `pingwaf_active_connections` gauge in `agent_metrics` for the connection
///    count — which is all an offline agent has.
///
/// The live report is preferred field by field with one deliberate exception:
/// CPU is decided by [`pick_cpu`], which takes the probe sample whenever there
/// is one, because only that figure is an interval delta on every agent version.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeCounters {
    pub cpu_percent: Option<f64>,
    /// Which source [`Self::cpu_percent`] was taken from — see [`pick_cpu`].
    /// [`CPU_SOURCE_NONE`] whenever the value itself is `None`, so the pair
    /// never claims a source for a figure that is not there.
    pub cpu_source: &'static str,
    /// Memory in use on the node, in bytes.
    pub used_memory_bytes: Option<u64>,
    /// RAM the node has, in bytes, when the retained probe sample carries it.
    pub total_memory_bytes: Option<u64>,
    pub connections: Option<u64>,
    pub rps: Option<u64>,
}

impl Default for NodeCounters {
    fn default() -> Self {
        // Hand-written rather than derived so the CPU label starts at `none`
        // instead of the empty string: an agent with no sample at all must
        // report `cpu_source: "none"`, the same word the field documents for
        // "neither source carried a reading".
        Self {
            cpu_percent: None,
            cpu_source: CPU_SOURCE_NONE,
            used_memory_bytes: None,
            total_memory_bytes: None,
            connections: None,
            rps: None,
        }
    }
}

async fn nodes(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<NodesView>, ApiError> {
    // The snapshots read /proc; keep that off the async workers (same reason
    // as `/system/metrics`).
    let (cpu, memory) = tokio::task::spawn_blocking(|| {
        (
            pingwaf_system::cpu_snapshot(),
            pingwaf_system::memory_snapshot(),
        )
    })
    .await
    .map_err(|err| ApiError::Internal(err.to_string()))?;
    let profile = pingwaf_profiler::ProfileSnapshot::collect();
    // A failed read must not take the whole node view down: the control card
    // and the agent table are still worth showing, so the history degrades to
    // empty and the reason is logged.
    let boots = match crate::boot_history::recent_boots(
        &state.db,
        crate::boot_history::BOOTS_SHOWN,
    )
    .await
    {
        Ok(rows) => rows.iter().map(boot_view).collect(),
        Err(err) => {
            tracing::warn!(error = %err, "could not read the control plane boot history");
            Vec::new()
        },
    };
    Ok(Json(NodesView {
        control: control_node(&cpu, &memory, &profile),
        agents: agent_nodes(&state.db, &state.agents).await?,
        boots,
    }))
}

/// Projects the OS snapshots and the profiler's `control` row.
fn control_node(
    cpu: &pingwaf_system::CpuLiveStats,
    memory: &pingwaf_system::MemorySnapshot,
    profile: &pingwaf_profiler::ProfileSnapshot,
) -> ControlNode {
    // `ProfileSnapshot::collect` emits one row per module (`Module::ALL`), so
    // the `control` row exists even before the first request; the fallback
    // keeps the handler total instead of panicking if that ever changes.
    let control = profile.modules.iter().find(|row| row.module == "control");
    let (requests, avg_us) = control
        .map(|row| (row.calls, row.avg_us))
        .unwrap_or((0, 0.0));
    ControlNode {
        process_cpu_percent: cpu.process_cpu_percent,
        host_cpu_percent: cpu.host_cpu_percent,
        cores: cpu.cores,
        rss_bytes: memory.rss_bytes,
        rss_peak_bytes: memory.rss_peak_bytes,
        vm_size_bytes: memory.vm_size_bytes,
        committed_as_bytes: memory.committed_as_bytes,
        commit_limit_bytes: memory.commit_limit_bytes,
        commit_percent: memory.commit_percent,
        requests,
        avg_elapsed_ms: avg_us / 1000.0,
    }
}

/// Loads the fleet and joins the retained heartbeat counters onto each row.
async fn agent_nodes(
    db: &DatabaseConnection,
    registry: &AgentRegistry,
) -> Result<Vec<AgentNode>, ApiError> {
    let rows = agent::Entity::find()
        .order_by_asc(agent::Column::Hostname)
        .all(db)
        .await?;
    if rows.is_empty() {
        // No query below is meaningful without ids, and an empty fleet must
        // come back as `[]`, not as a list of ghost rows.
        return Ok(Vec::new());
    }

    let ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();

    let mut counters = latest_samples(db, &ids).await?;
    for (id, connections) in latest_connections(db, &ids).await? {
        counters.entry(id).or_default().connections = Some(connections);
    }
    // Live heartbeat counters: fresher than anything stored, and the only
    // place RPS survives. Cloned once per poll — one flat entry per connected
    // agent, not per stored sample.
    let live = registry.metrics().await;
    let sites = agent_site_counts(db, &rows).await?;

    let unknown = NodeCounters::default();
    Ok(rows
        .iter()
        .map(|row| {
            let stored = counters.get(&row.id).unwrap_or(&unknown);
            let merged = merge_counters(stored, live.get(&row.id).copied());
            agent_node(row, &merged, sites.get(&row.id).copied().unwrap_or(0))
        })
        .collect())
}

/// `cpu_source` when the reported figure is a probe sample.
const CPU_SOURCE_PROBE: &str = "probe";
/// `cpu_source` when the reported figure is the live heartbeat's.
const CPU_SOURCE_HEARTBEAT: &str = "heartbeat";
/// `cpu_source` when neither source carried a reading.
const CPU_SOURCE_NONE: &str = "none";

/// Chooses the CPU figure the node view reports, and names its source.
///
/// Probe samples are interval deltas and therefore comparable across agent
/// versions; the heartbeat value is a since-boot average on agents that
/// predate 0.28.3. Prefer the probe, fall back to the heartbeat.
///
/// "Comparable" is the whole point. `host_samples.cpu_usage_percent` is a
/// 5-second delta whichever agent shipped it, so it means the same thing on
/// every node of the fleet; the heartbeat became one only in 0.28.3, and an
/// older agent still reports `total_busy / total_since_boot`, which a
/// long-running edge dilutes towards zero. Mixing the two into one column is
/// what made an un-upgraded node look idle, so the fresh-looking-but-diluted
/// value is no longer allowed to win: the probe is the answer whenever it
/// exists, and the heartbeat only covers a node that has not been sampled yet
/// (fresh registration, or a deployment that ships no probe samples).
///
/// A non-finite reading is not a reading: `NaN`/`±inf` are dropped here, where
/// the choice is made, so the returned label can never name a source for a
/// value the caller then has to discard.
fn pick_cpu(
    probe: Option<f64>,
    heartbeat: Option<f64>,
) -> (Option<f64>, &'static str) {
    let usable = |value: Option<f64>| value.filter(|value| value.is_finite());
    if let Some(value) = usable(probe) {
        return (Some(value), CPU_SOURCE_PROBE);
    }
    if let Some(value) = usable(heartbeat) {
        return (Some(value), CPU_SOURCE_HEARTBEAT);
    }
    (None, CPU_SOURCE_NONE)
}

/// Precedence between the two sources of [`NodeCounters`].
///
/// A live value wins field by field, with CPU as the one exception. The live
/// report is at most one heartbeat old, where the stored sample can be a minute
/// old (probe samples are persisted once a minute) or days old for an offline
/// agent. The stored sample still answers for what a live report cannot express:
///
/// * CPU is decided by [`pick_cpu`], which prefers the stored probe sample: it
///   is the delta-comparable figure, the heartbeat one is a since-boot average
///   on agents older than 0.28.3. Memory, connections and RPS keep the live
///   value, as before.
/// * Memory is a plain number on the wire, so `0` cannot be told apart from
///   "the agent could not read it". A zero memory reading therefore falls back
///   to the stored sample — showing `0` bytes would render as a node with no
///   memory at all — while a zero connection/RPS count is kept, an idle edge
///   being a real state.
/// * `total_memory_bytes` only ever comes from a probe sample or the
///   registration row, never from the heartbeat.
fn merge_counters(
    stored: &NodeCounters,
    live: Option<HeartbeatMetrics>,
) -> NodeCounters {
    let Some(live) = live else {
        return stored.clone();
    };
    let (cpu_percent, cpu_source) =
        pick_cpu(stored.cpu_percent, Some(live.cpu_percent));
    NodeCounters {
        cpu_percent,
        cpu_source,
        used_memory_bytes: (live.memory_bytes > 0)
            .then_some(live.memory_bytes)
            .or(stored.used_memory_bytes),
        total_memory_bytes: stored.total_memory_bytes,
        connections: Some(live.connections),
        rps: Some(live.rps),
    }
}

/// Maps one inventory row plus its retained counters onto the response.
fn agent_node(
    row: &agent::Model,
    counters: &NodeCounters,
    sites: u64,
) -> AgentNode {
    // A NaN would serialise as `null` anyway; dropping it here keeps the
    // struct free of a value nothing can compare. The label follows the value:
    // a counter set assembled by hand with a non-finite reading reports `none`
    // rather than naming a source for a figure that is not on the wire.
    let cpu_percent = counters.cpu_percent.filter(|value| value.is_finite());
    let cpu_source = if cpu_percent.is_some() {
        counters.cpu_source
    } else {
        CPU_SOURCE_NONE
    };
    AgentNode {
        id: row.id,
        // Same label as `GET /agents`: the operator's display name when one
        // is set, the reported hostname otherwise.
        name: row
            .display_name
            .clone()
            .unwrap_or_else(|| row.hostname.clone()),
        status: row.status.clone(),
        cpu_percent,
        cpu_source: cpu_source.to_string(),
        memory_bytes: counters.used_memory_bytes,
        used_memory_bytes: counters.used_memory_bytes,
        // The probe's newest reading is the same `MemTotal` registration
        // reports, and it is fresher; the row column covers an agent that has
        // re-registered but has not been sampled yet.
        total_memory_bytes: counters.total_memory_bytes.or(row
            .total_memory_bytes
            .and_then(|bytes| u64::try_from(bytes).ok())),
        connections: counters.connections,
        rps: counters.rps,
        sites,
        last_seen: row.last_heartbeat,
    }
}

/// Newest host probe sample per agent, as one ordered index scan.
///
/// `DISTINCT ON` is the cheapest "newest row per agent" on PostgreSQL and rides
/// `idx_host_samples_agent_time (agent_id, sampled_at DESC)`: `agent_id =
/// ANY($1)` is the index condition on the leading column and the index order is
/// exactly `(agent_id, sampled_at DESC)`.
///
/// The trailing `id DESC` only breaks ties, but it is not optional. The agent
/// buffers its 5-second probes and ships them as one batch, so several rows of
/// one `agent_id` can carry the same `sampled_at`; without a tie-breaker
/// `DISTINCT ON` returns whichever of them the scan reached first, which means
/// a different CPU/memory reading on a different plan or after a `VACUUM`.
/// `id` is the identity column, so it totally orders the tie group. The index
/// still supplies `(agent_id, sampled_at DESC)`, and the extra key is resolved
/// by an incremental sort inside each group — never over the table.
const LATEST_SAMPLES_SQL: &str = "SELECT DISTINCT ON (agent_id) agent_id, \
     cpu_usage_percent, memory_used_bytes, memory_total_bytes \
     FROM host_samples \
     WHERE agent_id = ANY($1) \
     ORDER BY agent_id, sampled_at DESC, id DESC";

/// The probe is what the edge samples for `cpu_usage_percent` and
/// `memory_usage_bytes`: the heartbeat's own copies of those two are only kept
/// per stream, but the raw samples are persisted, so the newest one is the same
/// measurement at most one heartbeat interval older. A node that has not been
/// sampled yet stays `None`.
///
/// The same row carries the node's `MemTotal`, which is the capacity the node
/// view reports — the inventory column only exists since migration 000064, and
/// the sample is fresher than it in any case.
async fn latest_samples(
    db: &DatabaseConnection,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, NodeCounters>, ApiError> {
    let mut out = HashMap::with_capacity(ids.len());
    for row in latest_rows(db, LATEST_SAMPLES_SQL, ids, Vec::new()).await? {
        let agent_id = required::<Uuid>(&row, "agent_id")?;
        // The stored sample *is* the probe, so it is labelled and sanity-checked
        // by the same function that later decides between it and the heartbeat:
        // one place owns both the precedence and the `probe`/`heartbeat`/`none`
        // vocabulary, and a non-finite stored reading never becomes a value.
        let (cpu_percent, cpu_source) =
            pick_cpu(optional::<f64>(&row, "cpu_usage_percent")?, None);
        // The columns are signed bigints; a negative count is a corrupt
        // sample, not a byte total, so it reads as unknown.
        let used_memory_bytes = optional::<i64>(&row, "memory_used_bytes")?
            .and_then(|bytes| u64::try_from(bytes).ok());
        let total_memory_bytes = optional::<i64>(&row, "memory_total_bytes")?
            .and_then(|bytes| u64::try_from(bytes).ok());
        out.insert(
            agent_id,
            NodeCounters {
                cpu_percent,
                cpu_source,
                used_memory_bytes,
                total_memory_bytes,
                ..Default::default()
            },
        );
    }
    Ok(out)
}

/// Gauge the agent publishes as `active_connections`. Bound as `$2` rather than
/// written into the statement: the query text then depends only on the shape,
/// so the planner sees one prepared statement for every poll.
const ACTIVE_CONNECTIONS_METRIC: &str = "pingwaf_active_connections";

/// Newest `pingwaf_active_connections` gauge per agent, as one ordered index
/// scan on `idx_agent_metrics_agent_name_time_desc` (migration
/// `m20240101_000061`).
///
/// The agent publishes this counter on the metrics stream, and it is built from
/// the very value its heartbeat carries as `active_connections`. It is the only
/// place that number survives: the heartbeat itself is not stored. A deployment
/// that disables metric shipping leaves it `None`, which is why the field is
/// nullable.
///
/// # Why this exact text
///
/// `agent_metrics` is high-write (batched metric points) and only swept after
/// `metric_retention_days`, and `/system/nodes` runs this once per poll, so the
/// statement has to be ordered by the index instead of sorting the retained
/// rows every 15s.
///
/// * `agent_id = ANY($1) AND name = $2` are both equality restrictions and they
///   lead the index, `(agent_id, name, recorded_at DESC)`. Writing them in
///   index order is for the reader — the planner reorders predicates — but the
///   index *column* order is what matters: with both as index conditions,
///   `name` is a pseudoconstant and the index suffix is exactly the requested
///   sort, `(agent_id, recorded_at DESC)`.
/// * `recorded_at DESC` is the part the old `idx_agent_metrics_agent_name_time
///   (agent_id, name, recorded_at)` could not deliver. It was already declared
///   with the equality columns first; what it lacked was the direction. A
///   b-tree yields a column pair all-ASC (forward) or all-DESC (backward), and
///   `agent_id ASC, recorded_at DESC` is mixed, so neither scan direction
///   matched and the planner had to `Sort` every matching row.
/// * `id DESC` (M6) breaks the tie when one batch wrote two rows with the same
///   `recorded_at`; without it `DISTINCT ON` picks an arbitrary row of the
///   group. The index does not carry `id`, so the planner resolves it with an
///   incremental sort over the same index — inside each `(agent_id,
///   recorded_at)` group, one row per agent, not over the table:
///
/// ```text
/// Unique                          <- DISTINCT ON (agent_id)
///   ->  Sort (id DESC)            <- over |agents| rows
///         ->  Index Scan using idx_agent_metrics_agent_name_time_desc
///               Index Cond: (agent_id = ANY($1) AND name = $2)
/// ```
const LATEST_CONNECTIONS_SQL: &str =
    "SELECT DISTINCT ON (agent_id) agent_id, value \
     FROM agent_metrics \
     WHERE agent_id = ANY($1) AND name = $2 \
     ORDER BY agent_id, recorded_at DESC, id DESC";

async fn latest_connections(
    db: &DatabaseConnection,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, u64>, ApiError> {
    let mut out = HashMap::with_capacity(ids.len());
    let metric =
        vec![sea_orm::Value::from(ACTIVE_CONNECTIONS_METRIC.to_owned())];
    for row in latest_rows(db, LATEST_CONNECTIONS_SQL, ids, metric).await? {
        let agent_id = required::<Uuid>(&row, "agent_id")?;
        let value = required::<f64>(&row, "value")?;
        if let Some(connections) = gauge_to_u64(value) {
            out.insert(agent_id, connections);
        }
    }
    Ok(out)
}

/// Runs one of the two "newest row per agent" queries with the fleet bound as
/// a `uuid[]` (`$1`), so the whole list costs one round trip.
///
/// `extra` carries the remaining parameters in order (`$2`, `$3`, …); only the
/// metric query has one today.
async fn latest_rows(
    db: &DatabaseConnection,
    sql: &str,
    ids: &[Uuid],
    extra: Vec<sea_orm::Value>,
) -> Result<Vec<sea_orm::QueryResult>, ApiError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut values = Vec::with_capacity(extra.len() + 1);
    values.push(sea_orm::Value::from(ids.to_vec()));
    values.extend(extra);
    query_all(db, sql, values).await
}

/// Rounds a gauge to a whole count, refusing values no count can hold.
///
/// A brief underflow in the edge's connection counter is reported as a
/// negative gauge; rendering that as a wrapped `u64` would show ~1.8e19
/// connections.
fn gauge_to_u64(value: f64) -> Option<u64> {
    (value.is_finite() && value >= 0.0).then(|| value.round() as u64)
}

/// How many sites each agent serves.
///
/// A bound agent serves exactly its own site. An unbound ("global") agent
/// serves every site of its API key's owner — `agent_visible_site_ids` in the
/// config push applies the same rule, so the number here is what that agent
/// actually receives, not a fleet-wide total.
///
/// Only the owners that matter are loaded, and only as a tally: the per-owner
/// figure comes from [`site_count_query`], not from counting rows in memory.
async fn agent_site_counts(
    db: &DatabaseConnection,
    rows: &[agent::Model],
) -> Result<HashMap<Uuid, u64>, ApiError> {
    // The common single-tenant fleet has no unbound agent at all: answer from
    // the binding and skip both queries.
    if rows.iter().all(|row| row.site_id.is_some()) {
        return Ok(rows.iter().map(|row| (row.id, 1)).collect());
    }

    // Aggregated by the database: one row per *owner*, never one row per site.
    // `/system/nodes` is polled every 15s, so shipping every `sites.user_id`
    // just to count them here was a full column scan per poll.
    let per_owner: HashMap<Uuid, u64> = site_count_query()
        .into_tuple::<(Uuid, i64)>()
        .all(db)
        .await?
        .into_iter()
        .filter_map(|(owner, count)| {
            // COUNT(*) is non-negative; a negative reading means a corrupt row
            // set, so it counts as "no sites" instead of wrapping to ~1.8e19.
            u64::try_from(count).ok().map(|count| (owner, count))
        })
        .collect();

    let key_ids: Vec<Uuid> = rows
        .iter()
        .filter(|row| row.site_id.is_none())
        .filter_map(|row| row.api_key_id)
        .collect();
    let owner_of_key: HashMap<Uuid, Uuid> = if key_ids.is_empty() {
        HashMap::new()
    } else {
        api_key::Entity::find()
            .filter(api_key::Column::Id.is_in(key_ids))
            .select_only()
            .column(api_key::Column::Id)
            .column(api_key::Column::UserId)
            .into_tuple()
            .all(db)
            .await?
            .into_iter()
            .collect()
    };

    Ok(rows
        .iter()
        .map(|row| {
            let count = match row.site_id {
                Some(_) => 1,
                // An unbound agent whose key is gone owns nothing: the row is
                // still listed, its site count is honestly zero.
                None => row
                    .api_key_id
                    .and_then(|key_id| owner_of_key.get(&key_id).copied())
                    .and_then(|owner| per_owner.get(&owner).copied())
                    .unwrap_or(0),
            };
            (row.id, count)
        })
        .collect())
}

/// `SELECT user_id, COUNT(*) FROM sites GROUP BY user_id`.
///
/// Extracted so the statement can be asserted without a database: the
/// regression this guards against (projecting every `sites.user_id` and
/// tallying in memory) is visible in the SQL, and it is the difference between
/// one row per owner and one row per site on every poll of `/system/nodes`.
fn site_count_query() -> sea_orm::Select<site::Entity> {
    site::Entity::find()
        .select_only()
        .column(site::Column::UserId)
        .column_as(site::Column::Id.count(), "site_count")
        .group_by(site::Column::UserId)
}

/// Reads a column the query always produces.
fn required<T>(row: &sea_orm::QueryResult, column: &str) -> Result<T, ApiError>
where
    T: sea_orm::TryGetable,
{
    row.try_get::<T>("", column)
        .map_err(ApiError::internal_generic)
}

/// Reads a nullable column.
fn optional<T>(
    row: &sea_orm::QueryResult,
    column: &str,
) -> Result<Option<T>, ApiError>
where
    T: sea_orm::TryGetable,
{
    row.try_get::<Option<T>>("", column)
        .map_err(ApiError::internal_generic)
}

/// Routes contributed to `/api/v1`.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/system/metrics", get(metrics))
        .route("/system/profile", get(profile))
        .route("/system/nodes", get(nodes))
        .route("/system/status", get(status))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The snapshot is a plain JSON value with the documented shape: module
    /// rows keyed by name and a bounded method list.
    #[test]
    fn profile_snapshot_shape() {
        let snapshot = pingwaf_profiler::ProfileSnapshot::collect();
        assert_eq!(
            snapshot.modules.len(),
            pingwaf_profiler::Module::ALL.len(),
            "one row per module, even before instrumentation runs"
        );
        assert!(
            snapshot.modules.iter().any(|row| row.module == "proxy"),
            "every module is present even before instrumentation runs"
        );
        assert!(
            snapshot.modules.iter().any(|row| row.module == "waf_body"),
            "the WAF stage split is part of the table"
        );
        assert!(
            snapshot.methods.len()
                <= pingwaf_profiler::ProfileSnapshot::MAX_METHODS
        );
        assert!(snapshot.collected_at_ms > 0);
    }

    /// `/system/metrics` hands `MemorySnapshot` to serde as-is, so the commit
    /// accounting the console reads *is* that struct field set. Pinned here as
    /// well as in `pingwaf-system`, because a field renamed there has to fail
    /// an API test rather than silently empty a panel on the dashboard.
    #[test]
    fn metrics_memory_carries_the_commit_accounting() {
        let json = serde_json::to_value(SystemMetrics {
            cpu: pingwaf_system::cpu_snapshot(),
            memory: pingwaf_system::memory_snapshot(),
        })
        .expect("serialises");
        let memory = json
            .get("memory")
            .and_then(serde_json::Value::as_object)
            .expect("memory is an object on the wire");

        for field in
            ["vm_size_bytes", "committed_as_bytes", "commit_limit_bytes"]
        {
            assert!(
                memory.get(field).is_some_and(serde_json::Value::is_u64),
                "memory.{field} must be an unsigned byte count"
            );
        }
        assert!(
            memory
                .get("commit_percent")
                .is_some_and(serde_json::Value::is_number),
            "memory.commit_percent must be a number serde_json can carry"
        );
        for field in ["vm_size", "committed_as", "commit_limit"] {
            assert!(
                memory.get(field).is_some_and(serde_json::Value::is_string),
                "memory.{field} must be a human-readable size"
            );
        }
    }

    fn inventory_row(
        hostname: &str,
        status: &str,
        last_heartbeat: Option<DateTime<Utc>>,
    ) -> agent::Model {
        agent::Model {
            id: Uuid::new_v4(),
            site_id: None,
            hostname: hostname.to_string(),
            display_name: None,
            ip_address: "10.0.0.5".to_string(),
            public_ip: None,
            private_ip: None,
            version: Some("0.28.1".to_string()),
            os_info: Some("linux x86_64".to_string()),
            cpu_cores: Some(4),
            // A row from before migration 000064: the single legacy figure,
            // no split columns.
            memory_bytes: Some(8 << 30),
            total_memory_bytes: None,
            used_memory_bytes: None,
            status: status.to_string(),
            api_key_id: None,
            config_hash: None,
            last_heartbeat,
            last_config_sync_at: None,
            last_policy_sync_at: None,
            registered_at: Utc::now(),
        }
    }

    /// The endpoint the dashboard polls must answer an empty fleet with an
    /// empty list, not an error or a placeholder row.
    #[test]
    fn nodes_view_without_agents_serialises_an_empty_list() {
        let view = NodesView {
            control: control_node(
                &pingwaf_system::cpu_snapshot(),
                &pingwaf_system::memory_snapshot(),
                &pingwaf_profiler::ProfileSnapshot::collect(),
            ),
            agents: Vec::new(),
            boots: Vec::new(),
        };
        let json = serde_json::to_value(&view).expect("serialises");
        assert_eq!(json["agents"], serde_json::json!([]));
        assert!(json["control"].is_object());
        assert_eq!(json["boots"], serde_json::json!([]));
    }

    /// The boot history is a list the console renders newest-first, and its
    /// fields keep their names on the wire.
    #[test]
    fn boot_view_carries_the_recorded_fields() {
        let booted_at = Utc::now();
        let row = control_plane_boot::Model {
            id: Uuid::new_v4(),
            booted_at,
            reason: "unclean".to_string(),
            version: "0.29.3".to_string(),
            hostname: "waf-1".to_string(),
            previous_uptime_seconds: Some(12),
            created_at: booted_at,
        };
        let json = serde_json::to_value(boot_view(&row)).expect("serialises");
        assert_eq!(json["reason"], "unclean");
        assert_eq!(json["version"], "0.29.3");
        assert_eq!(json["hostname"], "waf-1");
        assert_eq!(json["previous_uptime_seconds"], 12);
        assert!(json["booted_at"].is_string());
    }

    /// A first boot has no uptime to report: the field is null, not 0, so the
    /// console cannot render "lived 0 seconds" for a process that never had a
    /// predecessor.
    #[test]
    fn a_first_boot_reports_no_previous_uptime() {
        let row = control_plane_boot::Model {
            id: Uuid::new_v4(),
            booted_at: Utc::now(),
            reason: "fresh".to_string(),
            version: "0.29.3".to_string(),
            hostname: "waf-1".to_string(),
            previous_uptime_seconds: None,
            created_at: Utc::now(),
        };
        let json = serde_json::to_value(boot_view(&row)).expect("serialises");
        assert_eq!(json["previous_uptime_seconds"], serde_json::Value::Null);
    }

    /// The control block is a real reading, never a negative one, and its
    /// request figures come from the profiler's `control` row.
    #[test]
    fn control_block_reflects_the_profiler_and_the_process() {
        let snapshot = pingwaf_profiler::ProfileSnapshot::collect();
        let control = control_node(
            &pingwaf_system::cpu_snapshot(),
            &pingwaf_system::memory_snapshot(),
            &snapshot,
        );
        let row = snapshot
            .modules
            .iter()
            .find(|row| row.module == "control")
            .expect("the profiler always carries a control row");

        assert!(control.process_cpu_percent >= 0.0);
        assert!(control.host_cpu_percent >= 0.0);
        assert!(control.cores > 0);
        assert_eq!(control.requests, row.calls);
        assert!((control.avg_elapsed_ms - row.avg_us / 1000.0).abs() < 1e-9);
        assert!(control.avg_elapsed_ms >= 0.0);
    }

    /// A node that has reported everything maps field for field.
    #[test]
    fn agent_node_maps_the_last_heartbeat_counters() {
        let seen = Utc::now();
        let mut row = inventory_row("edge-1", "online", Some(seen));
        let counters = NodeCounters {
            cpu_percent: Some(12.5),
            cpu_source: CPU_SOURCE_PROBE,
            used_memory_bytes: Some(3 << 30),
            total_memory_bytes: Some(8 << 30),
            connections: Some(17),
            rps: Some(42),
        };

        let node = agent_node(&row, &counters, 3);
        assert_eq!(node.id, row.id);
        assert_eq!(node.name, "edge-1");
        assert_eq!(node.status, "online");
        assert_eq!(node.cpu_percent, Some(12.5));
        assert_eq!(node.cpu_source, "probe");
        assert_eq!(node.memory_bytes, Some(3 << 30));
        assert_eq!(node.used_memory_bytes, Some(3 << 30));
        assert_eq!(node.total_memory_bytes, Some(8 << 30));
        assert_eq!(node.connections, Some(17));
        assert_eq!(node.rps, Some(42));

        assert_eq!(node.sites, 3);
        assert_eq!(node.last_seen, Some(seen));

        let json = serde_json::to_value(&node).expect("serialises");
        assert_eq!(json["cpu_percent"], serde_json::json!(12.5));
        assert_eq!(json["cpu_source"], serde_json::json!("probe"));
        assert_eq!(json["memory_bytes"], serde_json::json!(3u64 << 30));
        assert_eq!(json["used_memory_bytes"], serde_json::json!(3u64 << 30));
        assert_eq!(json["total_memory_bytes"], serde_json::json!(8u64 << 30));
        assert_eq!(json["sites"], serde_json::json!(3));
        assert_eq!(
            json["last_seen"],
            serde_json::json!(
                seen.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
            ),
            "timestamps use RFC 3339 in UTC, the same encoding /agents emits"
        );

        // Without a retained probe sample the capacity comes from the row, and
        // the usage stays unknown rather than borrowing the legacy figure.
        row.total_memory_bytes = Some(16 << 30);
        let node = agent_node(&row, &NodeCounters::default(), 0);
        assert_eq!(node.total_memory_bytes, Some(16 << 30));
        assert_eq!(node.used_memory_bytes, None);
        assert_eq!(node.memory_bytes, None);
    }

    /// Unknown counters are `null`, never `0`: a node that has never
    /// heartbeated must not look like an idle one.
    #[test]
    fn agent_node_reports_unknown_counters_as_null() {
        let row = inventory_row("edge-2", "offline", None);
        let node = agent_node(&row, &NodeCounters::default(), 0);
        let json = serde_json::to_value(&node).expect("serialises");

        for field in [
            "cpu_percent",
            "memory_bytes",
            "used_memory_bytes",
            "total_memory_bytes",
            "connections",
            "rps",
            "last_seen",
        ] {
            assert!(json[field].is_null(), "{field} must be null, not 0");
        }
        assert_eq!(json["sites"], serde_json::json!(0));
        // No figure, no source: the label never names `probe`/`heartbeat` for a
        // value the console renders as a dash.
        assert_eq!(json["cpu_source"], serde_json::json!("none"));
    }

    #[test]
    fn agent_node_uses_the_display_name_when_set() {
        let mut row = inventory_row("edge-2", "online", None);
        row.display_name = Some("edge backup".to_string());
        assert_eq!(
            agent_node(&row, &NodeCounters::default(), 0).name,
            "edge backup"
        );

        // Clearing the label falls back to the reported identity.
        row.display_name = None;
        assert_eq!(
            agent_node(&row, &NodeCounters::default(), 0).name,
            "edge-2"
        );
    }

    /// A NaN sample (a platform whose CPU counters are unusable) is reported
    /// as unknown rather than as a number nothing can compare — and the label
    /// follows it to `none`, so `cpu_source` can never promise a figure that is
    /// not there.
    #[test]
    fn non_finite_cpu_is_reported_as_unknown() {
        let row = inventory_row("edge-3", "online", None);
        let counters = NodeCounters {
            cpu_percent: Some(f64::NAN),
            cpu_source: CPU_SOURCE_PROBE,
            ..Default::default()
        };
        let node = agent_node(&row, &counters, 0);
        assert_eq!(node.cpu_percent, None);
        assert_eq!(node.cpu_source, "none");
    }

    /// R7: the probe is the comparable figure — a 5-second delta on every agent
    /// version — so it outranks the heartbeat whenever one exists.
    #[test]
    fn pick_cpu_prefers_the_probe_sample() {
        assert_eq!(pick_cpu(Some(12.5), Some(90.0)), (Some(12.5), "probe"));
        // "Comparable" is the rule, not "smaller": a busy probe reading still
        // outranks a diluted since-boot average.
        assert_eq!(pick_cpu(Some(80.0), Some(4.0)), (Some(80.0), "probe"));
    }

    /// No sample yet — a fresh registration, or a deployment that ships no
    /// probe samples — falls back to the heartbeat, labelled so the console can
    /// tell the two apart.
    #[test]
    fn pick_cpu_falls_back_to_the_heartbeat() {
        assert_eq!(pick_cpu(None, Some(42.25)), (Some(42.25), "heartbeat"));
    }

    /// Neither source: no figure and `none` — never `0`, which would render as
    /// an idle node, and never a source for a value that is not there.
    #[test]
    fn pick_cpu_reports_none_without_a_reading() {
        assert_eq!(pick_cpu(None, None), (None, "none"));
        // A non-finite reading is not a reading: it is skipped, not reported as
        // a number nothing can compare.
        assert_eq!(pick_cpu(Some(f64::NAN), None), (None, "none"));
        assert_eq!(
            pick_cpu(Some(f64::INFINITY), Some(7.5)),
            (Some(7.5), "heartbeat")
        );
    }

    /// The figure is passed through untouched: the node view reports the
    /// sample's own precision and never rounds it for display.
    #[test]
    fn pick_cpu_keeps_the_samples_precision() {
        let sample = 12.345_678_901_234_5_f64;
        let (value, source) = pick_cpu(Some(sample), Some(1.0));
        assert_eq!(source, "probe");
        assert_eq!(value.map(f64::to_bits), Some(sample.to_bits()));
    }

    /// R7: CPU comes from the probe even when a live heartbeat is present — the
    /// stored sample is an interval delta on every agent version, the heartbeat
    /// figure is a since-boot average before 0.28.3 and would drag the column
    /// down for an un-upgraded edge. Every other field keeps the live value,
    /// which is the fresher source.
    #[test]
    fn the_probe_cpu_wins_while_the_heartbeat_supplies_the_rest() {
        let stored = NodeCounters {
            cpu_percent: Some(3.0),
            cpu_source: CPU_SOURCE_PROBE,
            used_memory_bytes: Some(2 << 30),
            total_memory_bytes: Some(8 << 30),
            connections: Some(1),
            rps: None,
        };
        let live = HeartbeatMetrics {
            cpu_percent: 42.5,
            memory_bytes: 3 << 30,
            connections: 17,
            rps: 130,
        };

        let merged = merge_counters(&stored, Some(live));
        assert_eq!(
            merged.cpu_percent,
            Some(3.0),
            "the probe delta outranks the heartbeat figure: the latter is a \
             since-boot average on an agent that predates 0.28.3"
        );
        assert_eq!(merged.cpu_source, "probe");
        assert_eq!(merged.used_memory_bytes, Some(3 << 30));
        assert_eq!(
            merged.total_memory_bytes,
            Some(8 << 30),
            "the heartbeat never reports capacity"
        );
        assert_eq!(merged.connections, Some(17));
        assert_eq!(merged.rps, Some(130), "RPS exists only in the live report");

        // A node whose probe samples have not landed yet — a fresh
        // registration, or a deployment that ships none — still gets a figure,
        // and the label says which one it is.
        let unsampled = NodeCounters {
            used_memory_bytes: Some(2 << 30),
            total_memory_bytes: Some(8 << 30),
            ..Default::default()
        };
        let merged = merge_counters(&unsampled, Some(live));
        assert_eq!(merged.cpu_percent, Some(42.5));
        assert_eq!(merged.cpu_source, "heartbeat");

        // A zero memory reading means "could not read it", not "a node with no
        // memory": the stored sample answers instead. An idle edge reporting
        // zero connections or RPS is a real value and is kept.
        let unreadable = HeartbeatMetrics {
            memory_bytes: 0,
            connections: 0,
            rps: 0,
            ..live
        };
        let merged = merge_counters(&stored, Some(unreadable));
        assert_eq!(merged.used_memory_bytes, Some(2 << 30));
        assert_eq!(merged.connections, Some(0));
        assert_eq!(merged.rps, Some(0));
    }

    /// An agent with no live stream keeps exactly the stored sample: the
    /// registry is a `None` for every offline node, not a zeroed entry.
    #[test]
    fn stored_sample_is_used_without_a_live_heartbeat() {
        let stored = NodeCounters {
            cpu_percent: Some(3.0),
            cpu_source: CPU_SOURCE_PROBE,
            used_memory_bytes: Some(2 << 30),
            total_memory_bytes: Some(8 << 30),
            connections: Some(1),
            rps: None,
        };
        // The offline path hands the counters through untouched, label
        // included: an offline node still reports the probe figure it was last
        // sampled with.
        assert_eq!(merge_counters(&stored, None), stored);
        assert_eq!(
            merge_counters(&NodeCounters::default(), None),
            NodeCounters::default()
        );
    }

    /// The console reads these keys by name, so the JSON shape is a
    /// contract: the exact field set, nothing renamed and nothing nested.
    #[test]
    fn node_json_matches_the_console_contract() {
        let control = serde_json::to_value(control_node(
            &pingwaf_system::cpu_snapshot(),
            &pingwaf_system::memory_snapshot(),
            &pingwaf_profiler::ProfileSnapshot::collect(),
        ))
        .expect("serialises");
        let control_fields = [
            "process_cpu_percent",
            "host_cpu_percent",
            "cores",
            "rss_bytes",
            "rss_peak_bytes",
            // The commit accounting travels with the control block: the node
            // view is where the operator looks when a host stops answering.
            "vm_size_bytes",
            "committed_as_bytes",
            "commit_limit_bytes",
            "commit_percent",
            "requests",
            "avg_elapsed_ms",
        ];
        let object = control.as_object().expect("object");
        assert_eq!(object.len(), control_fields.len());
        for field in control_fields {
            assert!(object.contains_key(field), "control.{field} is missing");
        }
        // The commit accounting is numeric on the wire, and the three sizes
        // are unsigned byte counts the console can divide.
        for field in
            ["vm_size_bytes", "committed_as_bytes", "commit_limit_bytes"]
        {
            assert!(
                control[field].is_u64(),
                "control.{field} must be a byte count"
            );
        }
        assert!(
            control["commit_percent"].is_number(),
            "control.commit_percent must be a ratio"
        );

        let row = inventory_row("edge-1", "online", Some(Utc::now()));
        let agent =
            serde_json::to_value(agent_node(&row, &NodeCounters::default(), 1))
                .expect("serialises");
        let agent_fields = [
            "id",
            "name",
            "status",
            "cpu_percent",
            // R7: the source travels with the figure, so a consumer can tell a
            // probe delta from a pre-0.28.3 since-boot average.
            "cpu_source",
            "memory_bytes",
            "used_memory_bytes",
            "total_memory_bytes",
            "connections",
            "rps",
            "sites",
            "last_seen",
        ];
        let object = agent.as_object().expect("object");
        assert_eq!(object.len(), agent_fields.len());
        for field in agent_fields {
            assert!(object.contains_key(field), "agents[].{field} is missing");
        }
    }

    /// The connection gauge is a rounded count, and the impossible values a
    /// broken edge could publish are dropped instead of wrapping.
    #[test]
    fn connection_gauge_is_bounded() {
        assert_eq!(gauge_to_u64(17.4), Some(17));
        assert_eq!(gauge_to_u64(0.0), Some(0));
        assert_eq!(gauge_to_u64(-3.0), None);
        assert_eq!(gauge_to_u64(f64::NAN), None);
        assert_eq!(gauge_to_u64(f64::INFINITY), None);
    }

    /// B1 + M6: both "newest row per agent" statements are pinned to the shape
    /// that makes them an ordered index scan, and to a total order.
    ///
    /// There is no database in a unit test, so the assertion is on the
    /// statement text — which is the whole contract here:
    ///
    /// * the fleet is one bound `uuid[]` (`agent_id = ANY($1)`), never an
    ///   interpolated id list, so the plan is reused and the index condition
    ///   stays on the leading column;
    /// * the metric name is bound (`$2`), matching
    ///   `idx_agent_metrics_agent_name_time_desc (agent_id, name, recorded_at
    ///   DESC)` — migration `m20240101_000061`;
    /// * the sort is `…recorded_at DESC` / `…sampled_at DESC`, the direction
    ///   the index declares. The old all-ASC index could not satisfy the mixed
    ///   `agent_id ASC, recorded_at DESC`, which is what turned every poll into
    ///   a full sort;
    /// * `id DESC` closes the sort (M6): equal timestamps inside one batch
    ///   would otherwise make `DISTINCT ON` pick an arbitrary row.
    #[test]
    fn latest_row_queries_are_index_ordered_and_deterministic() {
        for sql in [LATEST_SAMPLES_SQL, LATEST_CONNECTIONS_SQL] {
            assert!(sql.contains("DISTINCT ON (agent_id)"), "{sql}");
            assert!(
                sql.contains("FROM host_samples")
                    || sql.contains("FROM agent_metrics"),
                "{sql}"
            );
            assert!(
                sql.contains("WHERE agent_id = ANY($1)"),
                "the fleet is one bound array: {sql}"
            );
            // `DISTINCT ON` reads the leftmost ORDER BY expressions, and the
            // index is ordered `(agent_id, …, DESC)`: the sort must start with
            // the grouped column and then walk time backwards.
            assert!(
                sql.contains("ORDER BY agent_id, "),
                "the sort starts with the index's leading column: {sql}"
            );
            assert!(
                sql.trim_end().ends_with("DESC, id DESC"),
                "the sort needs `id DESC` as its tie-breaker: {sql}"
            );
        }

        // The two equality columns lead, in the index's order; the metric name
        // is a parameter, so the statement text cannot drift per poll.
        assert!(
            LATEST_CONNECTIONS_SQL
                .contains("WHERE agent_id = ANY($1) AND name = $2"),
            "{LATEST_CONNECTIONS_SQL}"
        );
        assert!(
            !LATEST_CONNECTIONS_SQL.contains(ACTIVE_CONNECTIONS_METRIC),
            "the metric name is bound as $2, not interpolated"
        );
        assert_eq!(
            LATEST_CONNECTIONS_SQL.matches('$').count(),
            2,
            "{LATEST_CONNECTIONS_SQL}"
        );
        assert_eq!(
            LATEST_SAMPLES_SQL.matches('$').count(),
            1,
            "{LATEST_SAMPLES_SQL}"
        );
    }

    /// M5: the per-owner site tally is one aggregate row per owner. Reading
    /// every `sites.user_id` and counting in memory is what this replaces, and
    /// the difference is visible in the statement SeaORM builds.
    #[test]
    fn site_counts_are_grouped_by_owner() {
        use sea_orm::{DbBackend, QueryTrait};

        let sql = site_count_query().build(DbBackend::Postgres).sql;
        assert!(
            sql.contains("GROUP BY \"sites\".\"user_id\""),
            "the tally is grouped in the database: {sql}"
        );
        assert!(
            sql.contains("COUNT("),
            "the figure is an aggregate, not a row count: {sql}"
        );
        assert!(
            sql.contains("FROM \"sites\""),
            "the aggregate reads the sites table: {sql}"
        );
        // Nothing but the owner and its tally may be selected: an extra column
        // would mean rows come back one per site again.
        let projection = sql
            .split_once("SELECT ")
            .and_then(|(_, rest)| rest.split_once(" FROM "))
            .map(|(cols, _)| cols)
            .expect("a SELECT … FROM statement");
        assert_eq!(
            projection.matches(',').count(),
            1,
            "exactly two selected columns (user_id, count): {sql}"
        );
    }
}
