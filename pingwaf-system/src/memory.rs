//! Process and host memory snapshot for the dashboard.
//!
//! Three scopes are reported side by side so a small container reading is
//! never mistaken for host usage (or the other way round):
//!
//! * `process` — RSS/virtual bytes of this process (`memory-stats`), plus the
//!   `/proc/self` counters: `VmHWM` (peak RSS), `VmSize` (address space) and
//!   the page-fault totals.
//! * `system` — total/used memory of the machine (sysinfo) and the
//!   `/proc/meminfo` figures sysinfo does not mirror (`MemAvailable`,
//!   `SwapTotal`/`SwapFree`, `Cached`, `Buffers`, `Committed_AS`/
//!   `CommitLimit`).
//! * container — the cgroup accounting, present only under a memory-limited
//!   cgroup.
//!
//! None of the three answers the question the 43.110.22.168 incident asked:
//! how many bytes has the kernel *promised*? RSS can sit at 113 MB while
//! `Committed_AS` runs into the RAM+swap ceiling, and from then on the kernel
//! has to reclaim page cache, `iowait` saturates and the box stops answering.
//! `vm_size_bytes` (process) and
//! `committed_as_bytes` / `commit_limit_bytes` / `commit_percent` (host) put
//! the address space and the commit accounting on the dashboard, where before
//! this they were visible only in a host-side `sar` capture.
//!
//! Every `/proc` parser here is a pure function over the file text, so the
//! accounting is fixture-tested on every platform; the readers themselves
//! only fall back to an empty string. On non-Linux hosts `/proc` does not
//! exist, the parsers see an empty text and every `/proc`-derived field
//! degrades to `0` / `"0 B"`: the snapshot never fails and never panics.

use std::time::{SystemTime, UNIX_EPOCH};

use bytesize::ByteSize;
use serde::Serialize;

use crate::cgroup;

/// Human-readable byte count — one helper for every string in the snapshot
/// so the existing fields and the new ones format identically.
fn human_bytes(bytes: u64) -> String {
    ByteSize(bytes).to_string()
}

