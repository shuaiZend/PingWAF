// Allocator retention and reservation policy.
//
// Two mimalloc knobs decide whether a small host survives:
//
//   * `purge_delay` — how long freed pages are retained before going back to
//     the OS. The shipped images pinned it at 1000 ms on every machine
//     (mimalloc's own default is 10 ms); on a 1 GB host the retained pages are
//     the difference between comfortable and OOM (2026-10-15/17 post-mortems:
//     RSS was allocator high-water retention, not live objects).
//   * `arena_reserve` — the *virtual* size of the arena mimalloc carves out of
//     the OS. It defaults to 1 GiB on 64-bit (`MI_DEFAULT_ARENA_RESERVE =
//     1024*1024` KiB, v3 options.c:53), and with `arena_eager_commit` at its
//     default of 2 the whole reservation is committed up front on an
//     overcommit kernel (arena.c::mi_arena_reserve: `arena_commit = overcommit`).
//     That is the 2026-11-06 path: `smaps` showed a single `[anon:mimalloc]`
//     with Size 1 GiB / Rss 93 MB inside a process with VmSize 2.1 GiB, while
//     `Committed_AS` climbed 1.02 GB → 1.84 GB, i.e. into RAM+swap. The kernel
//     then dropped page cache, the disk went read-only at 108 MB/s with
//     iowait 88% and idle 0, and the box stopped answering (ping still did).
//
// Both are derived from the *actual* memory ceiling (cgroup limit, then host
// total) and both can be overridden. Precedence is `PINGWAF_*` > `MIMALLOC_*`
// > adaptive:
//
//   PINGWAF_MALLOC_PURGE_DELAY_MS   explicit, preferred
//   MIMALLOC_PURGE_DELAY            standard mimalloc env, still honoured
//   PINGWAF_MALLOC_ARENA_RESERVE_MB explicit, in MiB, preferred
//   MIMALLOC_ARENA_RESERVE          standard mimalloc env (bytes, or `64M`)
//   PINGWAF_MALLOC_EAGER_COMMIT     explicit, 0/1
//   MIMALLOC_ARENA_EAGER_COMMIT     standard mimalloc env, 0/1
//
// Adaptive buckets (see `purge_delay_for`, `arena_reserve_for`,
// `eager_commit_for` below).
//
// **Runtime set vs. startup env — read this before trusting `apply()`.**
// `mi_option_set` updates the option, but mimalloc only reads `arena_reserve`
// when it *creates* an arena (arena.c::mi_arena_reserve) and it never shrinks a
// mapping that already exists. The first arena is normally created during early
// startup, before this module runs, so a container that must actually hold a
// smaller arena has to pass `MIMALLOC_ARENA_RESERVE` (and
// `MIMALLOC_ARENA_EAGER_COMMIT`) in its **process environment** — mimalloc
// parses those in `_mi_options_init`, a load-time constructor (init.c:1102).
// `docker-compose.yml` does exactly that. The calls below remain as a fallback
// for a cold process and so the effective value can be read back and logged;
// whether they shrink the *live* arena is a `smaps` question after a restart,
// not something this module can promise.

use std::os::raw::{c_int, c_long};
use std::sync::Once;

// `mi_option_t` ordinals, taken from the bundled mimalloc **v3** enum.
// libmimalloc-sys 0.1.49 compiles `c_src/mimalloc/v3` unless something in the
// dependency graph turns on its `v2` feature — nothing here does (the `mimalloc`
// crate is pulled in with default features). Verified through the include dir
// the build script prints: `.../libmimalloc-sys-0.1.49/c_src/mimalloc/v3/include`.
// Source, `c_src/mimalloc/v3/include/mimalloc.h:437-487`:
//
//    437  typedef enum mi_option_e {
//    439    mi_option_show_errors,                 // 0
//    440    mi_option_show_stats,                  // 1
//    441    mi_option_verbose,                     // 2
//    443    mi_option_deprecated_eager_commit,      // 3  (v2: eager_commit)
//    444    mi_option_arena_eager_commit,           // 4
//    ...
//    449    mi_option_reserve_os_memory,            // 9
//    ...
//    455    mi_option_purge_delay,                  // 15
//    ...
//    463    mi_option_arena_reserve,                // 23
//
// The five values are identical in the v2 enum, so the pin below holds for
// either bundled version. Re-check on libmimalloc-sys upgrades: a wrong ordinal
// does not fail loudly, it silently sets an unrelated option.
const MI_OPTION_EAGER_COMMIT: c_int = 3;
const MI_OPTION_ARENA_EAGER_COMMIT: c_int = 4;
const MI_OPTION_RESERVE_OS_MEMORY: c_int = 9;
const MI_OPTION_PURGE_DELAY: c_int = 15;
const MI_OPTION_ARENA_RESERVE: c_int = 23;

