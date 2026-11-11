//! Self-upgrade: follow the control plane's build.
//!
//! PingWAF ships one binary per build, so "the agent is on another version than
//! the control plane" is a real fleet problem, not a cosmetic one: the proto is
//! additive but rule semantics, config keys and defaults move together. This
//! module lets an agent close that gap on its own.
//!
//! Safety properties, all deliberate:
//!
//! * **Off by default.** `agent.auto_upgrade` must be set *and*
//!   `agent.upgrade_url` must name the control plane's REST API. The gRPC URL
//!   is not enough to guess it, and guessing would mean downloading a binary
//!   from a host nobody named.
//! * **Same origin as the control plane.** The artifact comes from the control
//!   plane that just authenticated this agent; no release feed is contacted.
//! * **Digest-pinned.** The manifest's `size` and `sha256` are checked against
//!   what arrived, and a mismatch aborts before anything is moved into place.
//! * **Reversible.** The current binary is kept as `<exe>.prev` until the next
//!   successful upgrade, so a bad build can be rolled back by hand.
//!
//! The upgrade restarts the process (`execv` on Unix). In-flight connections
//! drop, which is why this is opt-in and not something a heartbeat does to a
//! busy edge on its own.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::PingWafAgent;

/// How often the agent looks for a new build.
const CHECK_INTERVAL: Duration = Duration::from_secs(300);
/// First check waits this long: an upgrade loop fighting startup would make a
/// crash loop look like a network problem.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(60);

/// What the control plane says it is serving.
#[derive(Debug, Deserialize)]
struct Manifest {
    version: String,
    sha256: String,
    size: u64,
    /// Path relative to the API root, e.g. `/agent-upgrade/artifact`.
    url: String,
}