/// Memory usage of this process and the machine.
#[derive(Debug, Clone, Serialize)]
pub struct MemorySnapshot {
    /// Process RSS in bytes.
    pub rss_bytes: u64,
    /// Process virtual memory in bytes.
    pub virtual_bytes: u64,
    /// Machine (or cgroup) total memory in bytes.
    pub total_memory_bytes: u64,
    /// Machine used memory in bytes.
    pub used_memory_bytes: u64,
    /// Human-readable RSS.
    pub rss: String,
    /// Human-readable machine total.
    pub total_memory: String,
    /// Human-readable machine used.
    pub used_memory: String,
    /// Unix epoch milliseconds at which the snapshot was taken.
    pub collected_at_ms: u64,
    /// Scope label of `used_memory` (`"system"`).
    pub used_memory_scope: &'static str,
    /// Scope label of `rss` (`"process"`).
    pub process_memory_scope: &'static str,
    /// Container usage in bytes, when a cgroup limit applies.
    pub container_memory_used_bytes: Option<u64>,
    /// Container limit in bytes, when one is set.
    pub container_memory_limit_bytes: Option<u64>,
    /// Human-readable container usage.
    pub container_memory_used: Option<String>,
    /// Human-readable container limit.
    pub container_memory_limit: Option<String>,
    /// Which cgroup version produced the container numbers.
    pub container_memory_scope: Option<&'static str>,
    /// Peak process RSS since process start, in bytes — `/proc/self/status`
    /// `VmHWM`. `0` when the counter is unavailable (non-Linux).
    pub rss_peak_bytes: u64,
    /// Human-readable form of `rss_peak_bytes`.
    pub rss_peak: String,
    /// Swap in use, in bytes — `/proc/meminfo` `SwapTotal` minus `SwapFree`.
    pub swap_used_bytes: u64,
    /// Configured swap, in bytes — `/proc/meminfo` `SwapTotal`.
    pub swap_total_bytes: u64,
    /// Human-readable form of `swap_used_bytes`.
    pub swap_used: String,
    /// Human-readable form of `swap_total_bytes`.
    pub swap_total: String,
    /// Memory the kernel reports as available for new work, in bytes —
    /// `/proc/meminfo` `MemAvailable`.
    pub mem_available_bytes: u64,
    /// Human-readable form of `mem_available_bytes`.
    pub mem_available: String,
    /// `used_memory_bytes / total_memory_bytes * 100`, rounded to one
    /// decimal; `0.0` when the total is `0`. Carries the same scope as
    /// `used_memory_bytes` (`"system"`).
    pub memory_used_percent: f64,
    /// Minor page faults (served without I/O) — `/proc/self/stat` field 10.
    pub page_faults_minor: u64,
    /// Major page faults (required I/O) — `/proc/self/stat` field 12.
    pub page_faults_major: u64,
    /// Page cache in bytes, swap cache excluded — `/proc/meminfo` `Cached`.
    pub cached_bytes: u64,
    /// Block-device buffers in bytes — `/proc/meminfo` `Buffers`.
    pub buffers_bytes: u64,
    /// Human-readable form of `cached_bytes`.
    pub cached: String,
    /// Human-readable form of `buffers_bytes`.
    pub buffers: String,
    /// Virtual address space of this process in bytes — `/proc/self/status`
    /// `VmSize`, the procfs view of the figure `virtual_bytes` reports
    /// through `memory-stats`. A large allocator arena shows up here even
    /// when RSS is small: the mapping is reserved, its pages are not resident
    /// yet, so `rss_bytes` cannot see it. `0` when the counter is unavailable
    /// (non-Linux).
    pub vm_size_bytes: u64,
    /// System-wide committed anonymous memory in bytes — `/proc/meminfo`
    /// `Committed_AS`, everything the kernel has already promised to
    /// mappings across the machine.
    pub committed_as_bytes: u64,
    /// The commit ceiling in bytes — `/proc/meminfo` `CommitLimit`, i.e.
    /// `RAM * overcommit_ratio + swap`.
    pub commit_limit_bytes: u64,
    /// `committed_as_bytes / commit_limit_bytes * 100`, rounded to one
    /// decimal; `0.0` when the limit is `0`.
    ///
    /// A reading above 100 is passed through, not clamped. Under the default
    /// heuristic overcommit the ceiling is advisory, and `Committed_AS` is a
    /// per-CPU-batched estimate, so a healthy host can sit over it for a
    /// moment — and a host parked at 118 % is exactly the state this field
    /// exists to surface, which a cap at 100 would hide. Both inputs are
    /// integers and the limit is non-zero by construction, so the result is
    /// always finite and can never make serde_json reject the payload.
    pub commit_percent: f64,
    /// Human-readable form of `vm_size_bytes`.
    pub vm_size: String,
    /// Human-readable form of `committed_as_bytes`.
    pub committed_as: String,
    /// Human-readable form of `commit_limit_bytes`.
    pub commit_limit: String,
}

/// `/proc/meminfo` figures, in bytes.
///
/// Keys are matched exactly, so `Cached` never picks up `SwapCached`, and
/// values are converted from the kernel kB to bytes. A missing key — an old
/// kernel without `MemAvailable`, a non-Linux host — stays at `0`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemInfo {
    /// `SwapTotal` — configured swap in bytes.
    pub swap_total_bytes: u64,
    /// `SwapFree` — unused swap in bytes.
    pub swap_free_bytes: u64,
    /// `MemAvailable` — kernel estimate of memory available for new work
    /// without swapping (kernel 3.14+; `0` before that).
    pub available_bytes: u64,
    /// `Cached` — page cache in bytes, swap cache excluded.
    pub cached_bytes: u64,
    /// `Buffers` — block-device buffers in bytes.
    pub buffers_bytes: u64,
    /// `Committed_AS` — system-wide committed address space in bytes.
    pub committed_as_bytes: u64,
    /// `CommitLimit` — `RAM * overcommit_ratio + swap`, in bytes.
    pub commit_limit_bytes: u64,
}

