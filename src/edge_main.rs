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

//! Entry point of the agent-only `pingwaf-agent` binary.
//!
//! This is the data plane on its own: the rule cache, the gRPC client, the
//! heartbeat/log shipping and the pingap proxy that enforces the synced rules.
//! It deliberately does **not** declare `admin` (the PostgreSQL maintenance
//! commands) or the pingap admin plugin (which embeds the dashboard), and the
//! server-only branches of `cli`/`pingwaf` are compiled out by the `edge`
//! feature. The release pipeline builds it as a separate archive so an edge
//! node never downloads or maps the control plane's code and assets.
//!
//! The binary accepts both `pingwaf-agent agent ...` (the spelling install.sh
//! and the systemd unit use, and what a self-upgraded all-in-one binary still
//! understands) and `pingwaf-agent ...` with the flags directly.

// This binary compiles a subset of the shared runtime modules (process, the
// webhook notifier, the full CLI, ...) whose remaining items are used only by
// the control-plane/all-in-one binary. They are deliberately dead here rather
// than split into a crate whose only purpose would be to hide them.
#![allow(dead_code)]

#[cfg(all(feature = "openssl", feature = "tls-rustls"))]
compile_error!(
    "the `openssl` and `tls-rustls` features are mutually exclusive; build rustls with `--no-default-features --features tls-rustls,edge`"
);
#[cfg(not(any(feature = "openssl", feature = "tls-rustls")))]
compile_error!(
    "pingwaf-agent needs a TLS backend: enable `openssl` (default) or build with `--no-default-features --features tls-rustls,edge`"
);

#[cfg(not(feature = "dhat"))]
mod allocator_tune;
// The image optimizer registers itself through a `#[ctor]` in
// `pingap-imageoptim`; without a reference the linker drops the crate and the
// agent would accept a site policy it cannot honour.
#[cfg(feature = "imageoptim")]
#[allow(unused_imports)]
use pingap_imageoptim::ImageOptim;

mod certificates;
mod cli;
mod config_file;
mod config_manager;
mod locations;
mod pingwaf;
mod plugin;
mod process;
mod server_locations;
mod upgrade;
mod upstreams;
mod version;
mod webhook;

pub(crate) use version::version_banner;

// Default allocator: mimalloc on every platform (same rationale as main.rs —
// glibc's per-thread arenas are never trimmed, which turned the startup peak
// into ~620 MB of resident RSS in the 2026-10-15 demo post-mortem).
#[cfg(not(feature = "dhat"))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg(feature = "dhat")]
#[global_allocator]
static GLOBAL: dhat::Alloc = dhat::Alloc;

fn main() {
    #[cfg(feature = "dhat")]
    let _dhat = dhat::Profiler::builder()
        .file_name("dhat-heap.json")
        .build();

    // The graph enables both rustls crypto providers (ring and aws-lc-rs), so
    // install ring explicitly before any TLS client initialises; this is the
    // same provider the pingora rustls backend uses. No-op if already set.
    let _ = rustls::crypto::ring::default_provider().install_default();

    #[cfg(not(feature = "dhat"))]
    allocator_tune::apply();

    pingwaf::edge_main();
}