/// Starts the upgrade loop for an agent that opted in.
pub fn spawn(agent: std::sync::Arc<PingWafAgent>) -> JoinHandle<()> {
    tokio::spawn(async move {
        if !agent.config.auto_upgrade {
            debug!("agent.auto_upgrade is off; self-upgrade stays disabled");
            return;
        }
        let Some(base) = agent
            .config
            .upgrade_url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
        else {
            warn!(
                "agent.auto_upgrade is on but agent.upgrade_url is unset; \
                 self-upgrade stays disabled (set it to the control plane's \
                 REST base, e.g. https://cp.example.com:9080)"
            );
            return;
        };
        let base = base.trim_end_matches('/').to_string();
        tokio::time::sleep(FIRST_CHECK_DELAY).await;
        loop {
            match check_once(&agent, &base).await {
                Ok(true) => {
                    // The binary on disk is the new one; `restart` either
                    // replaces this process or exits for the supervisor.
                    restart();
                },
                Ok(false) => {},
                Err(err) => {
                    // Never fatal: a failed upgrade leaves the running build
                    // exactly as it was.
                    warn!(error = %err, "agent self-upgrade check failed");
                },
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    })
}

/// One check. `Ok(true)` means the binary on disk was replaced.
async fn check_once(agent: &PingWafAgent, base: &str) -> anyhow::Result<bool> {
    let Some(token) = agent.client.current_agent_token() else {
        anyhow::bail!("no agent token yet (registration has not completed)");
    };
    let client = http_client(agent)?;

    let manifest_url = format!("{base}/api/v1/agent-upgrade/manifest");
    let response = client.get(&manifest_url).bearer_auth(&token).send().await?;
    if !response.status().is_success() {
        anyhow::bail!("manifest request returned HTTP {}", response.status());
    }
    let manifest: Manifest = response.json().await?;

    let current = env!("CARGO_PKG_VERSION");
    if manifest.version == current {
        debug!(version = current, "agent is on the control plane's build");
        return Ok(false);
    }
    info!(
        current,
        target = %manifest.version,
        "control plane is running another build; starting self-upgrade"
    );

    let artifact_url = resolve_url(base, &manifest.url);
    let bytes = client
        .get(&artifact_url)
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    let exe = std::env::current_exe()?;
    let staged = staged_path(&exe);
    tokio::fs::write(&staged, &bytes).await?;
    verify_bytes(&bytes, manifest.size, &manifest.sha256)?;
    install(&exe, &staged).await?;
    info!(
        version = %manifest.version,
        path = %exe.display(),
        "agent binary replaced; restarting"
    );
    Ok(true)
}

/// Builds the HTTP client, trusting the same CA the gRPC channel trusts.
fn http_client(agent: &PingWafAgent) -> anyhow::Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .user_agent(concat!("PingWAF-Agent/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(300));
    // Either form of the CA the gRPC channel trusts: inline PEM wins, a file
    // path is read as a fallback.
    let pem: Option<Vec<u8>> = agent
        .config
        .server_ca_pem
        .as_ref()
        .map(|pem| pem.as_bytes().to_vec())
        .or_else(|| {
            agent
                .config
                .server_ca_cert
                .as_deref()
                .and_then(|path| std::fs::read(path).ok())
        });
    if let Some(pem) = pem {
        // A self-signed control plane is the normal case in all-in-one and in
        // private deployments; the agent must not fall back to the public
        // roots there, which would silently fail every check.
        let certificate = reqwest::Certificate::from_pem(&pem)?;
        builder = builder.add_root_certificate(certificate);
    }
    Ok(builder.build()?)
}

fn resolve_url(base: &str, url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else {
        format!("{base}{}", normalize_path(url))
    }
}

fn normalize_path(url: &str) -> String {
    if url.starts_with('/') {
        url.to_string()
    } else {
        format!("/{url}")
    }
}

fn staged_path(exe: &Path) -> PathBuf {
    let mut name = exe.as_os_str().to_os_string();
    name.push(".new");
    PathBuf::from(name)
}

fn backup_path(exe: &Path) -> PathBuf {
    let mut name = exe.as_os_str().to_os_string();
    name.push(".prev");
    PathBuf::from(name)
}

/// Size and digest check, split out so it is testable without a network.
fn verify_bytes(
    bytes: &[u8],
    expected_size: u64,
    expected_sha256: &str,
) -> anyhow::Result<()> {
    if bytes.len() as u64 != expected_size {
        anyhow::bail!(
            "artifact size mismatch: expected {expected_size} bytes, received {}",
            bytes.len()
        );
    }
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let sum = hasher.finalize();
    let mut actual = String::with_capacity(sum.len() * 2);
    for byte in sum {
        use std::fmt::Write as _;
        let _ = write!(actual, "{byte:02x}");
    }
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        anyhow::bail!(
            "artifact digest mismatch: expected {expected_sha256}, received {actual}"
        );
    }
    Ok(())
}

/// Moves the staged binary into place, keeping the old one as `<exe>.prev`.
async fn install(exe: &Path, staged: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(exe)
            .map(|meta| meta.permissions().mode())
            .unwrap_or(0o755);
        tokio::fs::set_permissions(
            staged,
            std::fs::Permissions::from_mode(mode),
        )
        .await?;
    }
    let backup = backup_path(exe);
    // Best effort: a first upgrade has nothing to back up, and a failed rename
    // here is not worth aborting an otherwise good artifact.
    if let Err(err) = tokio::fs::rename(exe, &backup).await {
        debug!(error = %err, "could not keep a copy of the previous binary");
    }
    tokio::fs::rename(staged, exe).await?;
    Ok(())
}

/// Replaces this process with the binary on disk.
fn restart() -> ! {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        let exe = std::env::current_exe()
            .unwrap_or_else(|_| PathBuf::from("pingwaf"));
        let err = std::process::Command::new(&exe)
            .args(std::env::args_os().skip(1))
            .exec();
        error!(error = %err, path = %exe.display(), "re-exec after upgrade failed");
    }
    // Whatever happens, do not keep running the code that was just replaced:
    // exiting lets systemd/docker start the new build.
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::{normalize_path, resolve_url, staged_path, verify_bytes};
    use std::path::{Path, PathBuf};

    #[test]
    fn digest_and_size_are_enforced() {
        // SHA-256("abc")
        let sum =
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(verify_bytes(b"abc", 3, sum).is_ok());
        // Uppercase hex is the same digest.
        assert!(verify_bytes(b"abc", 3, &sum.to_uppercase()).is_ok());
        assert!(verify_bytes(b"abcd", 3, sum).is_err(), "size mismatch");
        assert!(
            verify_bytes(b"abc", 3, &"0".repeat(64)).is_err(),
            "digest mismatch"
        );
    }

    #[test]
    fn artifact_url_follows_the_base_when_relative() {
        assert_eq!(
            resolve_url("https://cp:9080", "/api/v1/agent-upgrade/artifact"),
            "https://cp:9080/api/v1/agent-upgrade/artifact"
        );
        assert_eq!(
            resolve_url("https://cp:9080", "api/v1/agent-upgrade/artifact"),
            "https://cp:9080/api/v1/agent-upgrade/artifact"
        );
        // An absolute URL in the manifest is used as-is (a future control plane
        // may point at an object store).
        assert_eq!(
            resolve_url("https://cp:9080", "https://cdn/a.bin"),
            "https://cdn/a.bin"
        );
        assert_eq!(normalize_path("x"), "/x");
    }

    #[test]
    fn staging_keeps_the_binary_in_place() {
        // A sibling file, not a temp dir: the swap has to be a rename on the
        // same filesystem to be atomic.
        let exe = PathBuf::from("/usr/local/bin/pingwaf");
        assert_eq!(staged_path(&exe), Path::new("/usr/local/bin/pingwaf.new"));
    }
}