impl MemInfo {
    /// Swap actually in use: `SwapTotal - SwapFree`. Saturating, so a host
    /// with swap disabled reports `0` and a transient `SwapFree` ahead of
    /// `SwapTotal` cannot underflow.
    pub fn swap_used_bytes(self) -> u64 {
        self.swap_total_bytes.saturating_sub(self.swap_free_bytes)
    }
}

/// Parses the value half of a procfs `Key:  1234 kB` line. The kernel
/// reports memory counters in kB; an unknown unit is read as bytes and a
/// malformed number yields `None`.
fn parse_kib_value(rest: &str) -> Option<u64> {
    let mut fields = rest.split_whitespace();
    let value: u64 = fields.next()?.parse().ok()?;
    match fields.next() {
        Some(unit) if unit.eq_ignore_ascii_case("kb") => {
            Some(value.saturating_mul(1024))
        },
        _ => Some(value),
    }
}

/// Parses `/proc/meminfo` text into [`MemInfo`]. Unparsable lines are
/// skipped, never fatal.
pub fn parse_meminfo(text: &str) -> MemInfo {
    let mut info = MemInfo::default();
    for line in text.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let Some(bytes) = parse_kib_value(rest) else {
            continue;
        };
        match key.trim() {
            "SwapTotal" => info.swap_total_bytes = bytes,
            "SwapFree" => info.swap_free_bytes = bytes,
            "MemAvailable" => info.available_bytes = bytes,
            "Cached" => info.cached_bytes = bytes,
            "Buffers" => info.buffers_bytes = bytes,
            "Committed_AS" => info.committed_as_bytes = bytes,
            "CommitLimit" => info.commit_limit_bytes = bytes,
            _ => {},
        }
    }
    info
}

/// Parses the two commit accounting figures out of `/proc/meminfo` text:
/// `(Committed_AS, CommitLimit)`, both in bytes.
///
/// `Committed_AS` is the memory the kernel has already promised to mappings
/// system-wide; `CommitLimit` is the ceiling it refuses to exceed when
/// `vm.overcommit_memory=2` (`RAM * overcommit_ratio + swap`). Under the
/// default heuristic overcommit that ceiling is advisory, so the first figure
/// can legitimately run past the second — the ratio is reported as it is, see
/// [`MemorySnapshot::commit_percent`].
///
/// Delegates to [`parse_meminfo`], so the two readers share one set of key
/// matches and cannot drift apart. Missing keys stay at `0`: a non-Linux host
/// reads `""` and gets `(0, 0)`.
pub fn parse_meminfo_commit(text: &str) -> (u64, u64) {
    let info = parse_meminfo(text);
    (info.committed_as_bytes, info.commit_limit_bytes)
}

/// Parses one `Key:  1234 kB` counter out of a procfs status file, where
/// `key` is the literal line prefix *including* the colon (`"VmHWM:"`,
/// `"VmSize:"`), which makes this an exact key match: a `VmPeak:` line can
/// never answer for `VmSize`, and vice versa. An absent, truncated or
/// unparsable counter reports `0`.
fn parse_status_kib(text: &str, key: &str) -> u64 {
    text.lines()
        .find_map(|line| line.strip_prefix(key))
        .and_then(parse_kib_value)
        .unwrap_or_default()
}

/// Parses `VmHWM` — the peak resident set size, i.e. the high-water mark
/// since process start — out of `/proc/self/status`. An absent counter
/// (non-Linux, stripped-down procfs) reports `0`.
pub fn parse_vm_hwm(text: &str) -> u64 {
    parse_status_kib(text, "VmHWM:")
}

/// Parses `VmSize` — the whole virtual address space of the process, glibc
/// and jemalloc arenas included — out of `/proc/self/status`.
///
/// This is the counter RSS cannot stand in for: an arena that has been
/// `mmap`ed and committed to is address space (and counts towards the host
/// `Committed_AS`) long before its pages are touched, so a small RSS with a
/// huge `VmSize` is the signature of a reservation waiting to happen. An
/// absent counter (non-Linux) reports `0`.
pub fn parse_vm_size(text: &str) -> u64 {
    parse_status_kib(text, "VmSize:")
}