unsafe extern "C" {
    /// mimalloc v2/v3 runtime option setter (statically linked into this
    /// binary by the `mimalloc` crate). Size-typed options take **KiB**.
    fn mi_option_set(option: c_int, value: c_long);
    /// Read-back, used by the startup log so operators can verify the
    /// effective value without guessing.
    fn mi_option_get(option: c_int) -> c_long;
}

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;

const ADAPTIVE: &str = "adaptive";

/// Purge delay (ms) bucketed by the memory ceiling. Zero means "return
/// freed pages immediately" — a little more CPU for a materially lower RSS
/// on small hosts.
pub fn purge_delay_for(total_bytes: u64) -> i64 {
    match total_bytes / MIB {
        0..=1023 => 0,
        1024..=4095 => 250,
        4096..=16384 => 500,
        _ => 1000,
    }
}

/// Arena reserve bucketed by the memory ceiling. A 1 GiB host must not
/// reserve a 1 GiB arena: that reservation is what pushed `Committed_AS` to
/// the RAM+swap limit on 2026-11-06.
///
/// `None` (the probe found no cgroup limit and no host total) is treated as
/// the smallest host, matching `purge_delay_for`'s "safe side" convention.
pub fn arena_reserve_for(total: Option<u64>) -> u64 {
    match total.map(|bytes| bytes / MIB) {
        None | Some(0..=1023) => 64 * MIB,
        Some(1024..=4095) => 256 * MIB,
        Some(4096..=16384) => 512 * MIB,
        Some(_) => 1024 * MIB,
    }
}

/// Eager commit keeps the whole reservation committed; off on small hosts.
/// `None` is treated like "under 1 GiB": do not commit up front.
pub fn eager_commit_for(total: Option<u64>) -> bool {
    total.is_some_and(|bytes| bytes / MIB >= 4096)
}

/// Convert a byte count to the KiB unit mimalloc stores size options in
/// (`mi_option_get_size` multiplies by `MI_KiB`). Skipping this conversion
/// would reserve 1024x the intended amount.
fn bytes_to_kib(bytes: u64) -> c_long {
    bytes.div_ceil(KIB).min(c_long::MAX as u64) as c_long
}

/// KiB (as mimalloc reports it) to MiB, for the startup log.
fn kib_to_mib(kib: c_long) -> u64 {
    kib.max(0) as u64 * KIB / MIB
}

/// Report an override that was dropped. Goes to stderr rather than through
/// `tracing`: `apply()` runs before a subscriber exists, so an `info!` here
/// would be swallowed and the operator would never learn their value was
/// ignored. Never panics — a bad env var must not stop the process booting.
fn warn_ignored(key: &str, raw: &str, what: &str) {
    eprintln!("pingwaf: allocator WARN ignoring {what} override {key}={raw}");
}

/// Explicit operator override of the purge delay. `PINGWAF_*` wins over the
/// standard mimalloc variable so a deployment can pin its own value without
/// relying on the image default; `-1` (never purge) is accepted but logged.
fn purge_delay_override() -> Option<(i64, &'static str)> {
    for (key, source) in [
        (
            "PINGWAF_MALLOC_PURGE_DELAY_MS",
            "env:PINGWAF_MALLOC_PURGE_DELAY_MS",
        ),
        ("MIMALLOC_PURGE_DELAY", "env:MIMALLOC_PURGE_DELAY"),
    ] {
        let Ok(raw) = std::env::var(key) else {
            continue;
        };
        match raw.trim().parse::<i64>() {
            Ok(value) if value >= -1 => return Some((value, source)),
            // stderr, not tracing: this runs before a subscriber exists, so
            // an `info!` here would be dropped and the operator would never
            // learn that their override was ignored.
            _ => warn_ignored(key, &raw, "purge delay"),
        }
    }
    None
}

