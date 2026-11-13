//! `GET /api/v1/agent-upgrade/{manifest,artifact}` — self-upgrade for agents.
//!
//! PingWAF ships **one** binary that picks its role from `PINGWAF_MODE`, so the
//! artifact an agent should upgrade to is simply the control plane's own
//! executable. Serving that file means an autonomous upgrade follows the build
//! that is actually answering, with no release feed to trust and nothing to
//! configure beyond the URL of this API.
//!
//! Both endpoints require an agent token (`Authorization: Bearer …`) verified
//! the same way the gRPC stream verifies it, so only a registered agent can
//! download a binary. The digest is computed once over the running executable
//! and appears in the manifest *and* in a response header, so an agent can pin
//! what it actually received against what it was told to expect.

use std::io::Read;
use std::path::PathBuf;
use std::sync::OnceLock;

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use sea_orm::EntityTrait;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

use crate::api::error::ApiError;
use crate::api::keys::key_allows_agent;
use crate::api::state::AppState;
use crate::auth::jwt::verify_agent_token;
use crate::models::{agent, api_key};

/// Routes contributed to `/api/v1`.
///
/// The prefix is `agent-upgrade`, not `agents/upgrade`, so the path can never
/// be mistaken for the `/agents/{agent_id}` inventory route.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/agent-upgrade/manifest", get(manifest))
        .route("/agent-upgrade/artifact", get(artifact))
}

#[derive(Debug, Serialize)]
pub struct UpgradeManifest {
    /// Version this build reports; an agent compares it with its own.
    pub version: &'static str,
    /// SHA-256 of the artifact, lowercase hex.
    pub sha256: String,
    pub size: u64,
    /// Path (relative to the API root) the artifact is served from.
    pub url: String,
}

/// The running executable, measured once.
struct Artifact {
    path: PathBuf,
    size: u64,
    sha256: String,
}

fn artifact_info() -> Result<&'static Artifact, ApiError> {
    static INFO: OnceLock<Result<Artifact, String>> = OnceLock::new();
    INFO.get_or_init(|| {
        measure()
            .map_err(|err| format!("cannot read the running executable: {err}"))
    })
    .as_ref()
    .map_err(ApiError::internal)
}

fn measure() -> std::io::Result<Artifact> {
    let path = std::env::current_exe()?;
    let mut file = std::fs::File::open(&path)?;
    let (size, sha256) = digest(&mut file)?;
    Ok(Artifact { path, size, sha256 })
}

/// Size and lowercase SHA-256 of everything the reader yields.
///
/// Split out so the digest can be tested against a known value without an
/// executable on disk.
fn digest(reader: &mut impl Read) -> std::io::Result<(u64, String)> {
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size += read as u64;
    }
    let sum = hasher.finalize();
    let mut hex = String::with_capacity(sum.len() * 2);
    for byte in sum {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    Ok((size, hex))
}

/// Resolves the calling agent from its bearer token.
///
/// Reuses the gRPC verification path: the token must be one this control plane
/// minted *and* the agent must still exist, so a revoked agent cannot come back
/// for a binary after its row is gone.
async fn authorize(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(), ApiError> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApiError::Unauthorized("an agent token is required".to_string())
        })?;
    let claims =
        verify_agent_token(token, &state.config.jwt_secret).map_err(|_| {
            ApiError::Unauthorized("invalid agent token".to_string())
        })?;
    let agent_id = claims.subject_id().map_err(|_| {
        ApiError::Unauthorized("invalid agent token".to_string())
    })?;
    // The upgrade artifact is a binary: re-check the key on every download,
    // not just that the agent row exists. Deleting the key must revoke the
    // upgrade channel immediately (2026-11-13 audit, server-grpc-control-02).
    let row = agent::Entity::find_by_id(agent_id)
        .one(&state.db)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| {
            ApiError::Unauthorized("agent is no longer registered".to_string())
        })?;
    let Some(key_id) = row.api_key_id else {
        return Err(ApiError::Unauthorized(
            "the agent API key was revoked".to_string(),
        ));
    };
    let key = api_key::Entity::find_by_id(key_id)
        .one(&state.db)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| {
            ApiError::Unauthorized("the agent API key was revoked".to_string())
        })?;
    if !key_allows_agent(&key) {
        return Err(ApiError::Forbidden(
            "this API key may no longer act as an agent".to_string(),
        ));
    }
    Ok(())
}

async fn manifest(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    authorize(&state, &headers).await?;
    let info = artifact_info()?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(UpgradeManifest {
            version: env!("CARGO_PKG_VERSION"),
            sha256: info.sha256.clone(),
            size: info.size,
            url: "/api/v1/agent-upgrade/artifact".to_string(),
        }),
    ))
}

async fn artifact(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize(&state, &headers).await?;
    let info = artifact_info()?;
    let file = tokio::fs::File::open(&info.path)
        .await
        .map_err(ApiError::internal_generic)?;

    // Chunked rather than read-into-memory: the binary is tens of megabytes and
    // several agents may upgrade at once.
    let stream = futures_util::stream::unfold(Some(file), |state| async move {
        let mut file = state?;
        let mut buffer = vec![0u8; 64 * 1024];
        match file.read(&mut buffer).await {
            Ok(0) => None,
            Ok(read) => {
                buffer.truncate(read);
                Some((Ok::<Vec<u8>, std::io::Error>(buffer), Some(file)))
            },
            Err(err) => Some((Err(err), None)),
        }
    });

    let mut response = Response::new(Body::from_stream(stream));
    let outgoing = response.headers_mut();
    outgoing.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    outgoing.insert(header::CONTENT_LENGTH, HeaderValue::from(info.size));
    outgoing.insert(
        "x-pingwaf-version",
        HeaderValue::from_static(env!("CARGO_PKG_VERSION")),
    );
    if let Ok(value) = HeaderValue::from_str(&info.sha256) {
        outgoing.insert("x-pingwaf-sha256", value);
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::digest;

    /// SHA-256("abc"), the FIPS 180-2 test vector: a digest implementation that
    /// silently changed would keep every upgrade failing a check with no
    /// explanation, so pin one known value.
    #[test]
    fn digest_matches_a_known_vector() {
        let (size, sum) = digest(&mut &b"abc"[..]).expect("digest");
        assert_eq!(size, 3);
        assert_eq!(
            sum,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn digest_of_empty_input_is_the_empty_sum() {
        let (size, sum) = digest(&mut &b""[..]).expect("digest");
        assert_eq!(size, 0);
        assert_eq!(
            sum,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