/// Parses the page-fault counters out of `/proc/self/stat`: field 10
/// (`minflt`, faults served without I/O) and field 12 (`majflt`, faults
/// that needed disk I/O).
///
/// Field 2 (`comm`) is wrapped in parentheses and may itself contain spaces
/// and parentheses (`(my proc)`), so the field split starts after the
/// *last* `)`. A naive whitespace split would shift every counter by the
/// spaces in the process name. Truncated input yields `(0, 0)`.
pub fn parse_page_faults(text: &str) -> (u64, u64) {
    let Some(close) = text.rfind(')') else {
        return (0, 0);
    };
    let mut fields = text[close + 1..].split_whitespace();
    // Field 3 (`state`) is the first token after the parenthesis, so field
    // 10 is token 8 (index 7) and field 12 sits two tokens further on.
    let minor: Option<u64> = fields.nth(7).and_then(|raw| raw.parse().ok());
    let major: Option<u64> = fields.nth(1).and_then(|raw| raw.parse().ok());
    (minor.unwrap_or_default(), major.unwrap_or_default())
}

/// `used / total * 100`, rounded to one decimal.
///
/// `0.0` when `total` is `0`: no division happens, so the value is always
/// finite and safe to serialize into JSON. A reading above 100 is passed
/// through rather than clamped — `used` and `total` then came from
/// different sources, and surfacing that beats hiding it.
pub fn memory_used_percent(used: u64, total: u64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    let percent = used as f64 / total as f64 * 100.0;
    (percent * 10.0).round() / 10.0
}