/// Parse a mimalloc-style size string. A bare number means **bytes**; `K`/`M`/
/// `G` (with optional `iB`/`B`) scale it. This mirrors
/// `options.c::mi_option_init`, which converts a bare value to KiB for
/// size-typed options — so `67108864` and `64M` both mean 64 MiB there, and
/// must mean 64 MiB here too.
fn parse_mimalloc_size(raw: &str) -> Option<u64> {
    const SUFFIXES: [(&str, u64); 10] = [
        ("KIB", KIB),
        ("MIB", MIB),
        ("GIB", MIB * KIB),
        ("KB", KIB),
        ("MB", MIB),
        ("GB", MIB * KIB),
        ("K", KIB),
        ("M", MIB),
        ("G", MIB * KIB),
        ("B", 1),
    ];
    let upper = raw.trim().to_ascii_uppercase();
    for (suffix, multiplier) in SUFFIXES {
        if let Some(digits) = upper.strip_suffix(suffix) {
            let value = digits.trim().parse::<u64>().ok()?;
            return value.checked_mul(multiplier).filter(|bytes| *bytes > 0);
        }
    }
    upper.parse::<u64>().ok().filter(|bytes| *bytes > 0)
}

/// Arena reserve override, normalized to bytes. `PINGWAF_MALLOC_ARENA_RESERVE_MB`
/// is in MiB (operators size in MiB); `MIMALLOC_ARENA_RESERVE` follows
/// mimalloc's own convention (bytes, suffix accepted). A `0`/unparsable value
/// is ignored with a log line rather than panicking — a bad env var must never
/// stop the process from booting.
fn arena_reserve_override() -> Option<(u64, &'static str)> {
    for (key, source, in_mib) in [
        (
            "PINGWAF_MALLOC_ARENA_RESERVE_MB",
            "env:PINGWAF_MALLOC_ARENA_RESERVE_MB",
            true,
        ),
        (
            "MIMALLOC_ARENA_RESERVE",
            "env:MIMALLOC_ARENA_RESERVE",
            false,
        ),
    ] {
        let Ok(raw) = std::env::var(key) else {
            continue;
        };
        let parsed = if in_mib {
            raw.trim()
                .parse::<u64>()
                .ok()
                .and_then(|mib| mib.checked_mul(MIB))
        } else {
            parse_mimalloc_size(&raw)
        };
        match parsed.filter(|bytes| *bytes > 0) {
            Some(bytes) => return Some((bytes, source)),
            // stderr: see purge_delay_override.
            None => warn_ignored(key, &raw, "arena reserve"),
        }
    }
    None
}

/// Arena eager-commit override. Only `0`/`1` are accepted: `MIMALLOC_ARENA_EAGER_COMMIT=2`
/// is mimalloc's own "eager only where the kernel overcommits" default and is
/// treated as "no override" rather than reinterpreted.
fn eager_commit_override() -> Option<(bool, &'static str)> {
    for (key, source) in [
        (
            "PINGWAF_MALLOC_EAGER_COMMIT",
            "env:PINGWAF_MALLOC_EAGER_COMMIT",
        ),
        (
            "MIMALLOC_ARENA_EAGER_COMMIT",
            "env:MIMALLOC_ARENA_EAGER_COMMIT",
        ),
    ] {
        let Ok(raw) = std::env::var(key) else {
            continue;
        };
        match raw.trim() {
            "0" => return Some((false, source)),
            "1" => return Some((true, source)),
            other => warn_ignored(key, other, "eager commit"),
        }
    }
    None
}

/// One `source=` token for the startup log: `adaptive`, or the `+`-joined list
/// of variables that actually took effect.
fn source_label(sources: [&'static str; 3]) -> String {
    let explicit: Vec<&'static str> = sources
        .into_iter()
        .filter(|source| *source != ADAPTIVE)
        .collect();
    if explicit.is_empty() {
        ADAPTIVE.to_owned()
    } else {
        explicit.join("+")
    }
}

/// Applies the retention and reservation policy once per process. Safe to call
/// from both entry points (all-in-one and the pingap-compatible one).
pub fn apply() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let total = pingwaf_system::total_bytes();
        // Below 1 GiB, `reserve_os_memory` is pure risk — it is a second
        // reservation on top of the arena — so it is forced to 0 there even if
        // an operator asked for it. Its default is already 0 (v3 options.c:76),
        // which is why larger hosts are left alone.
        let small_host = total.is_none_or(|bytes| bytes < 1024 * MIB);

        let (delay, delay_source) = purge_delay_override()
            .unwrap_or_else(|| (purge_delay_for(total.unwrap_or(0)), ADAPTIVE));
        let (arena_bytes, arena_source) =
            arena_reserve_override().unwrap_or_else(|| (arena_reserve_for(total), ADAPTIVE));
        let (eager, eager_source) =
            eager_commit_override().unwrap_or_else(|| (eager_commit_for(total), ADAPTIVE));
        let source = source_label([delay_source, arena_source, eager_source]);

        let eager_flag: c_long = if eager { 1 } else { 0 };
        let arena_kib = bytes_to_kib(arena_bytes);

        let (purge_effective, arena_effective, eager_effective, reserve_os_effective) = {
            // SAFETY: mi_option_set/mi_option_get are mimalloc's documented C
            // ABI (mimalloc.h:454/451) and the allocator is statically linked
            // into this binary. Ordinals are pinned to libmimalloc-sys 0.1.49
            // (see the consts above).
            unsafe {
                mi_option_set(MI_OPTION_PURGE_DELAY, delay);
                // `mi_option_eager_commit` (3) is `deprecated_eager_commit` in
                // mimalloc v3 — nothing in v3's src/ reads it any more, so on
                // the bundled version it is inert. `arena_eager_commit` (4) is
                // the option `mi_arena_reserve` actually consults. Set both:
                // 3 preserves the v2 meaning if the `v2` feature is ever
                // enabled, 4 is the one that works today.
                mi_option_set(MI_OPTION_EAGER_COMMIT, eager_flag);
                mi_option_set(MI_OPTION_ARENA_EAGER_COMMIT, eager_flag);
                // KiB, not bytes — see bytes_to_kib.
                mi_option_set(MI_OPTION_ARENA_RESERVE, arena_kib);
                if small_host {
                    mi_option_set(MI_OPTION_RESERVE_OS_MEMORY, 0);
                }
                (
                    mi_option_get(MI_OPTION_PURGE_DELAY),
                    mi_option_get(MI_OPTION_ARENA_RESERVE),
                    mi_option_get(MI_OPTION_ARENA_EAGER_COMMIT),
                    mi_option_get(MI_OPTION_RESERVE_OS_MEMORY),
                )
            }
        };

        let arena_mib = kib_to_mib(arena_effective);
        let reserve_os_mib = kib_to_mib(reserve_os_effective);
        let total_mib = total.map_or(0, |bytes| bytes / MIB);

        // Startup diagnostics: stderr is the only reliable channel this early
        // (tracing may not be initialised yet). `eager_commit` here is the
        // read-back of `arena_eager_commit` (ordinal 4) — the effective knob.
        eprintln!(
            "pingwaf: allocator purge_delay={purge_effective}ms arena_reserve={arena_mib}MiB \
             eager_commit={eager_effective} reserve_os_memory={reserve_os_mib}MiB \
             (total={total_mib}MiB, source={source})"
        );

        if total.is_none_or(|bytes| bytes < 2 * 1024 * MIB) {
            eprintln!(
                "pingwaf: allocator WARN memory ceiling {total_mib}MiB is below the 2 GiB \
                 minimum; pin MIMALLOC_ARENA_RESERVE in the process environment (e.g. \
                 MIMALLOC_ARENA_RESERVE=67108864) — mi_option_set cannot shrink an arena \
                 that already exists"
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard};

    use super::*;

    /// `std::env` is process-global and `set_var`/`remove_var` are `unsafe` in
    /// edition 2024 (a concurrent reader is UB), so every test that touches the
    /// variables holds this lock. `serial_test` would be tidier, but the
    /// workspace does not carry it and one module does not justify adding a
    /// dependency.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    const ENV_KEYS: [&str; 4] = [
        "PINGWAF_MALLOC_ARENA_RESERVE_MB",
        "MIMALLOC_ARENA_RESERVE",
        "PINGWAF_MALLOC_EAGER_COMMIT",
        "MIMALLOC_ARENA_EAGER_COMMIT",
    ];

    fn env_lock() -> MutexGuard<'static, ()> {
        ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// SAFETY (both helpers): the caller holds ENV_LOCK, so no other test
    /// thread reads or writes the environment concurrently.
    fn clear_env() {
        for key in ENV_KEYS {
            unsafe { std::env::remove_var(key) };
        }
    }

    fn set_env(key: &str, value: &str) {
        unsafe { std::env::set_var(key, value) };
    }

    #[test]
    fn purge_delay_buckets() {
        // Unknown ceiling (probe failed) maps to the most aggressive bucket:
        // returning pages early is the safe side for OOM.
        assert_eq!(purge_delay_for(0), 0);
        assert_eq!(purge_delay_for(512 * MIB), 0);
        assert_eq!(purge_delay_for(1023 * MIB), 0);
        assert_eq!(purge_delay_for(1024 * MIB), 250);
        assert_eq!(purge_delay_for(2 * 1024 * MIB), 250);
        assert_eq!(purge_delay_for(4096 * MIB), 500);
        assert_eq!(purge_delay_for(16 * 1024 * MIB), 500);
        assert_eq!(purge_delay_for(17 * 1024 * MIB), 1000);
        assert_eq!(purge_delay_for(64 * 1024 * MIB), 1000);
    }

    #[test]
    fn purge_delay_is_monotonic() {
        let mut last = -1;
        for mib in [128_u64, 512, 1024, 2048, 4096, 8192, 16384, 32768] {
            let delay = purge_delay_for(mib * MIB);
            assert!(delay >= last, "delay must not shrink as memory grows");
            last = delay;
        }
    }

    #[test]
    fn arena_reserve_buckets() {
        assert_eq!(arena_reserve_for(Some(512 * MIB)), 64 * MIB);
        assert_eq!(arena_reserve_for(Some(1023 * MIB)), 64 * MIB);
        assert_eq!(arena_reserve_for(Some(1024 * MIB)), 256 * MIB);
        assert_eq!(arena_reserve_for(Some(2 * 1024 * MIB)), 256 * MIB);
        assert_eq!(arena_reserve_for(Some(4096 * MIB)), 512 * MIB);
        assert_eq!(arena_reserve_for(Some(8 * 1024 * MIB)), 512 * MIB);
        assert_eq!(arena_reserve_for(Some(16 * 1024 * MIB)), 512 * MIB);
        assert_eq!(arena_reserve_for(Some(17 * 1024 * MIB)), 1024 * MIB);
        // Probe failed: assume the smallest host, never the 1 GiB default.
        assert_eq!(arena_reserve_for(None), 64 * MIB);
    }

    #[test]
    fn arena_reserve_is_monotonic() {
        let mut last = 0;
        for mib in [128_u64, 512, 1024, 2048, 4096, 8192, 16384, 32768] {
            let reserve = arena_reserve_for(Some(mib * MIB));
            assert!(reserve >= last, "reserve must not shrink as memory grows");
            last = reserve;
        }
    }

    #[test]
    fn eager_commit_buckets() {
        assert!(!eager_commit_for(Some(1024 * MIB)));
        assert!(!eager_commit_for(Some(3 * 1024 * MIB)));
        assert!(eager_commit_for(Some(4096 * MIB)));
        assert!(eager_commit_for(Some(16 * 1024 * MIB)));
        // Unknown ceiling: do not commit a whole arena up front.
        assert!(!eager_commit_for(None));
    }

    #[test]
    fn arena_reserve_kib_conversion() {
        // mimalloc stores size options in KiB; the conversion is the whole
        // difference between a 64 MiB and a 64 GiB reservation.
        assert_eq!(bytes_to_kib(64 * MIB), 65536);
        assert_eq!(kib_to_mib(65536), 64);
        assert_eq!(kib_to_mib(0), 0);
        assert_eq!(kib_to_mib(-1), 0);
    }

    #[test]
    fn mimalloc_size_parsing() {
        // Bare numbers are bytes, exactly like mimalloc's own parser, so
        // `MIMALLOC_ARENA_RESERVE=67108864` means 64 MiB and not 64 GiB.
        assert_eq!(parse_mimalloc_size("67108864"), Some(64 * MIB));
        assert_eq!(parse_mimalloc_size("64M"), Some(64 * MIB));
        assert_eq!(parse_mimalloc_size("64MiB"), Some(64 * MIB));
        assert_eq!(parse_mimalloc_size("65536KiB"), Some(64 * MIB));
        assert_eq!(parse_mimalloc_size("1G"), Some(1024 * MIB));
        assert_eq!(parse_mimalloc_size("4096B"), Some(4096));
        assert_eq!(parse_mimalloc_size("0"), None);
        assert_eq!(parse_mimalloc_size("abc"), None);
        assert_eq!(parse_mimalloc_size(""), None);
    }

    #[test]
    fn override_parsing() {
        let _guard = env_lock();
        clear_env();

        // Adaptive when nothing is set.
        assert_eq!(arena_reserve_override(), None);
        assert_eq!(eager_commit_override(), None);

        // PINGWAF_* is in MiB and wins.
        set_env("PINGWAF_MALLOC_ARENA_RESERVE_MB", "32");
        assert_eq!(
            arena_reserve_override(),
            Some((32 * MIB, "env:PINGWAF_MALLOC_ARENA_RESERVE_MB"))
        );
        set_env("MIMALLOC_ARENA_RESERVE", "67108864");
        assert_eq!(
            arena_reserve_override(),
            Some((32 * MIB, "env:PINGWAF_MALLOC_ARENA_RESERVE_MB")),
            "PINGWAF_* must outrank MIMALLOC_*"
        );
        clear_env();

        // Unparsable / zero PINGWAF value is ignored, then the standard
        // mimalloc variable is used as the fallback.
        set_env("PINGWAF_MALLOC_ARENA_RESERVE_MB", "abc");
        assert_eq!(arena_reserve_override(), None);
        set_env("MIMALLOC_ARENA_RESERVE", "64M");
        assert_eq!(
            arena_reserve_override(),
            Some((64 * MIB, "env:MIMALLOC_ARENA_RESERVE"))
        );
        clear_env();

        set_env("PINGWAF_MALLOC_ARENA_RESERVE_MB", "0");
        assert_eq!(arena_reserve_override(), None, "0 means 'no arena'");
        clear_env();

        // Eager commit accepts 0/1 only.
        set_env("PINGWAF_MALLOC_EAGER_COMMIT", "1");
        assert_eq!(
            eager_commit_override(),
            Some((true, "env:PINGWAF_MALLOC_EAGER_COMMIT"))
        );
        set_env("PINGWAF_MALLOC_EAGER_COMMIT", "0");
        assert_eq!(
            eager_commit_override(),
            Some((false, "env:PINGWAF_MALLOC_EAGER_COMMIT"))
        );
        set_env("PINGWAF_MALLOC_EAGER_COMMIT", "yes");
        assert_eq!(eager_commit_override(), None);
        clear_env();

        set_env("MIMALLOC_ARENA_EAGER_COMMIT", "0");
        assert_eq!(
            eager_commit_override(),
            Some((false, "env:MIMALLOC_ARENA_EAGER_COMMIT"))
        );
        // 2 is mimalloc's "eager only on overcommit" default: not an override.
        set_env("MIMALLOC_ARENA_EAGER_COMMIT", "2");
        assert_eq!(eager_commit_override(), None);
        clear_env();

        // Nothing left behind for the other tests.
        assert_eq!(arena_reserve_override(), None);
        assert_eq!(eager_commit_override(), None);
    }

    #[test]
    fn source_label_variants() {
        assert_eq!(source_label([ADAPTIVE; 3]), "adaptive");
        assert_eq!(
            source_label([ADAPTIVE, "env:MIMALLOC_ARENA_RESERVE", ADAPTIVE]),
            "env:MIMALLOC_ARENA_RESERVE"
        );
        assert_eq!(
            source_label([
                "env:PINGWAF_MALLOC_PURGE_DELAY_MS",
                "env:PINGWAF_MALLOC_ARENA_RESERVE_MB",
                ADAPTIVE
            ]),
            "env:PINGWAF_MALLOC_PURGE_DELAY_MS+env:PINGWAF_MALLOC_ARENA_RESERVE_MB"
        );
    }
}