/// Reads a procfs file as text. A missing file — non-Linux host, restricted
/// container, stripped-down procfs — yields an empty string, which every
/// parser above maps to its documented zero value.
fn read_proc(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Collects a [`MemorySnapshot`]. Cheap enough for a polling dashboard: a
/// handful of small procfs reads plus one sysinfo refresh.
pub fn memory_snapshot() -> MemorySnapshot {
    let (rss_bytes, virtual_bytes) = match memory_stats::memory_stats() {
        Some(stats) => (stats.physical_mem as u64, stats.virtual_mem as u64),
        None => (0, 0),
    };
    let kind = sysinfo::MemoryRefreshKind::nothing().with_ram();
    let mut sys = sysinfo::System::new_with_specifics(
        sysinfo::RefreshKind::nothing().with_memory(kind),
    );
    sys.refresh_memory();
    let container = cgroup::container_memory();
    // The `/proc` extras are parsed from the raw text rather than taken from
    // sysinfo: every number gets a fixture-tested parser, independent of the
    // sysinfo version, and a file that cannot be read reads as "" and parses
    // to 0.
    let meminfo = parse_meminfo(&read_proc("/proc/meminfo"));
    // One read of `/proc/self/status` answers both counters: two reads could
    // disagree with each other, and the file is the same either way.
    let status = read_proc("/proc/self/status");
    let rss_peak_bytes = parse_vm_hwm(&status);
    let vm_size_bytes = parse_vm_size(&status);
    let (page_faults_minor, page_faults_major) =
        parse_page_faults(&read_proc("/proc/self/stat"));
    let total_memory_bytes = sys.total_memory();
    let used_memory_bytes = sys.used_memory();
    let swap_used_bytes = meminfo.swap_used_bytes();
    let committed_as_bytes = meminfo.committed_as_bytes;
    let commit_limit_bytes = meminfo.commit_limit_bytes;
    let collected_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default();
    MemorySnapshot {
        rss_bytes,
        virtual_bytes,
        total_memory_bytes,
        used_memory_bytes,
        rss: human_bytes(rss_bytes),
        total_memory: human_bytes(total_memory_bytes),
        used_memory: human_bytes(used_memory_bytes),
        collected_at_ms,
        used_memory_scope: "system",
        process_memory_scope: "process",
        container_memory_used_bytes: container.as_ref().map(|c| c.used_bytes),
        container_memory_limit_bytes: container.and_then(|c| c.limit_bytes),
        container_memory_used: container
            .as_ref()
            .map(|c| human_bytes(c.used_bytes)),
        container_memory_limit: container
            .and_then(|c| c.limit_bytes)
            .map(human_bytes),
        container_memory_scope: container.map(|c| c.version.scope_label()),
        rss_peak_bytes,
        rss_peak: human_bytes(rss_peak_bytes),
        swap_used_bytes,
        swap_total_bytes: meminfo.swap_total_bytes,
        swap_used: human_bytes(swap_used_bytes),
        swap_total: human_bytes(meminfo.swap_total_bytes),
        mem_available_bytes: meminfo.available_bytes,
        mem_available: human_bytes(meminfo.available_bytes),
        memory_used_percent: memory_used_percent(
            used_memory_bytes,
            total_memory_bytes,
        ),
        page_faults_minor,
        page_faults_major,
        cached_bytes: meminfo.cached_bytes,
        buffers_bytes: meminfo.buffers_bytes,
        cached: human_bytes(meminfo.cached_bytes),
        buffers: human_bytes(meminfo.buffers_bytes),
        vm_size_bytes,
        committed_as_bytes,
        commit_limit_bytes,
        // The same `used / total * 100` the memory percentage uses: one
        // rounding rule for every percentage on the dashboard, and `0.0` when
        // there is no total to divide by.
        commit_percent: memory_used_percent(
            committed_as_bytes,
            commit_limit_bytes,
        ),
        vm_size: human_bytes(vm_size_bytes),
        committed_as: human_bytes(committed_as_bytes),
        commit_limit: human_bytes(commit_limit_bytes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim `/proc/meminfo` excerpt of a 16 GB box with 8 GB swap and
    /// `vm.overcommit_ratio = 50`, so `CommitLimit` is RAM/2 + swap = 16 GiB.
    /// `Committed_AS` carries the 1.85 GB the incident host reported while
    /// RSS sat at 113 MB. Values are kB; the bare counters at the end carry
    /// no unit.
    const MEMINFO: &str = "\
MemTotal:       16316396 kB
MemFree:          882848 kB
MemAvailable:    9428488 kB
Buffers:          310504 kB
Cached:          6213860 kB
SwapCached:            0 kB
SwapTotal:       8388604 kB
SwapFree:        7340036 kB
Committed_AS:    1939866 kB
CommitLimit:    16777216 kB
Dirty:               344 kB
HugePages_Total:       0
";

    #[test]
    fn meminfo_parses_swap_available_cached_buffers() {
        let info = parse_meminfo(MEMINFO);
        assert_eq!(info.available_bytes, 9_428_488 * 1024);
        // `Cached` must not be confused with the `SwapCached` line.
        assert_eq!(info.cached_bytes, 6_213_860 * 1024);
        assert_eq!(info.buffers_bytes, 310_504 * 1024);
        assert_eq!(info.swap_total_bytes, 8_388_604 * 1024);
        assert_eq!(info.swap_free_bytes, 7_340_036 * 1024);
        assert_eq!(info.swap_used_bytes(), (8_388_604 - 7_340_036) * 1024);
        // SwapFree ahead of SwapTotal (transient, but reachable) must not
        // underflow.
        let odd = parse_meminfo("SwapTotal: 100 kB\nSwapFree: 200 kB\n");
        assert_eq!(odd.swap_used_bytes(), 0);
    }

    /// The two figures that decide whether the kernel still has room to
    /// promise memory: `Committed_AS` and `CommitLimit`.
    #[test]
    fn meminfo_parses_committed_as_and_limit() {
        let info = parse_meminfo(MEMINFO);
        assert_eq!(info.committed_as_bytes, 1_939_866 * 1024);
        assert_eq!(info.commit_limit_bytes, 16_777_216 * 1024);
        // The tuple reader is the same parse, so the two cannot disagree.
        assert_eq!(
            parse_meminfo_commit(MEMINFO),
            (info.committed_as_bytes, info.commit_limit_bytes)
        );
    }

    /// `VmSize` is the address space RSS cannot stand in for: a reserved
    /// arena lands here long before its pages are resident.
    #[test]
    fn status_parses_vm_size() {
        let status = "\
Name:\tpingwaf
State:\tS (sleeping)
VmPeak:\t 4212340 kB
VmSize:\t 4193300 kB
VmHWM:\t  178932 kB
VmRSS:\t  120044 kB
";
        // Neither `VmPeak` nor `VmRSS` may answer for `VmSize`, and a huge
        // address space next to a small resident set is a normal reading.
        assert_eq!(parse_vm_size(status), 4_193_300 * 1024);
        assert_eq!(parse_vm_hwm(status), 178_932 * 1024);
        // Absent off Linux: zero, not a failure.
        assert_eq!(parse_vm_size("Name:\tpingwaf\n"), 0);
        assert_eq!(parse_vm_size("VmSize:\tgarbage kB\n"), 0);
        assert_eq!(parse_vm_size(""), 0);
    }

    /// A missing key is a zero, never a panic: no `/proc` on this platform,
    /// an old kernel without the commit counters, a truncated line.
    #[test]
    fn missing_commit_fields_are_zero() {
        assert_eq!(parse_meminfo_commit(""), (0, 0));
        assert_eq!(parse_meminfo_commit("\n\n"), (0, 0));
        assert_eq!(parse_vm_size(""), 0);
        let info =
            parse_meminfo("MemAvailable: 1024 kB\nCommitted_AS: nope kB\n");
        assert_eq!(info.available_bytes, 1024 * 1024);
        assert_eq!(info.committed_as_bytes, 0);
        assert_eq!(info.commit_limit_bytes, 0);
        // One of the pair present leaves the other at zero.
        assert_eq!(
            parse_meminfo_commit("CommitLimit: 1024 kB\n"),
            (0, 1024 * 1024)
        );
    }

    #[test]
    fn commit_percent_rounds_to_one_decimal() {
        let gib = 1u64 << 30;
        // 1.234 GB promised against a 1.5 GB limit: 82.2666…% -> 82.3.
        let committed = (1.234 * gib as f64) as u64;
        let limit = (1.5 * gib as f64) as u64;
        assert_eq!(memory_used_percent(committed, limit), 82.3);
        // No limit, no ratio: 0.0 rather than a division by zero.
        assert_eq!(memory_used_percent(committed, 0), 0.0);
        assert_eq!(memory_used_percent(0, 0), 0.0);
        // Exactly at the ceiling is 100.0; the rounding is one decimal place,
        // not one significant figure.
        assert_eq!(memory_used_percent(limit, limit), 100.0);
        assert_eq!(memory_used_percent(limit / 3, limit), 33.3);
    }

    /// `Committed_AS` above `CommitLimit` is a real reading, not an error:
    /// under the default heuristic overcommit the ceiling is advisory and the
    /// first figure is a batched estimate, so a host can sit at 118 %.
    ///
    /// The ratio is therefore reported as measured instead of being clamped
    /// to 100 — a cap would hide exactly the exhaustion this field exists to
    /// surface. What is guaranteed is that it stays finite: both inputs are
    /// integers and the limit is non-zero here, so serde_json can never be
    /// handed a `NaN` that would fail the whole response.
    #[test]
    fn commit_percent_never_exceeds_a_sane_range() {
        let gib = 1u64 << 30;
        assert_eq!(memory_used_percent(2 * gib, gib), 200.0);
        assert_eq!(memory_used_percent(11 * gib, 10 * gib), 110.0);
        // The incident shape: 1.85 GiB promised against a 1.84 GB ceiling.
        assert_eq!(
            memory_used_percent(1_939_866 * 1024, 1_843_200 * 1024),
            105.2
        );
        // Extreme inputs neither panic nor produce a non-finite ratio.
        let extreme = memory_used_percent(u64::MAX, 1);
        assert!(extreme.is_finite());
        assert!(extreme > 100.0);
        assert!(memory_used_percent(1, u64::MAX).is_finite());
        assert_eq!(memory_used_percent(u64::MAX, u64::MAX), 100.0);
    }

    #[test]
    fn status_parses_vmhwm() {
        let status = "\
Name:\tpingwaf
State:\tS (sleeping)
VmPeak:\t  812345 kB
VmHWM:\t  178932 kB
VmRSS:\t  120044 kB
";
        assert_eq!(parse_vm_hwm(status), 178_932 * 1024);
        // The counter is absent off Linux: zero, not a failure.
        assert_eq!(parse_vm_hwm("Name:\tpingwaf\n"), 0);
        assert_eq!(parse_vm_hwm("VmHWM:\tgarbage kB\n"), 0);
    }

    #[test]
    fn stat_parses_page_faults_with_spaces_in_comm() {
        // Fields after the closing parenthesis: 3 state, 4 ppid, 5 pgrp,
        // 6 session, 7 tty_nr, 8 tpgid, 9 flags, 10 minflt, 11 cminflt,
        // 12 majflt, 13 cmajflt.
        let stat = "123 (my proc) S 1 2 3 4 5 6 111 222 333 444 22 0 0";
        assert_eq!(parse_page_faults(stat), (111, 333));
        // Parentheses inside the name are tolerated too: the split happens
        // after the last parenthesis, not the first.
        let nested = "123 (web (render) proc) R 1 2 3 4 5 6 7 8 9 10 0 0";
        assert_eq!(parse_page_faults(nested), (7, 9));
        // Truncated or non-stat input is not fatal.
        assert_eq!(parse_page_faults("123 (no close"), (0, 0));
        assert_eq!(parse_page_faults(""), (0, 0));
        assert_eq!(parse_page_faults("123 (short) S 1 2"), (0, 0));
    }

    #[test]
    fn missing_fields_default_to_zero() {
        // Off Linux every reader sees "": the whole snapshot degrades to 0.
        assert_eq!(parse_meminfo(""), MemInfo::default());
        assert_eq!(parse_meminfo("").swap_used_bytes(), 0);
        assert_eq!(parse_vm_hwm(""), 0);
        assert_eq!(parse_page_faults(""), (0, 0));
        // A single missing key leaves only that field at zero.
        let info = parse_meminfo("MemAvailable: 1024 kB\nSwapTotal: 2048 kB\n");
        assert_eq!(info.available_bytes, 1024 * 1024);
        assert_eq!(info.swap_total_bytes, 2048 * 1024);
        assert_eq!(info.cached_bytes, 0);
        assert_eq!(info.buffers_bytes, 0);
        // A malformed number is skipped, never turned into garbage.
        assert_eq!(parse_meminfo("Cached: nope kB\n").cached_bytes, 0);
    }

    #[test]
    fn used_percent_hits_the_boundaries() {
        // No total (non-Linux, or a failed refresh) is 0.0, not NaN: the JSON
        // payload must stay finite.
        assert_eq!(memory_used_percent(0, 0), 0.0);
        assert_eq!(memory_used_percent(5_000, 0), 0.0);
        assert_eq!(memory_used_percent(0, 1), 0.0);
        assert_eq!(memory_used_percent(1, 8), 12.5);
        assert_eq!(memory_used_percent(1, 3), 33.3);
        assert_eq!(memory_used_percent(1, 1), 100.0);
        // used > total is passed through instead of clamped.
        assert_eq!(memory_used_percent(3, 2), 150.0);
        assert!(memory_used_percent(u64::MAX, 1).is_finite());
    }

    #[test]
    fn snapshot_is_plausible() {
        let snapshot = memory_snapshot();
        assert!(snapshot.collected_at_ms > 0);
        assert!(snapshot.rss_bytes > 0, "process RSS should be visible");
        assert!(!snapshot.rss.is_empty());
        // The human-readable fields are populated even when the number behind
        // them is the non-Linux zero.
        for rendered in [
            &snapshot.rss_peak,
            &snapshot.swap_used,
            &snapshot.swap_total,
            &snapshot.mem_available,
            &snapshot.cached,
            &snapshot.buffers,
            &snapshot.vm_size,
            &snapshot.committed_as,
            &snapshot.commit_limit,
        ] {
            assert!(!rendered.is_empty());
        }
        assert!(snapshot.memory_used_percent.is_finite());
        assert!(snapshot.memory_used_percent >= 0.0);
        // The commit ratio is a number the payload can carry, and it is 0.0
        // exactly when there is no ceiling to divide by.
        assert!(snapshot.commit_percent.is_finite());
        assert!(snapshot.commit_percent >= 0.0);
        if snapshot.commit_limit_bytes == 0 {
            assert_eq!(snapshot.commit_percent, 0.0);
        }
        // A container limit may be absent, but when present it is positive.
        if let Some(limit) = snapshot.container_memory_limit_bytes {
            assert!(limit > 0);
        }
        // Linux keeps the peak RSS counter; a high-water mark can never sit
        // below the RSS sampled at the start of this snapshot.
        if snapshot.rss_peak_bytes > 0 {
            assert!(snapshot.rss_peak_bytes >= snapshot.rss_bytes);
        }
    }

    /// End-to-end read of the real procfs, checked field by field and
    /// skipped when a file is not there (a sandbox without /proc).
    #[cfg(target_os = "linux")]
    #[test]
    fn real_proc_files_parse() {
        let meminfo = read_proc("/proc/meminfo");
        if !meminfo.is_empty() {
            assert!(parse_meminfo(&meminfo).available_bytes > 0);
            // Every running system has promised something; `CommitLimit` is
            // left unasserted because RAM with `overcommit_ratio = 0` and no
            // swap legitimately reports 0.
            assert!(parse_meminfo_commit(&meminfo).0 > 0);
        }
        let status = read_proc("/proc/self/status");
        if !status.is_empty() {
            assert!(parse_vm_hwm(&status) > 0);
            assert!(
                parse_vm_size(&status) >= parse_vm_hwm(&status),
                "the address space covers the resident set"
            );
        }
        let stat = read_proc("/proc/self/stat");
        if !stat.is_empty() {
            assert!(
                parse_page_faults(&stat).0 > 0,
                "a live process has minor faults"
            );
        }
    }

    /// Field names are the JSON contract of `GET /api/v1/system/metrics`,
    /// so renaming one is an API break and is pinned here.
    #[test]
    fn snapshot_serializes_new_fields() {
        let json = serde_json::to_value(memory_snapshot()).expect("serialize");
        for key in [
            "rss_peak_bytes",
            "rss_peak",
            "swap_used_bytes",
            "swap_total_bytes",
            "swap_used",
            "swap_total",
            "mem_available_bytes",
            "mem_available",
            "memory_used_percent",
            "page_faults_minor",
            "page_faults_major",
            "cached_bytes",
            "buffers_bytes",
            "cached",
            "buffers",
            "vm_size_bytes",
            "committed_as_bytes",
            "commit_limit_bytes",
            "commit_percent",
            "vm_size",
            "committed_as",
            "commit_limit",
        ] {
            assert!(
                json.get(key).is_some(),
                "missing {key} in the snapshot JSON"
            );
        }
        // A non-finite percentage would make serde_json reject the whole
        // payload (HTTP 500), so the field is always a number.
        assert!(json["memory_used_percent"].is_number());
        assert!(json["commit_percent"].is_number());
        for key in ["vm_size_bytes", "committed_as_bytes", "commit_limit_bytes"]
        {
            assert!(
                json[key].is_u64(),
                "{key} is an unsigned byte count on the wire"
            );
        }
        for key in ["vm_size", "committed_as", "commit_limit"] {
            assert!(json[key].is_string(), "{key} is a rendered size");
        }
    }
}
