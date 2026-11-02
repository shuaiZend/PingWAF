//! SSL/TLS certificate management: upload, ACME issuance, renewal and settings.

use std::collections::{HashMap, HashSet};

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
use axum::Router;
use chrono::{DateTime, Utc};
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, EntityTrait,
    FromQueryResult, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect,
    Select, Set, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::common::{
    ensure_acme_wildcard_supported, load_site_read, load_site_write, non_empty,
    parse_uuid, scope_site, Page, Pagination,
};
use crate::api::error::ApiError;
use crate::api::sites::{
    load_tls_posture, save_tls_posture, touch_site, TlsPosture,
    TlsPostureRequest,
};
use crate::api::state::AppState;
use crate::api::validation::{validate_certificate_and_key, validate_pem_pair};
use crate::auth::{AdminUser, AuthUser};
use crate::grpc::notify_config_changed;
use crate::models::{
    acme_challenge, certificate_events, certificate_links, site,
    site_certificates, site_ssl,
};

/// Certificate status values.
pub(crate) mod cert_status {
    pub const ACTIVE: &str = "active";
    pub const PENDING: &str = "pending";
    pub const EXPIRED: &str = "expired";
    pub const FAILED: &str = "failed";

    pub fn is_valid(status: &str) -> bool {
        matches!(status, ACTIVE | PENDING | EXPIRED | FAILED)
    }
}

/// Columns the certificate list endpoints serialize, without the PEM blobs.
/// A page of 50 certificates would otherwise load 50 private keys only to
/// compute two booleans; the presence flags are derived in SQL.
#[derive(Debug, FromQueryResult)]
struct CertificateListRow {
    id: Uuid,
    site_id: Option<Uuid>,
    domain: String,
    issuer: Option<String>,
    not_before: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
    auto_renew: bool,
    acme_email: Option<String>,
    acme_challenge_type: String,
    acme_dns_provider: Option<String>,
    acme_dns_config: Option<serde_json::Value>,
    status: String,
    failure_reason: Option<String>,
    has_certificate: bool,
    has_private_key: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<CertificateListRow> for CertificateResponse {
    fn from(row: CertificateListRow) -> Self {
        Self {
            id: row.id,
            site_id: row.site_id,
            domain: row.domain,
            issuer: row.issuer,
            not_before: row.not_before,
            expires_at: row.expires_at,
            auto_renew: row.auto_renew,
            acme_email: row.acme_email,
            acme_challenge_type: row.acme_challenge_type,
            acme_dns_provider: row.acme_dns_provider,
            acme_dns_config: row.acme_dns_config,
            status: row.status,
            relation: owner_relation(),
            bound_sites: Vec::new(),
            bound_count: 0,
            failure_reason: row.failure_reason,
            has_certificate: row.has_certificate,
            has_private_key: row.has_private_key,
            warnings: None,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// The projection shared by both certificate list endpoints.
fn certificate_list_query() -> Select<site_certificates::Entity> {
    site_certificates::Entity::find()
        .select_only()
        .columns([
            site_certificates::Column::Id,
            site_certificates::Column::SiteId,
            site_certificates::Column::Domain,
            site_certificates::Column::Issuer,
            site_certificates::Column::NotBefore,
            site_certificates::Column::ExpiresAt,
            site_certificates::Column::AutoRenew,
            site_certificates::Column::AcmeEmail,
            site_certificates::Column::AcmeChallengeType,
            site_certificates::Column::AcmeDnsProvider,
            site_certificates::Column::AcmeDnsConfig,
            site_certificates::Column::Status,
            site_certificates::Column::FailureReason,
            site_certificates::Column::CreatedAt,
            site_certificates::Column::UpdatedAt,
        ])
        .column_as(
            Expr::cust("cert_pem IS NOT NULL AND cert_pem <> ''"),
            "has_certificate",
        )
        .column_as(
            Expr::cust("key_pem IS NOT NULL AND key_pem <> ''"),
            "has_private_key",
        )
}

/// Public certificate representation (never exposes the private key).
#[derive(Debug, Serialize)]
pub struct CertificateResponse {
    pub id: Uuid,
    pub site_id: Option<Uuid>,
    pub domain: String,
    pub issuer: Option<String>,
    pub not_before: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub auto_renew: bool,
    pub acme_email: Option<String>,
    pub acme_challenge_type: String,
    pub acme_dns_provider: Option<String>,
    pub acme_dns_config: Option<serde_json::Value>,
    pub status: String,
    /// `owner` when this site owns the certificate row, `linked` when it uses
    /// one shared by another site. Lets the console group the picker.
    #[serde(default = "owner_relation")]
    pub relation: String,
    /// Sites that reach this certificate through `site_certificate_links`, i.e.
    /// the sharing grants. The owner is **not** synthesised into this list — it
    /// is `site_id` — and it is filtered out when a link row of its own exists:
    /// migration 85 backfilled one for every site that had already selected its
    /// certificate, so such a row repeats the owner relation instead of
    /// granting anything. The row stays in the table — the data plane still
    /// reads it when it assembles a site's selectable certificates, and a site
    /// cloning its own certificates copies it — only this read is filtered.
    ///
    /// Filled in by [`bound_sites_index`]: one `is_in` query per page, never one
    /// per row.
    #[serde(default)]
    pub bound_sites: Vec<BoundSiteRef>,
    /// `bound_sites.len()`, repeated so the console can render a count without
    /// walking the array.
    #[serde(default)]
    pub bound_count: usize,
    /// Why the last ACME issuance attempt was given up on; null while an
    /// order is healthy or after it succeeded.
    pub failure_reason: Option<String>,
    pub has_certificate: bool,
    pub has_private_key: bool,
    /// Advisory only: certificate hostnames this site does not serve. Filled
    /// in on every endpoint that returns a certificate — create/update, and
    /// the list/show reads too, so the dashboard badge renders on a page
    /// refresh rather than only right after a save (H3) — but never turned
    /// into a rejection: uploading the certificate before adding its hostname
    /// (or for another site) is a legitimate workflow.
    ///
    /// Empty (not null) when nothing is wrong; `None` only survives on a
    /// response built from a model without a site lookup.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn owner_relation() -> String {
    "owner".to_string()
}

/// One site a certificate is bound to, as the console shows it.
///
/// The domain travels with the id so a bound-site list renders without a second
/// round trip per certificate.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BoundSiteRef {
    pub id: Uuid,
    pub domain: String,
}

impl From<site_certificates::Model> for CertificateResponse {
    fn from(m: site_certificates::Model) -> Self {
        Self {
            id: m.id,
            site_id: m.site_id,
            domain: m.domain,
            issuer: m.issuer,
            not_before: m.not_before,
            expires_at: m.expires_at,
            auto_renew: m.auto_renew,
            acme_email: m.acme_email,
            acme_challenge_type: m.acme_challenge_type,
            acme_dns_provider: m.acme_dns_provider,
            acme_dns_config: m.acme_dns_config,
            status: m.status,
            relation: owner_relation(),
            // Filled in by `attach_bindings` on every read path; a freshly
            // built response (the create paths) has no links by construction.
            bound_sites: Vec::new(),
            bound_count: 0,
            failure_reason: m.failure_reason,
            has_certificate: m.cert_pem.is_some(),
            has_private_key: m.key_pem.is_some(),
            warnings: None,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

/// SSL settings response (from site_ssl or derived).
#[derive(Debug, Serialize)]
pub struct SslSettingsResponse {
    pub https_enabled: bool,
    pub min_tls_version: String,
    pub max_tls_version: Option<String>,
    pub self_signed: bool,
    pub certificate_id: Option<Uuid>,
    pub mtls_enabled: bool,
    pub has_mtls_client_ca: bool,
    pub mtls_organization: Option<String>,
    pub mtls_require_client_cert: bool,
    pub hsts_enabled: bool,
    pub hsts_max_age: i32,
    pub always_use_https: bool,
    pub http1_enabled: bool,
    pub http2_enabled: bool,
}

impl From<TlsPosture> for SslSettingsResponse {
    fn from(posture: TlsPosture) -> Self {
        Self {
            https_enabled: posture.https_enabled,
            min_tls_version: posture.min_tls_version,
            max_tls_version: posture.max_tls_version,
            self_signed: posture.self_signed,
            certificate_id: posture.certificate_id,
            mtls_enabled: posture.mtls_enabled,
            has_mtls_client_ca: posture.mtls_client_ca.is_some(),
            mtls_organization: posture.mtls_organization,
            mtls_require_client_cert: posture.mtls_require_client_cert,
            hsts_enabled: posture.hsts_enabled,
            hsts_max_age: posture.hsts_max_age,
            always_use_https: posture.always_use_https,
            http1_enabled: posture.http1_enabled,
            http2_enabled: posture.http2_enabled,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(flatten)]
    pub pagination: Pagination,
}

#[derive(Debug, Deserialize)]
pub struct CreateCertificateRequest {
    pub domain: String,
    #[serde(default)]
    pub cert_pem: Option<String>,
    #[serde(default)]
    pub key_pem: Option<String>,
    #[serde(default)]
    pub issuer: Option<String>,
    #[serde(default)]
    pub not_before: Option<DateTime<Utc>>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default = "default_true")]
    pub auto_renew: bool,
    #[serde(default)]
    pub acme_email: Option<String>,
    #[serde(default = "default_challenge_type")]
    pub acme_challenge_type: String,
    #[serde(default)]
    pub acme_dns_provider: Option<String>,
    #[serde(default)]
    pub acme_dns_config: Option<serde_json::Value>,
    /// Site the certificate is uploaded for, when it is not created through the
    /// site's own certificate list.
    #[serde(default)]
    pub site_id: Option<Uuid>,
    /// Attach the certificate to its site (and turn HTTPS on) right away.
    #[serde(default)]
    pub activate: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateCertificateRequest {
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub cert_pem: Option<String>,
    #[serde(default)]
    pub key_pem: Option<String>,
    #[serde(default)]
    pub issuer: Option<String>,
    #[serde(default)]
    pub not_before: Option<DateTime<Utc>>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub auto_renew: Option<bool>,
    #[serde(default)]
    pub acme_email: Option<String>,
    #[serde(default)]
    pub acme_challenge_type: Option<String>,
    #[serde(default)]
    pub acme_dns_provider: Option<String>,
    #[serde(default)]
    pub acme_dns_config: Option<serde_json::Value>,
    #[serde(default)]
    pub status: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_challenge_type() -> String {
    "http-01".to_string()
}

/// Splits the comma-separated hostname list a certificate row stores.
fn split_domains(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

/// True when the wildcard `cert` (`*.example.com`) matches the concrete
/// hostname `host` under the usual single-label rule: a certificate wildcard
/// covers exactly one label, never `a.b.example.com`. Both inputs are already
/// lower-cased.
fn wildcard_covers(cert: &str, host: &str) -> bool {
    let Some(parent) = cert.strip_prefix("*.") else {
        return false;
    };
    // The wildcard label itself never matches its own parent
    // (`*.example.com` does not cover `example.com`).
    let Some(label) = host.strip_suffix(parent).and_then(|head| {
        head.strip_suffix('.').filter(|label| !label.is_empty())
    }) else {
        return false;
    };
    // Exactly one label, and not a wildcard: `a.b.example.com` and
    // `*.example.com` are both outside a `*.example.com` certificate.
    !label.is_empty() && !label.contains('.') && !label.contains('*')
}

/// Multi-label wildcard suffix check, mirroring the data-plane
/// `lookup_with_wildcard` (pingwaf-agent cache): a site wildcard
/// `*.example.com` covers any depth of subdomain (`a.example.com`,
/// `a.b.example.com`) but not the parent `example.com` itself.
fn site_wildcard_covers(site: &str, cert: &str) -> bool {
    let Some(parent) = site.strip_prefix("*.") else {
        return false;
    };
    // The wildcard never covers its own parent (`*.example.com` does not
    // serve `example.com`), and only concrete hostnames match — not
    // another wildcard.
    let Some(head) =
        cert.strip_suffix(parent).and_then(|h| h.strip_suffix('.'))
    else {
        return false;
    };
    !head.is_empty() && !head.contains('*')
}

/// Whether `served` (a hostname the site answers for) is covered by the
/// certificate hostname `cert`. The two directions are not symmetric: a
/// wildcard on the certificate side covers any single label under it
/// (the TLS standard), while a wildcard on the site side uses multi-label
/// suffix matching, mirroring the agent's data-plane rule lookup — the
/// site serves `*.example.com`, so both `api.example.com` and
/// `a.b.example.com` are served (`example.com` itself is not).
fn cert_name_covers_site_name(cert: &str, served: &str) -> bool {
    cert == served
        || wildcard_covers(cert, served)
        || site_wildcard_covers(served, cert)
}

/// Certificate hostnames the site does not serve.
///
/// The control plane attaches a certificate to whatever hostnames it names,
/// but the site only answers for `site.domain` plus its `alternate_domains`.
/// A certificate carrying a hostname outside that set is almost always a
/// mistake — the wrong site, or a copy-pasted SAN — and used to be saved
/// silently. It is reported as a warning rather than rejected: naming a host
/// the operator is about to add, or one served through a separate site, is a
/// legitimate workflow, and taking HTTPS down over it would be worse.
///
/// Returns the uncovered certificate hostnames in certificate order with
/// duplicates removed (both sides are de-duplicated and lower-cased before
/// the comparison).
fn uncovered_certificate_domains(
    cert_domain: &str,
    site_domain: &str,
    alternate: &[String],
) -> Vec<String> {
    let served: Vec<String> = std::iter::once(site_domain.to_ascii_lowercase())
        .chain(alternate.iter().map(|domain| domain.to_ascii_lowercase()))
        .filter(|domain| !domain.is_empty())
        .collect();

    let mut uncovered: Vec<String> = Vec::new();
    for name in split_domains(cert_domain) {
        let name = name.to_ascii_lowercase();
        if served
            .iter()
            .any(|host| cert_name_covers_site_name(&name, host))
        {
            continue;
        }
        if !uncovered.iter().any(|existing| existing == &name) {
            uncovered.push(name);
        }
    }
    uncovered
}

/// Site hostnames the certificate does **not** cover — the direction that
/// decides whether a certificate can serve a site at all.
///
/// Deliberately not [`uncovered_certificate_domains`] with the arguments
/// swapped: that one reports *extra* certificate hostnames, which are only a
/// warning, while this one reports hostnames the site answers for but the
/// certificate cannot validate — the TLS handshake fails for exactly those, so a
/// bind request naming them is rejected. A certificate carrying SANs for other
/// sites is therefore accepted (its extra names warn), and a certificate issued
/// for a different domain is refused.
///
/// Returns the uncovered site hostnames in site order (primary first), with
/// duplicates removed and both sides lower-cased. An empty certificate hostname
/// list covers nothing and returns no verdict, so a malformed row cannot reject
/// a bind on its own.
///
/// Also used by the gRPC configuration builder to choose between several
/// certificates linked to one site: the shortest result wins, and a result as
/// long as the site's hostname count means the certificate covers nothing.
pub(crate) fn uncovered_site_hostnames(
    cert_domain: &str,
    site_domain: &str,
    alternate: &[String],
) -> Vec<String> {
    let cert_names: Vec<String> = split_domains(cert_domain)
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect();
    if cert_names.is_empty() {
        return Vec::new();
    }

    let mut uncovered: Vec<String> = Vec::new();
    let served = std::iter::once(site_domain.to_ascii_lowercase())
        .chain(alternate.iter().map(|domain| domain.to_ascii_lowercase()));
    for host in served.filter(|domain| !domain.is_empty()) {
        if cert_names
            .iter()
            .any(|name| cert_name_covers_site_name(name, &host))
        {
            continue;
        }
        if !uncovered.iter().any(|existing| existing == &host) {
            uncovered.push(host);
        }
    }
    uncovered
}

/// Builds the advisory warnings for a saved certificate, logging each one.
/// Never fails and never blocks the save.
fn certificate_warnings(
    site_id: Uuid,
    site: &site::Model,
    cert_domain: &str,
) -> Vec<String> {
    let warnings = uncovered_certificate_domains(
        cert_domain,
        &site.domain,
        &site.alternate_domains,
    );
    for domain in &warnings {
        tracing::warn!(
            site_id = %site_id,
            domain = %domain,
            "certificate covers hosts the site does not serve"
        );
    }
    warnings
        .iter()
        .map(|domain| {
            format!("certificate domain {domain} is not served by this site")
        })
        .collect()
}

/// The hostnames each site answers for, keyed by site id.
///
/// `(domain, alternate_domains)` is the whole of what
/// [`certificate_warnings_for`] needs, so a list endpoint can fetch it once
/// for the whole page instead of joining per row (H3 N+1). Sites that are
/// gone from the table simply have no entry, which reads as "serves nothing".
type SiteHostnames = HashMap<Uuid, (String, Vec<String>)>;

/// Loads [`SiteHostnames`] for the given site ids in **one** query.
///
/// A database error degrades to an empty map (i.e. "no warnings") rather than
/// propagating: these warnings are advisory, and failing a certificate listing
/// because the advisory decoration could not be computed would be worse than
/// showing the certificates without it.
async fn site_hostname_index(
    state: &AppState,
    site_ids: impl IntoIterator<Item = Uuid>,
) -> SiteHostnames {
    let ids: Vec<Uuid> = {
        let mut ids: Vec<Uuid> = site_ids.into_iter().collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    };
    if ids.is_empty() {
        return HashMap::new();
    }
    match site::Entity::find()
        .filter(site::Column::Id.is_in(ids))
        .all(&state.db)
        .await
    {
        Ok(rows) => rows
            .into_iter()
            .map(|model| (model.id, (model.domain, model.alternate_domains)))
            .collect(),
        Err(err) => {
            tracing::warn!(
                error = %err,
                "could not load site hostnames for certificate warnings"
            );
            HashMap::new()
        },
    }
}

/// Computes the advisory warnings for a certificate read back from the
/// database, using hostnames already loaded by [`site_hostname_index`].
///
/// Same wording as [`certificate_warnings`] minus the per-row `tracing::warn!`:
/// a *listing* of N already-saved certificates would otherwise log N WARN
/// lines on every dashboard refresh, drowning the one line that mattered when
/// the certificate was saved.
fn certificate_warnings_for(
    served: &SiteHostnames,
    site_id: Uuid,
    cert_domain: &str,
) -> Vec<String> {
    let Some((domain, alternate)) = served.get(&site_id) else {
        return Vec::new();
    };
    uncovered_certificate_domains(cert_domain, domain, alternate)
        .iter()
        .map(|domain| {
            format!("certificate domain {domain} is not served by this site")
        })
        .collect()
}

/// The sites each certificate is bound to, keyed by certificate id.
type BoundSites = HashMap<Uuid, Vec<BoundSiteRef>>;

/// True when a `site_certificate_links` row merely restates ownership: a site
/// that selects its own certificate got such a row from migration 85. Such a
/// row is not a share, so it must not inflate `bound_count` nor block a delete.
fn is_owner_self_link(link_site: Uuid, owner: Option<Uuid>) -> bool {
    owner == Some(link_site)
}

/// The owner site of `cert_id`, as recorded in the `owners` index a read path
/// builds for the certificates it just loaded: `None` for an unbound
/// certificate, and for an id the caller did not put in the index.
fn owner_of(
    owners: &HashMap<Uuid, Option<Uuid>>,
    cert_id: Uuid,
) -> Option<Uuid> {
    owners.get(&cert_id).copied().flatten()
}

/// Loads [`BoundSites`] for the given certificate ids in **two** queries
/// (links, then site domains) however many certificates a page holds.
///
/// One `is_in` over `site_certificate_links` answers "which sites use this
/// certificate" for the whole page, and the domains come from the same
/// [`site_hostname_index`] the warnings already need — so a page pays one extra
/// query, not one per row. A database error degrades to "nothing bound": this
/// decorates a listing, and the paths where the answer decides something
/// (delete, unbind) query the link table themselves.
///
/// `owners` maps each certificate id to its owner (`site_certificates.site_id`)
/// and is what makes the answer mean "shared with *other* sites".
/// `m20240101_000085_site_certificate_links` backfilled a link row for the
/// certificate every site had already selected, so an owner's own row is a
/// duplicate of the owner relation rather than a grant: counting it made a
/// certificate with no share report `bound_count: 1`, pointing at the owner
/// itself. The row stays in the table; the read filters it.
async fn bound_sites_index(
    state: &AppState,
    cert_ids: impl IntoIterator<Item = Uuid>,
    owners: &HashMap<Uuid, Option<Uuid>>,
) -> BoundSites {
    let ids: Vec<Uuid> = {
        let mut ids: Vec<Uuid> = cert_ids.into_iter().collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    };
    if ids.is_empty() {
        return HashMap::new();
    }
    let links = match certificate_links::Entity::find()
        .filter(certificate_links::Column::CertificateId.is_in(ids))
        .all(&state.db)
        .await
    {
        Ok(rows) => rows,
        Err(err) => {
            tracing::warn!(error = %err, "could not load certificate bindings");
            return HashMap::new();
        },
    };
    if links.is_empty() {
        return HashMap::new();
    }

    let mut cert_to_sites: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for link in &links {
        // The owner's own link row (migration 85 backfill) is the owner
        // relation written twice, not a share — see this function's docs.
        if owner_of(owners, link.certificate_id) == Some(link.site_id) {
            continue;
        }
        cert_to_sites
            .entry(link.certificate_id)
            .or_default()
            .push(link.site_id);
    }
    let sites =
        site_hostname_index(state, links.iter().map(|link| link.site_id)).await;

    let mut bound: BoundSites = HashMap::with_capacity(cert_to_sites.len());
    for (cert_id, mut site_ids) in cert_to_sites {
        // Deterministic order: the link table has no natural one, and a list
        // that reshuffles between two identical requests is noise in the
        // console and in any diff of two responses.
        site_ids.sort_unstable();
        let refs: Vec<BoundSiteRef> = site_ids
            .into_iter()
            .filter_map(|site_id| {
                // A site deleted between the two queries leaves a link row that
                // `ON DELETE CASCADE` already removed; skipping a stale id is
                // the same fail-soft as a missing warning entry.
                sites.get(&site_id).map(|(domain, _)| BoundSiteRef {
                    id: site_id,
                    domain: domain.clone(),
                })
            })
            .collect();
        if !refs.is_empty() {
            bound.insert(cert_id, refs);
        }
    }
    bound
}

/// Copies a certificate's bindings onto its response.
///
/// The owner is filtered out here as well as in [`bound_sites_index`]: this is
/// the one place the response fields are written, and the rule the console
/// depends on — `bound_count` counts *other* sites — must hold for a
/// [`BoundSites`] map built by anyone, not only by the query helper. The row
/// itself is left in the table (migration 85 backfilled the owner's own link
/// when the site had already selected the certificate; it repeats the owner
/// relation, it does not share anything).
fn attach_bindings(
    response: &mut CertificateResponse,
    bound: &BoundSites,
    owners: &HashMap<Uuid, Option<Uuid>>,
) {
    let owner = owner_of(owners, response.id);
    let sites: Vec<BoundSiteRef> = bound
        .get(&response.id)
        .into_iter()
        .flatten()
        .filter(|site| Some(site.id) != owner)
        .cloned()
        .collect();
    response.bound_count = sites.len();
    response.bound_sites = sites;
}

/// Decorates one certificate that was just read back from the database with
/// the two derived fields every write endpoint returns: the advisory
/// [`CertificateResponse::warnings`] and the [`BoundSiteRef`] list.
///
/// The two endpoints that grant a share — `POST /certificates/{id}/bindings`
/// and `POST /sites/{site_id}/certificate-links` — both call this, including on
/// their already-linked path, so a retried request answers with the same shape
/// as the first one instead of a narrower row. `relation` is deliberately *not*
/// set here: it describes the caller's point of view (the site it asked about)
/// and each endpoint decides it on its own.
///
/// Costs up to three reads — the owner's hostnames, the links, and the
/// hostnames of the linked sites — and no errors of its own: the helpers
/// degrade, because a decoration must never fail a write that already
/// committed.
async fn decorate_certificate(
    state: &AppState,
    certificate: site_certificates::Model,
) -> CertificateResponse {
    let served = site_hostname_index(
        state,
        std::iter::once(certificate.site_id).flatten(),
    )
    .await;
    // One map for one certificate: `bound_count` counts the *other* sites, so
    // the owner has to travel with the id or the owner's own backfilled link
    // row (migration 85) would be counted as a share.
    let owners = HashMap::from([(certificate.id, certificate.site_id)]);
    let bound =
        bound_sites_index(state, std::iter::once(certificate.id), &owners)
            .await;

    let mut response = CertificateResponse::from(certificate);
    response.warnings = Some(certificate_warnings_for(
        &served,
        response.site_id.unwrap_or_default(),
        &response.domain,
    ));
    attach_bindings(&mut response, &bound, &owners);
    response
}

/// Longest single hostname a certificate row may name. A DNS name is at most
/// 253 characters (RFC 1035), and one wildcard label still fits inside it.
const MAX_SINGLE_HOSTNAME_LEN: usize = 253;

/// Anti-abuse bound on the whole comma-separated list. The `domain` column is
/// TEXT — a certificate legitimately covering five hostnames already exceeds
/// the old 255-character limit (see
/// `m20240101_000021_site_alternate_domains`) — so the cap here only exists to
/// stop a caller parking an unbounded blob in the row.
const MAX_CERT_DOMAIN_LIST_LEN: usize = 4096;

/// Normalises and vets one hostname from a certificate's hostname list.
///
/// Returns the trimmed, lower-cased form; a single leading `*.` wildcard label
/// is allowed, anything else about the name must look like DNS labels.
fn validate_cert_hostname(raw: &str) -> Result<String, ApiError> {
    let domain = raw.trim().to_ascii_lowercase();
    let invalid = |reason: &str| {
        ApiError::BadRequest(format!("invalid domain: {reason}"))
    };

    if domain.is_empty() {
        return Err(invalid("must be 1-253 characters"));
    }
    if domain.len() > MAX_SINGLE_HOSTNAME_LEN {
        return Err(invalid("must be 1-253 characters"));
    }
    // Control characters and the separators that would make the value
    // ambiguous to whoever parses it as a host: `a/b` reads as a path and
    // `a:443` as host plus port. `,` is the list separator and is handled by
    // the caller, never inside one hostname.
    if domain.chars().any(|ch| {
        ch.is_ascii_control()
            || ch.is_whitespace()
            || matches!(ch, '/' | ':' | '\\' | '@' | '?' | '#' | ',')
    }) {
        return Err(invalid("contains a forbidden character"));
    }

    // One wildcard label, and only as the leftmost label.
    let rest = match domain.strip_prefix("*.") {
        Some(rest) => rest,
        None => domain.as_str(),
    };
    if rest.contains('*') {
        return Err(invalid("wildcard is only allowed as a leading '*.'"));
    }
    if rest.is_empty() {
        return Err(invalid("wildcard needs a parent domain"));
    }
    // Empty labels (`..`, a leading or trailing `.`) and over-long labels.
    for label in rest.split('.') {
        if label.is_empty() {
            return Err(invalid("empty label"));
        }
        if label.len() > 63 {
            return Err(invalid("label exceeds 63 characters"));
        }
    }

    Ok(domain)
}

/// Normalises and vets the `domain` a certificate row is stored under.
///
/// The column holds the comma-separated list of hostnames the certificate is
/// issued for — `a.com,b.com` is an existing, supported input, and five names
/// legitimately exceed 255 characters. Each segment is therefore validated as
/// one hostname and the whole list only carries an anti-abuse bound.
///
/// The value is not merely cosmetic: it names the hostnames the certificate is
/// issued for (the ACME order is built from it) and it is what the control
/// plane matches against a site's hostnames. A string carrying `/`, `:` or
/// control characters used to be stored verbatim, so a caller could park a
/// path, a port or a smuggled header inside a field the edge later parses as a
/// hostname.
///
/// Returns the normalised list: `,`-joined, lower-cased, no surrounding
/// spaces, input order preserved, duplicates dropped. An empty segment
/// (`a.com,,b.com`, a trailing `,`) is a bad request rather than something to
/// silently skip.
pub(crate) fn validate_cert_domain_list(raw: &str) -> Result<String, ApiError> {
    let invalid = |reason: &str| {
        ApiError::BadRequest(format!("invalid domain: {reason}"))
    };

    // Bound the input before splitting so a huge blob is refused cheaply.
    if raw.len() > MAX_CERT_DOMAIN_LIST_LEN {
        return Err(invalid("domain list exceeds 4096 bytes"));
    }
    if raw.trim().is_empty() {
        return Err(invalid("must name at least one hostname"));
    }

    let mut hostnames: Vec<String> = Vec::new();
    for segment in raw.split(',') {
        if segment.trim().is_empty() {
            return Err(invalid(
                "empty entry in the comma-separated domain list",
            ));
        }
        let hostname = validate_cert_hostname(segment)?;
        if !hostnames.iter().any(|existing| existing == &hostname) {
            hostnames.push(hostname);
        }
    }
    if hostnames.is_empty() {
        return Err(invalid("must name at least one hostname"));
    }

    let joined = hostnames.join(",");
    // Deduplication only shortens the list, so this can only fire on input
    // that was already over the bound; kept as a belt-and-braces check.
    if joined.len() > MAX_CERT_DOMAIN_LIST_LEN {
        return Err(invalid("domain list exceeds 4096 bytes"));
    }
    Ok(joined)
}

/// Routes contributed to `/api/v1`.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/sites/{site_id}/certificates",
            get(list_certificates).post(create_certificate),
        )
        .route(
            "/sites/{site_id}/certificates/{cert_id}",
            get(show_certificate)
                .put(update_certificate)
                .delete(delete_certificate),
        )
        .route(
            "/sites/{site_id}/certificates/{cert_id}/renew",
            axum::routing::post(renew_certificate),
        )
        .route(
            "/sites/{site_id}/ssl-settings",
            get(get_ssl_settings).put(update_ssl_settings),
        )
        // Certificate sharing: list/link/unlink a certificate another site
        // owns, plus the set this site may still link.
        .route(
            "/sites/{site_id}/certificate-links",
            get(list_certificate_links).post(link_certificate),
        )
        .route(
            "/sites/{site_id}/certificate-links/{cert_id}",
            axum::routing::delete(unlink_certificate),
        )
        .route(
            "/sites/{site_id}/certificates/available",
            get(list_available_certificates),
        )
        // Global aliases. The dashboard's SSL/TLS page spans sites: it lists
        // every application, uploads or renews a certificate for a chosen site,
        // and summarises what is about to lapse.
        .route(
            "/certificates",
            get(list_all_certificates).post(create_global_certificate),
        )
        .route("/certificates/summary", get(certificate_summary))
        .route(
            "/certificates/{cert_id}",
            get(show_global_certificate)
                .put(update_global_certificate)
                .delete(delete_global_certificate),
        )
        // Who owns, is shared with, and currently serves this certificate —
        // one read that lets the bind / unbind / delete dialogs name the
        // sites a `409` would otherwise only list as UUIDs.
        .route("/certificates/{cert_id}/usage", get(certificate_usage))
        .route(
            "/certificates/{cert_id}/renew",
            axum::routing::post(renew_global_certificate),
        )
        // Binding a certificate to a site, from the certificate's side: one
        // uploaded PEM can serve several sites and the upload no longer names
        // one. The site-scoped pair above stays the site's own view of the same
        // relation.
        .route(
            "/certificates/{cert_id}/bindings",
            axum::routing::post(bind_certificate),
        )
        .route(
            "/certificates/{cert_id}/bindings/{site_id}",
            axum::routing::delete(unbind_certificate),
        )
        .route(
            "/certificates/{cert_id}/events",
            get(list_certificate_events),
        )
        .route("/ssl-events", get(list_all_events))
}

/// `GET /api/v1/sites/{site_id}/certificates`
async fn list_certificates(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Page<CertificateResponse>>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    load_site_read(&state.db, id, &current).await?;
    let pagination = query.pagination.normalise();

    // Own certificates plus the ones shared with this site. A site can only
    // *select* what is visible here, so the link table has to be part of the
    // listing — otherwise sharing a certificate would not make it pickable.
    let linked_ids: Vec<Uuid> = certificate_links::Entity::find()
        .filter(certificate_links::Column::SiteId.eq(id))
        .all(&state.db)
        .await?
        .into_iter()
        .map(|link| link.certificate_id)
        .collect();
    let mut condition =
        Condition::any().add(site_certificates::Column::SiteId.eq(id));
    if !linked_ids.is_empty() {
        condition =
            condition.add(site_certificates::Column::Id.is_in(linked_ids));
    }

    let paginator = certificate_list_query()
        .filter(condition)
        .order_by_desc(site_certificates::Column::CreatedAt)
        .into_model::<CertificateListRow>()
        .paginate(&state.db, pagination.limit());

    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(pagination.index()).await?;
    // H3: the advisory warnings are only useful where the dashboard reads
    // certificates back, so the list fills them in too — one site lookup, then
    // pure string comparisons per row. A failed lookup degrades to "no
    // warnings" rather than failing the read: the list is a listing, and
    // holding it hostage to an advisory field would be the wrong trade.
    let served = site_hostname_index(&state, std::iter::once(id)).await;
    // One map for the whole page: `bound_count` counts the *other* sites, so
    // the owner of each row has to come along with the ids.
    let owners: HashMap<Uuid, Option<Uuid>> =
        rows.iter().map(|row| (row.id, row.site_id)).collect();
    let bound =
        bound_sites_index(&state, rows.iter().map(|row| row.id), &owners).await;
    let items: Vec<CertificateResponse> = rows
        .into_iter()
        .map(|row| {
            let owner = row.site_id;
            let mut response = CertificateResponse::from(row);
            // Owner first: the owner also has a `site_certificate_links` row
            // (backfilled for the selected certificate), but it is still the
            // owner — only genuinely foreign rows are `linked`.
            if owner == Some(id) {
                response.relation = "owner".to_string();
            } else {
                response.relation = "linked".to_string();
            }
            response.warnings = Some(certificate_warnings_for(
                &served,
                response.site_id.unwrap_or_default(),
                &response.domain,
            ));
            attach_bindings(&mut response, &bound, &owners);
            response
        })
        .collect();
    Ok(Json(Page::new(items, total, pagination)))
}

/// `GET /api/v1/sites/{site_id}/certificate-links`
///
/// Same page as the certificate list: a link is not a separate resource from
/// the console's point of view, it only changes where a row came from.
async fn list_certificate_links(
    state: State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Page<CertificateResponse>>, ApiError> {
    list_certificates(state, current, Path(site_id), Query(query)).await
}

#[derive(Debug, Deserialize)]
struct LinkCertificateRequest {
    certificate_id: Uuid,
}

/// `POST /api/v1/sites/{site_id}/certificate-links`
///
/// Grants this site the right to serve another site's certificate.
///
/// Idempotent, exactly like the certificate-side `POST
/// /certificates/{id}/bindings`: a link that already exists is the state the
/// caller asked for, not an error, so a duplicate answers `200` with the same
/// decorated row instead of a conflict — a retried request (a flaky network, a
/// double click, a retrying client) must not read as a failure. The one
/// conflict kept is **ownership**, checked first: a site cannot "link" the
/// certificate it already owns, because that grant would mean nothing.
async fn link_certificate(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
    Json(payload): Json<LinkCertificateRequest>,
) -> Result<Response, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    let site = load_site_write(&state.db, id, &current).await?;

    let certificate =
        site_certificates::Entity::find_by_id(payload.certificate_id)
            .one(&state.db)
            .await?
            .ok_or_else(|| {
                ApiError::NotFound(format!(
                    "certificate {} not found",
                    payload.certificate_id
                ))
            })?;
    if certificate.site_id == Some(site.id) {
        return Err(ApiError::Conflict(
            "certificate already belongs to this site".to_string(),
        ));
    }
    // Linking grants the right to serve another site's certificate, so the
    // caller must be able to read the owning site as well.
    let owner = certificate.site_id.ok_or_else(|| {
        ApiError::BadRequest("certificate is not bound to any site".to_string())
    })?;
    load_site_read(&state.db, owner, &current).await?;

    let exists = certificate_links::Entity::find()
        .filter(certificate_links::Column::SiteId.eq(site.id))
        .filter(certificate_links::Column::CertificateId.eq(certificate.id))
        .one(&state.db)
        .await?
        .is_some();
    if exists {
        // The grant is already in place: the target state is reached, so this
        // is the same success the certificate-side endpoint reports, not a
        // `409`. The certificate row loaded above is reused — the idempotent
        // path costs the three decoration reads below and never a second
        // `find_by_id`.
        let owned_here = certificate.site_id == Some(site.id);
        let mut response = decorate_certificate(&state, certificate).await;
        // Owner first, then the share — the same precedence the certificate
        // list applies. Ownership returned `409` above, so this branch only
        // ever reports `linked`; the rule stays explicit so the field cannot
        // drift if that check is ever relaxed.
        response.relation =
            if owned_here { "owner" } else { "linked" }.to_string();
        return Ok(Json(response).into_response());
    }

    certificate_links::ActiveModel {
        site_id: Set(site.id),
        certificate_id: Set(certificate.id),
        linked_at: Set(Utc::now()),
    }
    .insert(&state.db)
    .await?;

    // The site's bundle now carries the PEM, so its agents have to re-pull -
    // the same side effect the certificate-side bindings endpoint performs.
    // Without it a share created here would reach the edge only on the next
    // unrelated configuration change.
    touch_site(&state, site.id).await?;
    notify_config_changed(&state, site.id, Some(&current.email)).await;

    // Same decorated shape as the idempotent branch and as every other
    // certificate response, so a caller cannot tell the two paths apart.
    let mut response = decorate_certificate(&state, certificate).await;
    response.relation = "linked".to_string();
    Ok((StatusCode::CREATED, Json(response)).into_response())
}

/// `DELETE /api/v1/sites/{site_id}/certificate-links/{cert_id}`
async fn unlink_certificate(
    State(state): State<AppState>,
    current: AuthUser,
    Path((site_id, cert_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    let cert_id = parse_uuid(&cert_id, "certificate id")?;
    let site = load_site_write(&state.db, id, &current).await?;

    // Unlinking the selected certificate would silently drop TLS; require the
    // operator to clear or replace it first.
    let posture = site_ssl::Entity::find()
        .filter(site_ssl::Column::SiteId.eq(site.id))
        .one(&state.db)
        .await?;
    if posture.and_then(|row| row.certificate_id) == Some(cert_id) {
        return Err(ApiError::Conflict(
            "certificate is the site's current TLS certificate; clear or replace it first"
                .to_string(),
        ));
    }

    let result = certificate_links::Entity::delete_many()
        .filter(certificate_links::Column::SiteId.eq(site.id))
        .filter(certificate_links::Column::CertificateId.eq(cert_id))
        .exec(&state.db)
        .await?;
    if result.rows_affected == 0 {
        return Err(ApiError::NotFound(format!(
            "certificate link {cert_id} not found"
        )));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// The filter behind [`list_available_certificates`].
///
/// "Available" here means **linkable**: the certificates this site could still
/// add a `site_certificate_links` grant for. A certificate row has three
/// possible origins, and only the third belongs to this endpoint:
///
/// | origin | `site_id` | listed here | which page shows it instead |
/// |---|---|---|---|
/// | the site's own | `= site_id` | no | `GET /sites/{id}/certificates` |
/// | uploaded, not bound | `NULL` | no | `POST /certificates/{id}/bindings` |
/// | another visible site's | `= other` | yes, until linked | this endpoint |
///
/// The two exclusions are not stylistic:
///
/// - the site's own certificate cannot be linked — `POST
///   /sites/{id}/certificate-links` answers `409 certificate already belongs to
///   this site` — and it is already in the site's own list;
/// - an unbound certificate cannot be linked either (`400 certificate is not
///   bound to any site`); it is attached through the certificate-side
///   `POST /certificates/{id}/bindings`, which is what creates the link row in
///   the first place;
/// - a link row of this site *is* the grant, so listing that certificate again
///   would only offer a second "link" that can produce a duplicate.
///
/// `site_id IS NOT NULL` is written out even though both other predicates
/// already drop NULLs through SQL three-valued logic (`NULL <> x` and `NULL IN
/// (…)` are `NULL`, not `TRUE`). Leaving that implicit is how a later rewrite
/// of this condition re-admits every unbound certificate: it costs one clause
/// and is a no-op today.
fn available_certificate_condition(
    site_id: Uuid,
    visible_ids: Vec<Uuid>,
    linked_ids: Vec<Uuid>,
) -> Condition {
    let mut condition = Condition::all()
        .add(site_certificates::Column::SiteId.is_not_null())
        .add(site_certificates::Column::SiteId.ne(site_id))
        .add(site_certificates::Column::SiteId.is_in(visible_ids));
    if !linked_ids.is_empty() {
        condition =
            condition.add(site_certificates::Column::Id.is_not_in(linked_ids));
    }
    condition
}

/// `GET /api/v1/sites/{site_id}/certificates/available`
///
/// Certificates this site may link: owned by a site the caller can read, not
/// owned by this site and not linked yet — the "shared certificates" picker.
/// The filter and the reasoning behind each exclusion live in
/// [`available_certificate_condition`].
async fn list_available_certificates(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Page<CertificateResponse>>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    load_site_read(&state.db, id, &current).await?;
    let sites = visible_sites(&state, &current).await?;
    let pagination = query.pagination.normalise();

    let linked_ids: Vec<Uuid> = certificate_links::Entity::find()
        .filter(certificate_links::Column::SiteId.eq(id))
        .all(&state.db)
        .await?
        .into_iter()
        .map(|link| link.certificate_id)
        .collect();

    let visible_ids: Vec<Uuid> = sites.keys().copied().collect();
    let condition =
        available_certificate_condition(id, visible_ids, linked_ids);

    let paginator = certificate_list_query()
        .filter(condition)
        .order_by_desc(site_certificates::Column::CreatedAt)
        .into_model::<CertificateListRow>()
        .paginate(&state.db, pagination.limit());
    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(pagination.index()).await?;
    let served = site_hostname_index(
        &state,
        rows.iter()
            .filter_map(|row| row.site_id)
            .chain(std::iter::once(id)),
    )
    .await;
    let owners: HashMap<Uuid, Option<Uuid>> =
        rows.iter().map(|row| (row.id, row.site_id)).collect();
    let bound =
        bound_sites_index(&state, rows.iter().map(|row| row.id), &owners).await;
    let items: Vec<CertificateResponse> = rows
        .into_iter()
        .map(|row| {
            let mut response = CertificateResponse::from(row);
            response.relation = "available".to_string();
            response.warnings = Some(certificate_warnings_for(
                &served,
                response.site_id.unwrap_or_default(),
                &response.domain,
            ));
            attach_bindings(&mut response, &bound, &owners);
            response
        })
        .collect();
    Ok(Json(Page::new(items, total, pagination)))
}

/// `POST /api/v1/sites/{site_id}/certificates`
async fn create_certificate(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
    Json(payload): Json<CreateCertificateRequest>,
) -> Result<Response, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    let site = load_site_write(&state.db, id, &current).await?;

    let model = insert_certificate(&state.db, Some(&site), payload).await?;
    announce_certificate_created(&state, &model, Some(&current.email)).await;
    // Advisory only: a certificate naming hosts this site does not serve is
    // saved anyway, with the mismatch reported back to the operator.
    let warnings = certificate_warnings(site.id, &site, &model.domain);
    let mut response = CertificateResponse::from(model);
    response.warnings = Some(warnings);
    Ok((StatusCode::CREATED, Json(response)).into_response())
}

/// Validates and stores one certificate row.
///
/// Takes the connection explicitly so callers can run it inside a transaction:
/// `create_global_certificate` must not leave a certificate behind when the
/// attached TLS posture fails to validate. Side effects that assume the row is
/// committed — the audit event, the site touch and the agent push — belong to
/// [`announce_certificate_created`], run after the surrounding transaction
/// commits.
async fn insert_certificate<C: ConnectionTrait>(
    db: &C,
    site: Option<&site::Model>,
    payload: CreateCertificateRequest,
) -> Result<site_certificates::Model, ApiError> {
    let domain = validate_cert_domain_list(&payload.domain)?;
    if !acme_challenge::is_valid(&payload.acme_challenge_type) {
        return Err(ApiError::BadRequest(format!(
            "invalid acme_challenge_type '{}'",
            payload.acme_challenge_type
        )));
    }
    // An ACME order without an uploaded PEM is issued against the site's whole
    // hostname set, which a wildcard may not contain under http-01. Without a
    // site the whole request is refused further down instead: an order no site
    // carries can never be picked up by the edge.
    if payload.cert_pem.is_none() && payload.acme_email.is_some() {
        if let Some(site) = site {
            ensure_acme_wildcard_supported(
                site,
                &split_domains(&domain),
                Some(&payload.acme_challenge_type),
            )?;
        }
    }

    // Reject unparsable or mismatched PEM pairs before they reach the data
    // plane: a broken certificate takes the site's HTTPS down, and this path
    // used to store whatever string arrived (2026-10-18 audit). The checks
    // live in `validation`, shared with the update path.
    let has_pem = payload
        .cert_pem
        .as_deref()
        .is_some_and(|pem| !pem.trim().is_empty());
    match (
        payload
            .cert_pem
            .as_deref()
            .filter(|pem| !pem.trim().is_empty()),
        payload
            .key_pem
            .as_deref()
            .filter(|pem| !pem.trim().is_empty()),
    ) {
        (Some(cert_pem), Some(key_pem)) => {
            validate_certificate_and_key(cert_pem, key_pem)?;
        },
        (Some(cert_pem), None) => validate_pem_pair(cert_pem, None)?,
        // A key without its certificate cannot be served; storing it would
        // leave the row looking complete.
        (None, Some(_)) => {
            return Err(ApiError::BadRequest(
                "key_pem requires cert_pem".to_string(),
            ));
        },
        (None, None) => {},
    }

    let timestamp = Utc::now();
    let status = if has_pem {
        match site {
            // Nothing serves an unbound row, so no edge report ever corrects
            // its status. Recording what the dates say keeps a lapsed
            // certificate from looking healthy in the console forever.
            None => uploaded_cert_status(payload.expires_at, timestamp),
            Some(_) => cert_status::ACTIVE,
        }
    } else if payload.acme_email.is_some() {
        if site.is_none() {
            // Issuance runs on the edge, driven by the configuration of the
            // site that carries the certificate. One no site carries is never
            // picked up, so the row would sit `pending` until the stale-order
            // sweep marked it `failed` — say why instead.
            return Err(ApiError::BadRequest(
                "an ACME certificate needs a site_id: issuance runs on the edge \
                 for the site that carries it — upload the PEM instead, or name \
                 the site"
                    .to_string(),
            ));
        }
        cert_status::PENDING
    } else {
        // Neither a PEM pair nor an ACME order: nothing to serve, and the
        // old code still marked the row ACTIVE (2026-10-18 audit).
        return Err(ApiError::BadRequest(
            "provide cert_pem/key_pem or an acme_email".to_string(),
        ));
    };

    let model = site_certificates::ActiveModel {
        id: Set(Uuid::new_v4()),
        site_id: Set(site.map(|site| site.id)),
        domain: Set(domain),
        cert_pem: Set(payload.cert_pem),
        key_pem: Set(payload.key_pem),
        issuer: Set(payload.issuer),
        not_before: Set(payload.not_before),
        expires_at: Set(payload.expires_at),
        auto_renew: Set(payload.auto_renew),
        acme_email: Set(payload.acme_email),
        acme_challenge_type: Set(payload.acme_challenge_type),
        acme_dns_provider: Set(payload.acme_dns_provider),
        acme_dns_config: Set(payload.acme_dns_config),
        status: Set(status.to_string()),
        failure_reason: Set(None),
        created_at: Set(timestamp),
        updated_at: Set(timestamp),
    }
    .insert(db)
    .await?;

    tracing::info!(
        site_id = ?model.site_id,
        cert_id = %model.id,
        domain = %model.domain,
        "certificate created"
    );
    Ok(model)
}

/// Initial status of an uploaded certificate that no site serves yet.
///
/// Served certificates get their status corrected by the edge report, which is
/// why the upload path writes `active` for them. An unbound row has no edge to
/// report anything, so its status follows its own dates from the start.
fn uploaded_cert_status(
    expires_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> &'static str {
    match expires_at {
        Some(expires_at) if expires_at <= now => cert_status::EXPIRED,
        _ => cert_status::ACTIVE,
    }
}

/// Records the audit event for a committed certificate and pushes the change
/// to the site's agents. The event is best-effort (like every certificate
/// event); the touch failure is logged, not propagated — the row already
/// exists, so an error response would misrepresent the outcome.
///
/// An unbound certificate has no site whose configuration could have changed —
/// it reaches the edge the moment it is bound — so only the audit event is
/// written for it. The event row keeps `site_id` NULL rather than a nil UUID,
/// which is what "belongs to no site" means in this table.
async fn announce_certificate_created(
    state: &AppState,
    model: &site_certificates::Model,
    actor: Option<&str>,
) {
    write_cert_event(
        &state.db,
        model.id,
        model.site_id,
        "created",
        format!(
            "Certificate for {} created (status: {})",
            model.domain, model.status
        ),
        None,
    )
    .await;
    let Some(site_id) = model.site_id else {
        return;
    };
    if let Err(err) = touch_site(state, site_id).await {
        tracing::warn!(
            cert_id = %model.id,
            error = %err,
            "could not touch the site after certificate creation"
        );
    }
    notify_config_changed(state, site_id, actor).await;
}

/// `GET /api/v1/sites/{site_id}/certificates/{cert_id}`
async fn show_certificate(
    State(state): State<AppState>,
    current: AuthUser,
    Path((site_id, cert_id)): Path<(String, String)>,
) -> Result<Json<CertificateResponse>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    let target = parse_uuid(&cert_id, "certificate id")?;
    load_site_read(&state.db, id, &current).await?;

    let row = find_certificate(&state, id, target).await?;
    let served = site_hostname_index(&state, std::iter::once(id)).await;
    let owners = HashMap::from([(target, row.site_id)]);
    let bound =
        bound_sites_index(&state, std::iter::once(target), &owners).await;
    let mut response = CertificateResponse::from(row);
    response.warnings = Some(certificate_warnings_for(
        &served,
        response.site_id.unwrap_or_default(),
        &response.domain,
    ));
    attach_bindings(&mut response, &bound, &owners);
    Ok(Json(response))
}

/// `PUT /api/v1/sites/{site_id}/certificates/{cert_id}`
async fn update_certificate(
    State(state): State<AppState>,
    current: AuthUser,
    Path((site_id, cert_id)): Path<(String, String)>,
    Json(payload): Json<UpdateCertificateRequest>,
) -> Result<Json<CertificateResponse>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    let target = parse_uuid(&cert_id, "certificate id")?;
    let site = load_site_write(&state.db, id, &current).await?;

    let row = find_certificate(&state, id, target).await?;
    let updated =
        apply_certificate_update(&state, Some(&site), row, payload).await?;

    tracing::info!(site_id = %id, cert_id = %target, "certificate updated");
    touch_site(&state, id).await?;
    // A shared certificate can be served by several sites, so every one of them
    // has to re-pull — not only the owner. Replacing the PEM of a certificate
    // another site links would otherwise reach the edge only on its next
    // unrelated configuration change.
    notify_certificate_users(&state, target, Some(id), Some(&current.email))
        .await;

    // The warning is evaluated against the certificate as it is stored now,
    // not against the payload alone: an update that only replaces the PEM
    // still has to surface a hostname mismatch introduced earlier.
    let warnings = certificate_warnings(site.id, &site, &updated.domain);
    let owner = updated.site_id;
    let mut response = CertificateResponse::from(updated);
    response.warnings = Some(warnings);
    // Same decorations as every other certificate response: an update returns
    // the row, so it has to describe its shares too (the panel renders this
    // object in place and would otherwise show "unbound" after an edit).
    let owners = std::collections::HashMap::from([(target, owner)]);
    let bound =
        bound_sites_index(&state, std::iter::once(target), &owners).await;
    attach_bindings(&mut response, &bound, &owners);
    Ok(Json(response))
}

/// Validates and applies one certificate update, with or without an owner site.
///
/// Split out of the handler so the path that edits a certificate uploaded
/// before it was bound (`PUT /certificates/{id}` on an unowned row) shares every
/// check rather than re-implementing them: the PEM pair, the hostname list and
/// the ACME bounds are identical whether or not a site currently owns the row.
/// `site` is `None` for an unbound certificate, which drops exactly the one
/// site-scoped step — the ACME wildcard guard, which needs the site's hostname
/// set to compare against.
async fn apply_certificate_update(
    state: &AppState,
    site: Option<&site::Model>,
    row: site_certificates::Model,
    payload: UpdateCertificateRequest,
) -> Result<site_certificates::Model, ApiError> {
    // Replacing either half of the stored pair re-validates the pair as it
    // will be after this request: the update path used to store an unparsable
    // certificate (or a key that does not match it) that create would have
    // refused, which takes the site's HTTPS down on the next push
    // (2026-10-19 audit, D07).
    let stored_cert =
        row.cert_pem.as_deref().filter(|pem| !pem.trim().is_empty());
    let stored_key =
        row.key_pem.as_deref().filter(|pem| !pem.trim().is_empty());
    let next_cert = match payload.cert_pem.as_deref() {
        Some(pem) => Some(pem).filter(|pem| !pem.trim().is_empty()),
        None => stored_cert,
    };
    let next_key = match payload.key_pem.as_deref() {
        Some(pem) => Some(pem).filter(|pem| !pem.trim().is_empty()),
        None => stored_key,
    };
    // A replacement certificate validates against the stored key and a
    // replacement key against the stored certificate, so a mismatched pair
    // cannot be assembled one request at a time.
    if let Some(cert_pem) = next_cert {
        validate_pem_pair(cert_pem, next_key)?;
    } else if next_key.is_some() {
        return Err(ApiError::BadRequest(
            "key_pem requires a certificate: upload cert_pem with it"
                .to_string(),
        ));
    }

    // The domain is vetted first, exactly as on the create path: the ACME
    // wildcard guard below reads the hostname list, so letting it run on the
    // unvalidated `payload.domain` would both check a different string from
    // the one stored and reorder the two failures (audit R-03). A rejected
    // domain must leave the stored certificate exactly as it was.
    let next_domain = match non_empty(&payload.domain) {
        Some(raw) => Some(validate_cert_domain_list(&raw)?),
        None => None,
    };
    // The list the certificate will name once this request commits: the
    // normalised payload domain when one was sent, the stored one otherwise.
    let effective_domain = next_domain.as_deref().unwrap_or(&row.domain);

    // An ACME order without a stored PEM is issued against the site's whole
    // hostname set, which a wildcard may not contain under http-01.
    let cert_missing = match payload.cert_pem.as_deref() {
        Some(pem) => pem.trim().is_empty(),
        None => row
            .cert_pem
            .as_deref()
            .is_none_or(|pem| pem.trim().is_empty()),
    };
    let acme_email = match payload.acme_email.as_ref() {
        Some(email) => non_empty(&Some(email.clone())),
        None => non_empty(&row.acme_email),
    };
    if cert_missing && acme_email.is_some() {
        if let Some(site) = site {
            let challenge = non_empty(&payload.acme_challenge_type)
                .unwrap_or_else(|| row.acme_challenge_type.clone());
            ensure_acme_wildcard_supported(
                site,
                &split_domains(effective_domain),
                Some(&challenge),
            )?;
        }
    }

    let mut active: site_certificates::ActiveModel = row.into();

    if let Some(domain) = next_domain {
        active.domain = Set(domain);
    }
    if let Some(cert) = payload.cert_pem {
        active.cert_pem = Set(Some(cert));
    }
    if let Some(key) = payload.key_pem {
        active.key_pem = Set(Some(key));
    }
    if let Some(issuer) = payload.issuer {
        active.issuer = Set(non_empty(&Some(issuer)));
    }
    if let Some(nb) = payload.not_before {
        active.not_before = Set(Some(nb));
    }
    if let Some(exp) = payload.expires_at {
        active.expires_at = Set(Some(exp));
    }
    if let Some(ar) = payload.auto_renew {
        active.auto_renew = Set(ar);
    }
    if let Some(email) = payload.acme_email {
        active.acme_email = Set(non_empty(&Some(email)));
    }
    if let Some(ct) = non_empty(&payload.acme_challenge_type) {
        if !acme_challenge::is_valid(&ct) {
            return Err(ApiError::BadRequest(format!(
                "invalid acme_challenge_type '{ct}'"
            )));
        }
        active.acme_challenge_type = Set(ct);
    }
    if let Some(provider) = payload.acme_dns_provider {
        active.acme_dns_provider = Set(non_empty(&Some(provider)));
    }
    if let Some(config) = payload.acme_dns_config {
        active.acme_dns_config = Set(Some(config));
    }
    if let Some(status) = non_empty(&payload.status) {
        if !cert_status::is_valid(&status) {
            return Err(ApiError::BadRequest(format!(
                "invalid certificate status '{status}'"
            )));
        }
        active.status = Set(status.clone());
        // Re-arming an order discards the reason the previous one was given
        // up on, so a stale failure never outlives its retry.
        if status == cert_status::PENDING {
            active.failure_reason = Set(None);
        }
    }
    active.updated_at = Set(Utc::now());

    let updated = active.update(&state.db).await?;
    Ok(updated)
}

/// Pushes the configuration of every site that uses `cert_id`: the owner, when
/// it has one, plus every site holding a link row.
///
/// Certificate material is no longer reachable from one site alone — a PEM on
/// the owner's row is injected into the bundle of each site that links it — so
/// a change to the row is a change to *their* configurations too. `touch_site`
/// runs for each of them so the fingerprint the agents compare moves as well.
///
/// Best-effort like [`notify_config_changed`] itself: the rows are committed,
/// and an agent that misses the push picks the change up on its next sync.
async fn notify_certificate_users(
    state: &AppState,
    cert_id: Uuid,
    owner: Option<Uuid>,
    actor: Option<&str>,
) {
    let linked: Vec<Uuid> = certificate_links::Entity::find()
        .filter(certificate_links::Column::CertificateId.eq(cert_id))
        .all(&state.db)
        .await
        .map(|rows| rows.into_iter().map(|link| link.site_id).collect())
        .unwrap_or_default();
    let mut sites: Vec<Uuid> = owner.into_iter().chain(linked).collect();
    sites.sort_unstable();
    sites.dedup();

    for site_id in sites {
        if let Err(err) = touch_site(state, site_id).await {
            tracing::warn!(
                %site_id,
                error = %err,
                "could not touch a site that uses this certificate"
            );
        }
        notify_config_changed(state, site_id, actor).await;
    }
}

/// `DELETE /api/v1/sites/{site_id}/certificates/{cert_id}`
async fn delete_certificate(
    State(state): State<AppState>,
    current: AuthUser,
    Path((site_id, cert_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    let target = parse_uuid(&cert_id, "certificate id")?;
    load_site_write(&state.db, id, &current).await?;
    find_certificate(&state, id, target).await?;

    delete_certificate_row(&state, &current, Some(id), target).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Deletes a certificate after refusing while anything still depends on it.
///
/// `owner` is the site the request was authorised through, `None` when the row
/// has no owner (uploaded, not bound to a site of its own yet).
///
/// Two blockers, in this order:
///
/// 1. a `site_ssl.certificate_id` still selecting it — deleting would leave the
///    id dangling and the data plane would silently serve no certificate;
/// 2. a `site_certificate_links` grant — another site serves this PEM, so the
///    operator has to unbind first. The order matters: an unlink is itself
///    refused while the site has the certificate selected, so reporting the
///    link first would send the operator in a circle.
async fn delete_certificate_row(
    state: &AppState,
    current: &AuthUser,
    owner: Option<Uuid>,
    target: Uuid,
) -> Result<(), ApiError> {
    // Same predicate as the usage endpoint, same index:
    // `idx_site_ssl_certificate_id` (migration `m20240101_000091`).
    let in_use: Vec<Uuid> = site_ssl::Entity::find()
        .filter(site_ssl::Column::CertificateId.eq(target))
        .all(&state.db)
        .await?
        .into_iter()
        .map(|row| row.site_id)
        .collect();
    if !in_use.is_empty() {
        return Err(ApiError::Conflict(format!(
            "certificate is selected by site(s) {}; clear or replace it there first",
            in_use
                .iter()
                .map(Uuid::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }

    // Any remaining grant means another site serves this PEM: refuse instead of
    // pulling the certificate out from under it. The owner's own row is not a
    // grant - migration 85 wrote one when a site selected its certificate - so
    // it is skipped here as well; otherwise a delete would answer 409 with a
    // site the console never offers an unbind button for.
    let mut bound: Vec<Uuid> = certificate_links::Entity::find()
        .filter(certificate_links::Column::CertificateId.eq(target))
        .all(&state.db)
        .await?
        .into_iter()
        .filter(|link| !is_owner_self_link(link.site_id, owner))
        .map(|link| link.site_id)
        .collect();
    if !bound.is_empty() {
        bound.sort_unstable();
        bound.dedup();
        return Err(ApiError::Conflict(format!(
            "certificate is bound to site(s) {}; unbind it there first",
            bound
                .iter()
                .map(Uuid::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }

    site_certificates::Entity::delete_by_id(target)
        .exec(&state.db)
        .await?;

    // Shared uses must not outlive the certificate. `site_certificate_links`
    // has no FK on `certificate_id` (site-scoped restore requires that), so the
    // cleanup is explicit — unreachable through the guard above, kept as the
    // belt-and-braces that a row written by another path cannot outlive its
    // certificate. `site_ssl.certificate_id` is left as-is on purpose: the data
    // plane falls back to the legacy PEM/self-signed path, and a dangling id is
    // rejected on the next save rather than silently changing TLS behaviour
    // here.
    certificate_links::Entity::delete_many()
        .filter(certificate_links::Column::CertificateId.eq(target))
        .exec(&state.db)
        .await?;

    tracing::info!(site_id = ?owner, cert_id = %target, "certificate deleted");
    // Deleting the certificate cascades to its events, so the deletion's own
    // audit trail must not point at the deleted row — referencing it violates
    // `fk_certificate_events_certificate_id` and the event is silently lost.
    let event = certificate_events::ActiveModel {
        id: Set(Uuid::new_v4()),
        certificate_id: Set(None),
        site_id: Set(owner),
        event_type: Set("deleted".to_string()),
        message: Set(format!("Certificate {target} deleted")),
        details: Set(None),
        created_at: Set(Utc::now()),
    };
    if let Err(err) = event.insert(&state.db).await {
        tracing::warn!(%err, "failed to write certificate event");
    }
    if let Some(owner) = owner {
        touch_site(state, owner).await?;
        notify_config_changed(state, owner, Some(&current.email)).await;
    }
    Ok(())
}

/// `POST /api/v1/sites/{site_id}/certificates/{cert_id}/renew`
async fn renew_certificate(
    State(state): State<AppState>,
    current: AuthUser,
    Path((site_id, cert_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    let target = parse_uuid(&cert_id, "certificate id")?;
    load_site_write(&state.db, id, &current).await?;

    let row = find_certificate(&state, id, target).await?;
    if row.acme_email.is_none() {
        return Err(ApiError::BadRequest(
            "certificate is not ACME-managed; cannot auto-renew".to_string(),
        ));
    }

    let domain = row.domain.clone();
    // Mark as pending renewal; the previous failure (if any) is superseded
    // by this retry.
    let mut active: site_certificates::ActiveModel = row.into();
    active.status = Set(cert_status::PENDING.to_string());
    active.failure_reason = Set(None);
    active.updated_at = Set(Utc::now());
    active.update(&state.db).await?;

    tracing::info!(site_id = %id, cert_id = %target, "certificate renewal triggered");
    write_cert_event(
        &state.db,
        target,
        Some(id),
        "renewal_requested",
        format!("Manual renewal requested for {}", domain),
        None,
    )
    .await;

    // Without this the renewal used to be a dud: only the status column
    // changed, the agents were never told to re-pull the site config, and
    // the edge kept serving the old certificate until it lapsed.
    touch_site(&state, id).await?;
    notify_config_changed(&state, id, Some(&current.email)).await;

    Ok(Json(serde_json::json!({
        "message": "renewal initiated",
        "certificate_id": target,
        "status": cert_status::PENDING,
    })))
}

/// `GET /api/v1/sites/{site_id}/ssl-settings`
async fn get_ssl_settings(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
) -> Result<Json<SslSettingsResponse>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    load_site_read(&state.db, id, &current).await?;

    let posture = load_tls_posture(&state, id).await?;
    Ok(Json(posture.into()))
}

/// `PUT /api/v1/sites/{site_id}/ssl-settings`
///
/// Persists the TLS switches on the site's `site_ssl` row, creating the row if
/// the site has none yet — the settings and the certificate share the row, so
/// either endpoint can establish it.
async fn update_ssl_settings(
    State(state): State<AppState>,
    current: AuthUser,
    Path(site_id): Path<String>,
    Json(payload): Json<TlsPostureRequest>,
) -> Result<Json<SslSettingsResponse>, ApiError> {
    let id = parse_uuid(&site_id, "site id")?;
    let site = load_site_write(&state.db, id, &current).await?;

    let posture = save_tls_posture(&state.db, &site, &payload).await?;
    tracing::info!(site_id = %id, "SSL settings updated");
    touch_site(&state, id).await?;
    notify_config_changed(&state, id, Some(&current.email)).await;

    Ok(Json(posture.into()))
}

/// Loads a certificate scoped to a site.
async fn find_certificate(
    state: &AppState,
    site_id: Uuid,
    cert_id: Uuid,
) -> Result<site_certificates::Model, ApiError> {
    site_certificates::Entity::find_by_id(cert_id)
        .filter(site_certificates::Column::SiteId.eq(site_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| {
            ApiError::NotFound(format!("certificate {cert_id} not found"))
        })
}

// ─── Global certificate management ─────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct GlobalListQuery {
    /// Restrict the list to one site. Optional for administrators, who would
    /// otherwise have to walk their sites one by one.
    #[serde(default)]
    pub site_id: Option<String>,
    /// Filter on the certificate status (`active`, `pending`, …).
    #[serde(default)]
    pub status: Option<String>,
    #[serde(flatten)]
    pub pagination: Pagination,
}

/// A certificate plus the site it belongs to, for the cross-site list.
#[derive(Debug, Serialize)]
pub struct CertificateWithSite {
    #[serde(flatten)]
    pub certificate: CertificateResponse,
    pub site_domain: String,
    pub site_name: String,
}

/// Counts behind the global SSL/TLS overview.
#[derive(Debug, Serialize)]
pub struct CertificateSummary {
    pub total: usize,
    pub active: usize,
    pub pending: usize,
    pub failed: usize,
    pub expired: usize,
    /// Certificates that lapse within the next 30 days, lapsed ones included.
    pub expiring_soon: usize,
}

/// Site id -> `(domain, name)`, the shape [`visible_sites`] returns.
type VisibleSites = HashMap<Uuid, (String, String)>;

/// Sites the caller may see, mapped to `(domain, name)`.
async fn visible_sites(
    state: &AppState,
    current: &AuthUser,
) -> Result<VisibleSites, ApiError> {
    let mut condition = Condition::all();
    if !current.is_admin() {
        condition = condition.add(site::Column::UserId.eq(current.id));
    }
    let rows = site::Entity::find()
        .filter(condition)
        .all(&state.db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|model| (model.id, (model.domain, model.name)))
        .collect())
}

/// `GET /api/v1/certificates` — certificate applications across every site the
/// caller sees, newest first.
async fn list_all_certificates(
    State(state): State<AppState>,
    current: AuthUser,
    Query(query): Query<GlobalListQuery>,
) -> Result<Json<Page<CertificateWithSite>>, ApiError> {
    let requested = match query.site_id.as_deref() {
        Some(raw) => Some(parse_uuid(raw, "site id")?),
        None => None,
    };
    let sites = visible_sites(&state, &current).await?;
    let pagination = query.pagination.normalise();

    let mut condition = Condition::all();
    if let Some(id) = scope_site(requested, &current)? {
        if !sites.contains_key(&id) {
            return Err(ApiError::NotFound(format!("site {id} not found")));
        }
        condition = condition.add(site_certificates::Column::SiteId.eq(id));
    }
    if let Some(status) = non_empty(&query.status) {
        if !cert_status::is_valid(&status) {
            return Err(ApiError::BadRequest(format!(
                "invalid status '{status}' (supported: active, pending, expired, failed)"
            )));
        }
        condition = condition.add(site_certificates::Column::Status.eq(status));
    }

    let paginator = certificate_list_query()
        .filter(condition)
        .order_by_desc(site_certificates::Column::CreatedAt)
        .into_model::<CertificateListRow>()
        .paginate(&state.db, pagination.limit());

    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(pagination.index()).await?;
    // H3: one batch lookup for the hostnames of every site on this page (plus
    // the site named in the query, if any), then pure string comparisons — no
    // query per row. Sites outside the caller's visible set are skipped here;
    // their certificates are already filtered out above.
    let served = site_hostname_index(
        &state,
        rows.iter().filter_map(|row| row.site_id).chain(requested),
    )
    .await;
    // Built once for the page: one entry per certificate, so the binding count
    // can drop the owner's own link row (see `bound_sites_index`).
    let owners: HashMap<Uuid, Option<Uuid>> =
        rows.iter().map(|row| (row.id, row.site_id)).collect();
    let bound =
        bound_sites_index(&state, rows.iter().map(|row| row.id), &owners).await;
    let items: Vec<CertificateWithSite> = rows
        .into_iter()
        .map(|row| {
            // An unbound certificate has no owner: the two site columns stay
            // empty, which is what the console renders as "not bound yet".
            let (site_domain, site_name) = row
                .site_id
                .and_then(|id| sites.get(&id).cloned())
                .unwrap_or_default();
            let warnings = certificate_warnings_for(
                &served,
                row.site_id.unwrap_or_default(),
                row.domain.as_str(),
            );
            let mut certificate = CertificateResponse::from(row);
            certificate.warnings = Some(warnings);
            attach_bindings(&mut certificate, &bound, &owners);
            CertificateWithSite {
                certificate,
                site_domain,
                site_name,
            }
        })
        .collect();
    Ok(Json(Page::new(items, total, pagination)))
}

/// `POST /api/v1/certificates` without a site: a certificate that exists before
/// anything is bound to it.
///
/// Administrator-only, like every other certificate operation addressed by
/// certificate id with no site to authorise through (see
/// [`authorize_certificate`]) — there is no ownership that could grant the write.
///
/// Only an uploaded PEM can land here. An ACME request is refused by
/// [`insert_certificate`]: issuance runs on the edge, driven by the
/// configuration of the site that carries the certificate, so an order nothing
/// carries would sit `pending` until the stale sweep marked it `failed`.
async fn create_unbound_certificate(
    state: &AppState,
    current: &AuthUser,
    payload: CreateCertificateRequest,
) -> Result<Response, ApiError> {
    current.require_admin().map_err(ApiError::from)?;
    if payload.activate {
        return Err(ApiError::BadRequest(
            "activate requires site_id: turning HTTPS on is a per-site setting"
                .to_string(),
        ));
    }

    let model = insert_certificate(&state.db, None, payload).await?;
    announce_certificate_created(state, &model, Some(&current.email)).await;
    // Coverage is measured against a site's hostnames, and nothing is bound
    // yet: "checked, nothing to report" rather than a missing field.
    let mut response = CertificateResponse::from(model);
    response.warnings = Some(Vec::new());
    Ok((StatusCode::CREATED, Json(response)).into_response())
}

/// `POST /api/v1/certificates` — upload a certificate, optionally on behalf of
/// a site, or apply for one for a site.
///
/// `site_id` is optional: a PEM already in hand has no intrinsic site, and the
/// same certificate may later serve several of them, so an upload without a site
/// creates an **unbound** row (`site_id = NULL`, `bound_count = 0`) that
/// `POST /certificates/{id}/bindings` attaches afterwards. The `domain` is what
/// identifies the certificate and stays mandatory.
///
/// `activate` attaches it to its site and turns HTTPS on in the same call, so
/// the dashboard does not have to make a second request; it needs a site.
async fn create_global_certificate(
    State(state): State<AppState>,
    current: AuthUser,
    Json(payload): Json<CreateCertificateRequest>,
) -> Result<Response, ApiError> {
    let Some(raw_site) = payload.site_id else {
        return create_unbound_certificate(&state, &current, payload).await;
    };
    let site_id = parse_uuid(&raw_site.to_string(), "site id")?;
    let site = load_site_write(&state.db, site_id, &current).await?;
    let activate = payload.activate;

    // The row and the activated posture commit together: a posture failure
    // (bad TLS bounds, a certificate id from another site) must not leave a
    // pending certificate behind, which is how the dashboard used to show an
    // errored request with the task already in the list.
    let txn = state.db.begin().await?;
    let model = insert_certificate(&txn, Some(&site), payload).await?;
    if activate {
        let posture = TlsPostureRequest {
            https_enabled: Some(true),
            certificate_id: Some(model.id),
            ..Default::default()
        };
        save_tls_posture(&txn, &site, &posture).await?;
    }
    txn.commit().await?;

    announce_certificate_created(&state, &model, Some(&current.email)).await;
    if activate {
        tracing::info!(site_id = %site_id, cert_id = %model.id, "certificate activated");
    }

    let warnings = certificate_warnings(site_id, &site, &model.domain);
    let mut response = CertificateResponse::from(model);
    response.warnings = Some(warnings);
    Ok((StatusCode::CREATED, Json(response)).into_response())
}

/// Authorises a certificate operation that is addressed by certificate id.
///
/// A certificate with an owner is authorised through that site, exactly as
/// before. One that has no owner yet has no site to check either, so it is
/// administrator-only — the same boundary [`scope_site`] draws for the
/// cross-site listing.
async fn authorize_certificate(
    state: &AppState,
    current: &AuthUser,
    owner: Option<Uuid>,
    write: bool,
) -> Result<(), ApiError> {
    match owner {
        Some(owner) if write => {
            load_site_write(&state.db, owner, current).await?;
        },
        Some(owner) => {
            load_site_read(&state.db, owner, current).await?;
        },
        None => current.require_admin().map_err(ApiError::from)?,
    }
    Ok(())
}

/// `GET /api/v1/certificates/{cert_id}`
async fn show_global_certificate(
    State(state): State<AppState>,
    current: AuthUser,
    Path(cert_id): Path<String>,
) -> Result<Json<CertificateResponse>, ApiError> {
    let target = parse_uuid(&cert_id, "certificate id")?;
    let row = find_certificate_by_id(&state, target).await?;
    authorize_certificate(&state, &current, row.site_id, false).await?;

    let served =
        site_hostname_index(&state, std::iter::once(row.site_id).flatten())
            .await;
    let owners = HashMap::from([(target, row.site_id)]);
    let bound =
        bound_sites_index(&state, std::iter::once(target), &owners).await;
    let mut response = CertificateResponse::from(row);
    response.warnings = Some(certificate_warnings_for(
        &served,
        response.site_id.unwrap_or_default(),
        &response.domain,
    ));
    attach_bindings(&mut response, &bound, &owners);
    Ok(Json(response))
}

/// One site a certificate is shared with, plus whether that site currently
/// serves it.
///
/// `selected` is the flag the console needs before it renders an "unlink"
/// button: it is exactly the state [`unbind_certificate`] and
/// [`delete_certificate_row`] refuse with a `409`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LinkedSiteRef {
    pub id: Uuid,
    pub domain: String,
    pub selected: bool,
}

/// `GET /api/v1/certificates/{cert_id}/usage`.
///
/// The three relations a certificate can stand in, kept separate rather than
/// merged into one "in use" list because each one blocks a different action
/// with a different message: an `owner` is why binding the certificate to that
/// site answers `409 certificate already belongs to this site`, a
/// `linked_sites` entry is the grant `DELETE /bindings` removes, and a
/// `serving_sites` entry is why deletion is refused until the site clears its
/// TLS choice.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct CertificateUsageResponse {
    /// `site_certificates.site_id`, or `null` for an unbound upload. `null`
    /// also covers an owner site the caller cannot see: the certificate is
    /// still readable, so the answer degrades in this field instead of 404ing.
    pub owner: Option<BoundSiteRef>,
    /// `site_certificate_links` rows of this certificate — the sharing grants.
    /// The owner's own row is excluded: migration 85 backfilled one for every
    /// site that had already selected its certificate, so it repeats the owner
    /// relation instead of granting anything (the row stays in the table).
    pub linked_sites: Vec<LinkedSiteRef>,
    /// Sites whose `site_ssl.certificate_id` is this certificate.
    pub serving_sites: Vec<BoundSiteRef>,
}

/// Assembles the usage answer from rows the caller has already loaded.
///
/// Query-free on purpose: the derivation — in particular `selected`, which is
/// the condition the unbind and delete paths refuse on — is the part worth
/// testing, and it must not depend on the *link* table to be right. A site can
/// select a certificate whose grant row was never written (the site's own
/// certificate, reached through `PUT /sites/{id}/ssl-settings`), so
/// `serving_sites` is derived from `site_ssl` and `linked_sites` only carries
/// the flag.
///
/// The owner's own link row is dropped from `linked_sites`: migration 85
/// backfilled `(site_id, certificate_id)` for every site that had already
/// selected its certificate, so the owner's row repeats the owner relation
/// instead of granting anything. `serving_sites` keeps it — an owner serving
/// its own certificate is the normal case — which is why the two lists are not
/// derived from one another.
///
/// An id that does not resolve (a row deleted between two of the handler's
/// queries) is skipped; an unresolvable **owner** is logged once and reported
/// as `null`, matching how [`bound_sites_index`] treats a stale link.
fn certificate_usage_snapshot(
    owner: Option<Uuid>,
    linked_ids: impl IntoIterator<Item = Uuid>,
    serving_ids: impl IntoIterator<Item = Uuid>,
    sites: &VisibleSites,
) -> CertificateUsageResponse {
    let serving: HashSet<Uuid> = serving_ids.into_iter().collect();

    // Kept as the raw id: the owner's own link row is not a share, so it is
    // filtered out of `linked_sites` below. `serving_sites` keeps the owner —
    // an owner serving its own certificate is the normal case, and it is the
    // delete blocker the console has to explain.
    let owner_id = owner;
    let owner = owner.and_then(|id| match sites.get(&id) {
        Some((domain, _)) => Some(BoundSiteRef {
            id,
            domain: domain.clone(),
        }),
        None => {
            tracing::warn!(
                site_id = %id,
                "certificate owner site is not visible; reporting no owner"
            );
            None
        },
    });

    let mut linked_sites: Vec<LinkedSiteRef> = linked_ids
        .into_iter()
        .filter(|id| Some(*id) != owner_id)
        .filter_map(|id| {
            sites.get(&id).map(|(domain, _)| LinkedSiteRef {
                id,
                domain: domain.clone(),
                selected: serving.contains(&id),
            })
        })
        .collect();
    let mut serving_sites: Vec<BoundSiteRef> = serving
        .into_iter()
        .filter_map(|id| {
            sites.get(&id).map(|(domain, _)| BoundSiteRef {
                id,
                domain: domain.clone(),
            })
        })
        .collect();

    // Domain order, id as the tiebreaker. Neither `site_certificate_links` nor
    // `site_ssl` has a natural order, and a list that reshuffles between two
    // identical requests is noise in the console and in any diff of two
    // responses.
    linked_sites.sort_by(|a, b| a.domain.cmp(&b.domain).then(a.id.cmp(&b.id)));
    serving_sites.sort_by(|a, b| a.domain.cmp(&b.domain).then(a.id.cmp(&b.id)));

    CertificateUsageResponse {
        owner,
        linked_sites,
        serving_sites,
    }
}

/// `GET /api/v1/certificates/{cert_id}/usage`
///
/// Four queries, whatever the number of sites involved:
///
/// 1. the certificate row — [`find_certificate_by_id`], `site_id` is the owner;
/// 2. `site_certificate_links` filtered on `certificate_id` — the grants, minus
///    the owner's own row (migration 85's backfill; see
///    [`certificate_usage_snapshot`]);
/// 3. `site_ssl` filtered on `certificate_id` — the sites that selected it,
///    which is the `selected` flag and the first delete blocker in
///    [`delete_certificate_row`];
/// 4. the site rows — [`visible_sites`], one query that resolves the domains
///    for 1–3 and doubles as the visibility oracle for the owner.
///
/// Administrator-only: every consumer is a dialog in front of a bind, unbind
/// or delete, and all three mutate, which [`require_write`] already restricts
/// to administrators. `visible_sites` is still scoped per caller, so the
/// `owner == null` branch for an invisible site becomes real work the moment a
/// read-only surface needs this and the extractor is switched to `AuthUser`.
///
/// [`require_write`]: crate::api::common::require_write
async fn certificate_usage(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(cert_id): Path<String>,
) -> Result<Json<CertificateUsageResponse>, ApiError> {
    let target = parse_uuid(&cert_id, "certificate id")?;
    let certificate = find_certificate_by_id(&state, target).await?;

    let linked_ids = certificate_links::Entity::find()
        .filter(certificate_links::Column::CertificateId.eq(target))
        .all(&state.db)
        .await?
        .into_iter()
        .map(|link| link.site_id);
    // Equality on `site_ssl.certificate_id`, served by the partial index
    // `idx_site_ssl_certificate_id` (migration `m20240101_000091`).
    let serving_ids = site_ssl::Entity::find()
        .filter(site_ssl::Column::CertificateId.eq(target))
        .all(&state.db)
        .await?
        .into_iter()
        .map(|row| row.site_id);
    let sites = visible_sites(&state, &admin).await?;

    Ok(Json(certificate_usage_snapshot(
        certificate.site_id,
        linked_ids,
        serving_ids,
        &sites,
    )))
}

/// `PUT /api/v1/certificates/{cert_id}`
///
/// Works on an unbound row too: a certificate uploaded before it is bound still
/// has to be editable (a wrong domain, a rotated PEM) without inventing a site
/// for it. Both cases share [`apply_certificate_update`]; only the ACME wildcard
/// guard and the warning computation need a site and are skipped without one.
async fn update_global_certificate(
    State(state): State<AppState>,
    current: AuthUser,
    Path(cert_id): Path<String>,
    payload: Json<UpdateCertificateRequest>,
) -> Result<Json<CertificateResponse>, ApiError> {
    let target = parse_uuid(&cert_id, "certificate id")?;
    let row = find_certificate_by_id(&state, target).await?;
    let site = match row.site_id {
        Some(site_id) => {
            Some(load_site_write(&state.db, site_id, &current).await?)
        },
        None => {
            current.require_admin().map_err(ApiError::from)?;
            None
        },
    };

    let updated =
        apply_certificate_update(&state, site.as_ref(), row, payload.0).await?;
    tracing::info!(cert_id = %target, "certificate updated");
    // The new material has to reach every site that serves it, owner or not.
    notify_certificate_users(
        &state,
        target,
        site.as_ref().map(|site| site.id),
        Some(&current.email),
    )
    .await;

    let warnings = match site.as_ref() {
        Some(site) => certificate_warnings(site.id, site, &updated.domain),
        // Coverage is measured against a site's hostnames; an unbound
        // certificate has none to be measured against.
        None => Vec::new(),
    };
    let owners = HashMap::from([(target, updated.site_id)]);
    let bound =
        bound_sites_index(&state, std::iter::once(target), &owners).await;
    let mut response = CertificateResponse::from(updated);
    response.warnings = Some(warnings);
    attach_bindings(&mut response, &bound, &owners);
    Ok(Json(response))
}

/// `DELETE /api/v1/certificates/{cert_id}`
///
/// Refused with `409` while any `site_certificate_links` row still grants the
/// certificate to a site: that site's data-plane configuration carries the PEM,
/// so deleting it would take its TLS down. An unbound certificate (no links, no
/// owner) deletes straight away.
async fn delete_global_certificate(
    State(state): State<AppState>,
    current: AuthUser,
    Path(cert_id): Path<String>,
) -> Result<Response, ApiError> {
    let target = parse_uuid(&cert_id, "certificate id")?;
    let row = find_certificate_by_id(&state, target).await?;
    authorize_certificate(&state, &current, row.site_id, true).await?;

    delete_certificate_row(&state, &current, row.site_id, target).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `POST /api/v1/certificates/{cert_id}/renew`
async fn renew_global_certificate(
    State(state): State<AppState>,
    current: AuthUser,
    Path(cert_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let target = parse_uuid(&cert_id, "certificate id")?;
    let row = find_certificate_by_id(&state, target).await?;
    // ACME issuance and renewal run on the edge, driven by a site's config, so a
    // certificate no site carries has nothing to renew against. Saying so beats
    // arming an order that can never be picked up.
    let Some(owner) = row.site_id else {
        return Err(ApiError::BadRequest(
            "certificate is not bound to any site; bind it before triggering an ACME renewal"
                .to_string(),
        ));
    };
    renew_certificate(State(state), current, Path((owner.to_string(), cert_id)))
        .await
}

#[derive(Debug, Deserialize)]
pub struct BindCertificateRequest {
    pub site_id: Uuid,
}

/// `POST /api/v1/certificates/{cert_id}/bindings`
///
/// Grants a site the right to serve this certificate — one `site_certificate_links`
/// row — which is what puts the PEM into that site's data-plane configuration.
///
/// Two checks run before the grant is written:
///
/// - the certificate has to **cover** the site's hostnames. Binding one that
///   cannot validate them would leave the site serving a TLS error it has no way
///   to notice; the uncovered hostnames are listed in the `400`. Extra hostnames
///   on the certificate are *not* an error — that is the same advisory warning
///   the upload path reports — so a certificate with SANs for several sites binds
///   to each of them.
/// - a certificate that already has an owner is another site's row, so the caller
///   must be able to read that site too, exactly like the site-scoped link
///   endpoint.
///
/// Idempotent: binding twice returns the same `200` instead of a conflict, so a
/// retried request is not an error. The site-scoped `POST
/// /sites/{id}/certificate-links` answers a duplicate the same way and decorates
/// its row through the same helper ([`decorate_certificate`]), so the two
/// endpoints cannot disagree about what "already bound" looks like; only the
/// ownership clash is a `409` on either of them.
async fn bind_certificate(
    State(state): State<AppState>,
    current: AuthUser,
    Path(cert_id): Path<String>,
    Json(payload): Json<BindCertificateRequest>,
) -> Result<Json<CertificateResponse>, ApiError> {
    let target = parse_uuid(&cert_id, "certificate id")?;
    let certificate = find_certificate_by_id(&state, target).await?;
    let site = load_site_write(&state.db, payload.site_id, &current).await?;

    if let Some(owner) = certificate.site_id {
        if owner == site.id {
            return Err(ApiError::Conflict(
                "certificate already belongs to this site".to_string(),
            ));
        }
        load_site_read(&state.db, owner, &current).await?;
    }

    let uncovered = uncovered_site_hostnames(
        &certificate.domain,
        &site.domain,
        &site.alternate_domains,
    );
    if !uncovered.is_empty() {
        return Err(ApiError::BadRequest(format!(
            "certificate {} (issued for {}) does not cover site hostname(s) {}",
            certificate.id,
            certificate.domain,
            uncovered.join(", ")
        )));
    }

    let exists = certificate_links::Entity::find()
        .filter(certificate_links::Column::SiteId.eq(site.id))
        .filter(certificate_links::Column::CertificateId.eq(certificate.id))
        .one(&state.db)
        .await?
        .is_some();
    if !exists {
        certificate_links::ActiveModel {
            site_id: Set(site.id),
            certificate_id: Set(certificate.id),
            linked_at: Set(Utc::now()),
        }
        .insert(&state.db)
        .await?;
        tracing::info!(
            site_id = %site.id,
            cert_id = %certificate.id,
            domain = %certificate.domain,
            "certificate bound to site"
        );
    }

    // The site's bundle now carries the PEM, so its agents have to re-pull.
    // `notify_config_changed` records a new configuration version only when the
    // fingerprint actually moved, which is why this also runs on the idempotent
    // path: a link written before this endpoint existed still has to reach the
    // edge on the first call.
    touch_site(&state, site.id).await?;
    notify_config_changed(&state, site.id, Some(&current.email)).await;

    Ok(Json(decorate_certificate(&state, certificate).await))
}

/// `DELETE /api/v1/certificates/{cert_id}/bindings/{site_id}`
///
/// Removes the grant; the certificate itself stays. The site falls back to
/// whatever its own `site_ssl` row provides — nothing, when it was relying on the
/// link — so the site's configuration is pushed straight away.
///
/// Refused while the site still *selects* this certificate in `site_ssl`: the
/// selection is what serves it, and removing the grant underneath would drop the
/// PEM out of the configuration silently. Clear or replace the selection first.
async fn unbind_certificate(
    State(state): State<AppState>,
    current: AuthUser,
    Path((cert_id, site_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let target = parse_uuid(&cert_id, "certificate id")?;
    let id = parse_uuid(&site_id, "site id")?;
    let certificate = find_certificate_by_id(&state, target).await?;
    let site = load_site_write(&state.db, id, &current).await?;
    if let Some(owner) = certificate.site_id {
        load_site_read(&state.db, owner, &current).await?;
    }

    let posture = site_ssl::Entity::find()
        .filter(site_ssl::Column::SiteId.eq(site.id))
        .one(&state.db)
        .await?;
    if posture.and_then(|row| row.certificate_id) == Some(target) {
        return Err(ApiError::Conflict(
            "certificate is the site's current TLS certificate; clear or replace it first"
                .to_string(),
        ));
    }

    let result = certificate_links::Entity::delete_many()
        .filter(certificate_links::Column::SiteId.eq(site.id))
        .filter(certificate_links::Column::CertificateId.eq(target))
        .exec(&state.db)
        .await?;
    if result.rows_affected == 0 {
        return Err(ApiError::NotFound(format!(
            "certificate {target} is not bound to site {}",
            site.id
        )));
    }

    tracing::info!(site_id = %site.id, cert_id = %target, "certificate unbound from site");
    touch_site(&state, site.id).await?;
    notify_config_changed(&state, site.id, Some(&current.email)).await;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/v1/certificates/summary`
async fn certificate_summary(
    State(state): State<AppState>,
    current: AuthUser,
) -> Result<Json<CertificateSummary>, ApiError> {
    let sites = visible_sites(&state, &current).await?;
    let mut summary = CertificateSummary {
        total: 0,
        active: 0,
        pending: 0,
        failed: 0,
        expired: 0,
        expiring_soon: 0,
    };
    if !current.is_admin() && sites.is_empty() {
        return Ok(Json(summary));
    }

    let mut condition = Condition::all();
    if !current.is_admin() {
        condition = condition.add(
            site_certificates::Column::SiteId.is_in(sites.keys().copied()),
        );
    }
    let rows = site_certificates::Entity::find()
        .filter(condition)
        .all(&state.db)
        .await?;

    let soon = Utc::now() + chrono::Duration::days(30);
    summary.total = rows.len();
    for row in &rows {
        match row.status.as_str() {
            cert_status::ACTIVE => summary.active += 1,
            cert_status::PENDING => summary.pending += 1,
            cert_status::FAILED => summary.failed += 1,
            cert_status::EXPIRED => summary.expired += 1,
            _ => {},
        }
        if row.expires_at.is_some_and(|expires_at| expires_at <= soon) {
            summary.expiring_soon += 1;
        }
    }
    Ok(Json(summary))
}

/// Looks a certificate up by id alone; the caller authorises through its site.
async fn find_certificate_by_id(
    state: &AppState,
    cert_id: Uuid,
) -> Result<site_certificates::Model, ApiError> {
    site_certificates::Entity::find_by_id(cert_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| {
            ApiError::NotFound(format!("certificate {cert_id} not found"))
        })
}

// ─── Certificate event audit trail ──────────────────────────────────────────

/// Certificate event response.
#[derive(Debug, Serialize)]
pub struct CertificateEventResponse {
    pub id: Uuid,
    pub certificate_id: Option<Uuid>,
    pub site_id: Option<Uuid>,
    pub event_type: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub domain: Option<String>,
    pub site_domain: Option<String>,
}

/// Writes one row into `certificate_events`. Best-effort: logs on failure but
/// does not propagate the error, so event recording never breaks the main flow.
async fn write_cert_event(
    db: &sea_orm::DatabaseConnection,
    certificate_id: Uuid,
    site_id: Option<Uuid>,
    event_type: &str,
    message: String,
    details: Option<serde_json::Value>,
) {
    let event = certificate_events::ActiveModel {
        id: Set(Uuid::new_v4()),
        certificate_id: Set(Some(certificate_id)),
        site_id: Set(site_id),
        event_type: Set(event_type.to_string()),
        message: Set(message),
        details: Set(details),
        created_at: Set(Utc::now()),
    };
    if let Err(err) = event.insert(db).await {
        tracing::warn!(%err, "failed to write certificate event");
    }
}

#[derive(Debug, Deserialize)]
pub struct EventsQuery {
    #[serde(default)]
    pub certificate_id: Option<String>,
    #[serde(default)]
    pub event_type: Option<String>,
    #[serde(flatten)]
    pub pagination: Pagination,
}

/// `GET /api/v1/certificates/{cert_id}/events`
async fn list_certificate_events(
    State(state): State<AppState>,
    current: AuthUser,
    Path(cert_id): Path<String>,
    Query(query): Query<EventsQuery>,
) -> Result<Json<Page<CertificateEventResponse>>, ApiError> {
    let target = parse_uuid(&cert_id, "certificate id")?;
    let row = find_certificate_by_id(&state, target).await?;
    // An unbound certificate's history is readable by an administrator only,
    // the same boundary `show_global_certificate` draws.
    authorize_certificate(&state, &current, row.site_id, false).await?;

    let pagination = query.pagination.normalise();
    let mut condition = Condition::all()
        .add(certificate_events::Column::CertificateId.eq(target));
    if let Some(ref et) = non_empty(&query.event_type) {
        condition = condition
            .add(certificate_events::Column::EventType.eq(et.as_str()));
    }

    let paginator = certificate_events::Entity::find()
        .filter(condition)
        .order_by_desc(certificate_events::Column::CreatedAt)
        .paginate(&state.db, pagination.limit());

    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(pagination.index()).await?;

    let items = rows
        .into_iter()
        .map(|e| CertificateEventResponse {
            id: e.id,
            certificate_id: e.certificate_id,
            site_id: e.site_id,
            event_type: e.event_type,
            message: e.message,
            details: e.details,
            created_at: e.created_at,
            domain: Some(row.domain.clone()),
            site_domain: None,
        })
        .collect();
    Ok(Json(Page::new(items, total, pagination)))
}

/// `GET /api/v1/ssl-events` — cross-certificate event log for the SSL/TLS page.
async fn list_all_events(
    State(state): State<AppState>,
    current: AuthUser,
    Query(query): Query<EventsQuery>,
) -> Result<Json<Page<CertificateEventResponse>>, ApiError> {
    let sites = visible_sites(&state, &current).await?;
    let pagination = query.pagination.normalise();

    let mut condition = Condition::all();
    if !current.is_admin() {
        condition = condition.add(
            certificate_events::Column::SiteId.is_in(sites.keys().copied()),
        );
    }
    if let Some(ref cid) = non_empty(&query.certificate_id) {
        let cert_uuid = parse_uuid(cid, "certificate id")?;
        condition = condition
            .add(certificate_events::Column::CertificateId.eq(cert_uuid));
    }
    if let Some(ref et) = non_empty(&query.event_type) {
        condition = condition
            .add(certificate_events::Column::EventType.eq(et.as_str()));
    }

    let paginator = certificate_events::Entity::find()
        .filter(condition)
        .order_by_desc(certificate_events::Column::CreatedAt)
        .paginate(&state.db, pagination.limit());

    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(pagination.index()).await?;

    let cert_ids: Vec<Uuid> = rows
        .iter()
        .filter_map(|e| e.certificate_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let certs = if cert_ids.is_empty() {
        HashMap::new()
    } else {
        site_certificates::Entity::find()
            .filter(site_certificates::Column::Id.is_in(cert_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|c| (c.id, c))
            .collect()
    };

    let items = rows
        .into_iter()
        .map(|e| {
            let cert = e.certificate_id.and_then(|cid| certs.get(&cid));
            let site_domain = e
                .site_id
                .and_then(|sid| sites.get(&sid))
                .map(|(d, _)| d.clone());
            CertificateEventResponse {
                id: e.id,
                certificate_id: e.certificate_id,
                site_id: e.site_id,
                event_type: e.event_type,
                message: e.message,
                details: e.details,
                created_at: e.created_at,
                domain: cert.map(|c| c.domain.clone()),
                site_domain,
            }
        })
        .collect();
    Ok(Json(Page::new(items, total, pagination)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R-03: one hostname longer than a DNS name is refused before it reaches
    /// the database. The bound is per hostname (253), not per row: the row
    /// holds a comma-separated list and the column is TEXT.
    #[test]
    fn an_overlong_certhostname_domain_is_a_bad_request() {
        let long = format!("{}.example.com", "a".repeat(250));
        assert!(long.len() > MAX_SINGLE_HOSTNAME_LEN);
        let err = validate_cert_hostname(&long).unwrap_err();
        assert_eq!(err.status(), StatusCode::BAD_REQUEST);

        // The same hostname inside a list is refused too.
        let list = format!("a.com,{long}");
        let err = validate_cert_domain_list(&list).unwrap_err();
        assert_eq!(err.status(), StatusCode::BAD_REQUEST);
    }

    /// R-03: the empty string and a blank one are not a domain.
    #[test]
    fn a_blank_certificate_domain_is_a_bad_request() {
        for raw in ["", "   ", "\t\n"] {
            let err = validate_cert_domain_list(raw).unwrap_err();
            assert_eq!(err.status(), StatusCode::BAD_REQUEST, "{raw:?}");
            let err = validate_cert_hostname(raw).unwrap_err();
            assert_eq!(err.status(), StatusCode::BAD_REQUEST, "{raw:?}");
        }
    }

    /// R-03: `/` and `:` would let the value read as a path or a host:port to
    /// whoever parses it downstream, so they are refused rather than stored.
    #[test]
    fn a_certificate_domain_with_a_separator_is_a_bad_request() {
        for raw in [
            "example.com/path",
            "example.com:443",
            "https://example.com",
            "exa mple.com",
            "example.com\u{0}",
            "example.com\r\nX-Injected: 1",
            "example.com#frag",
            "user@example.com",
        ] {
            let err = validate_cert_domain_list(raw).unwrap_err();
            assert_eq!(err.status(), StatusCode::BAD_REQUEST, "{raw:?}");
        }
    }

    /// R-03: empty labels are refused; a leading `*.` is not one.
    #[test]
    fn empty_labels_are_refused_but_a_leading_wildcard_is_not() {
        for raw in [
            "..",
            ".example.com",
            "example.com.",
            "a..b",
            "*",
            "*.",
            "*.*.example.com",
            "a.*.com",
        ] {
            let err = validate_cert_domain_list(raw).unwrap_err();
            assert_eq!(err.status(), StatusCode::BAD_REQUEST, "{raw:?}");
        }
        assert_eq!(
            validate_cert_domain_list("*.example.com").unwrap(),
            "*.example.com"
        );
    }

    /// R-03: the stored form is trimmed and lower-cased, and a legitimate
    /// hostname still passes untouched.
    #[test]
    fn a_valid_certificate_domain_is_normalised() {
        assert_eq!(
            validate_cert_domain_list("  Example.COM  ").unwrap(),
            "example.com"
        );
        assert_eq!(
            validate_cert_domain_list("*.Example.COM").unwrap(),
            "*.example.com"
        );
        assert_eq!(
            validate_cert_domain_list("a-b.c1.example.com").unwrap(),
            "a-b.c1.example.com"
        );
        assert_eq!(
            validate_cert_hostname("  Example.COM  ").unwrap(),
            "example.com"
        );
    }

    /// R-03 (High): the column holds the whole hostname list — `a.com,b.com`
    /// is an existing, accepted input (`split_domains`,
    /// `ensure_acme_wildcard_supported`) — so the validator must split on `,`
    /// instead of treating the row as one hostname.
    #[test]
    fn a_certhostname_list_domain_is_accepted() {
        assert_eq!(
            validate_cert_domain_list("a.com,b.com").unwrap(),
            "a.com,b.com"
        );
        // Whitespace around a segment is cosmetic and must not reject the
        // list, nor survive into the stored value.
        assert_eq!(
            validate_cert_domain_list("a.com, b.com").unwrap(),
            "a.com,b.com"
        );
        assert_eq!(
            validate_cert_domain_list(" A.COM ,\tb.com\n").unwrap(),
            "a.com,b.com"
        );
        // A wildcard and an exact name in one certificate.
        assert_eq!(
            validate_cert_domain_list("*.example.com,api.example.com").unwrap(),
            "*.example.com,api.example.com"
        );
        // Input order is preserved, duplicates drop out.
        assert_eq!(
            validate_cert_domain_list("b.com,a.com,b.com").unwrap(),
            "b.com,a.com"
        );
        // An IDN in its A-label form is an ordinary hostname.
        assert_eq!(
            validate_cert_domain_list("xn--fiq228c.com").unwrap(),
            "xn--fiq228c.com"
        );
    }

    /// R-03 (High): a realistic certificate for five hostnames exceeds the old
    /// 255-character limit and must still be accepted — the bound lives on the
    /// column (TEXT, widened by m20240101_000021_site_alternate_domains), not
    /// on the validator.
    #[test]
    fn a_certhostname_list_domain_over_255_chars_is_accepted() {
        let hosts: Vec<String> = (1..=5)
            .map(|i| {
                format!("{i}.{}.production.example.com", "segment".repeat(5))
            })
            .collect();
        let list = hosts.join(",");
        assert!(list.len() > 255, "{list} is {} chars", list.len());
        assert_eq!(validate_cert_domain_list(&list).unwrap(), list);

        // Five long (but legal) hostnames: still well under the 4096 guard.
        let hosts: Vec<String> = (1..=5)
            .map(|i| format!("{i}{}.example.com", "a".repeat(50)))
            .collect();
        let joined = hosts.join(",");
        assert!(joined.len() > 255);
        assert_eq!(validate_cert_domain_list(&joined).unwrap(), joined);
    }

    /// R-03 (High): an empty segment is a mistake in the client's list, not
    /// something to silently skip — `a.com,,b.com` would otherwise store a
    /// shorter list than the caller asked for.
    #[test]
    fn an_empty_certhostname_list_segment_is_a_bad_request() {
        for raw in [
            "a.com,,b.com",
            "a.com,",
            ",a.com",
            ",,",
            "a.com, ,b.com",
            " , ",
        ] {
            let err = validate_cert_domain_list(raw).unwrap_err();
            assert_eq!(err.status(), StatusCode::BAD_REQUEST, "{raw:?}");
        }
    }

    /// R-03 (High): the anti-abuse bound on the whole list is 4096 bytes, so a
    /// caller cannot park an unbounded blob in a TEXT column.
    #[test]
    fn an_oversized_certhostname_list_is_a_bad_request() {
        let hosts: Vec<String> = (0..205)
            .map(|i| format!("host{i:03}.example.com"))
            .collect();
        let joined = hosts.join(",");
        // Just over the bound: the guard, not the hostnames, is what rejects.
        assert!(joined.len() > 4096, "{}", joined.len());
        assert!(joined.len() < 4096 + 32, "{}", joined.len());
        let err = validate_cert_domain_list(&joined).unwrap_err();
        assert_eq!(err.status(), StatusCode::BAD_REQUEST);

        // Just under the guard still passes, so the bound is the only limit.
        let hosts: Vec<String> = (0..200)
            .map(|i| format!("host{i:03}.example.com"))
            .collect();
        let joined = hosts.join(",");
        assert!(joined.len() < 4096 && joined.len() > 2048);
        assert_eq!(validate_cert_domain_list(&joined).unwrap(), joined);
    }

    /// R-03 (High): a malicious segment inside an otherwise legal list is
    /// refused, so the list form cannot be used to smuggle a path or a port
    /// past the single-hostname checks.
    #[test]
    fn a_bad_segment_inside_a_certhostname_list_is_a_bad_request() {
        for raw in [
            "a.com,evil.com/path",
            "a.com,evil.com:443",
            "a.com,exa mple.com",
            "a.com,evil.com\u{0}",
            "a.com,..",
            "a.com,*.*.evil.com",
            "a.com,evil.com,",
        ] {
            let err = validate_cert_domain_list(raw).unwrap_err();
            assert_eq!(err.status(), StatusCode::BAD_REQUEST, "{raw:?}");
        }
    }

    /* ── Binding: does the certificate cover the site? ─────────────── */

    fn uncovered_hosts(
        cert: &str,
        site: &str,
        alternate: &[&str],
    ) -> Vec<String> {
        let alternate: Vec<String> =
            alternate.iter().map(|d| (*d).to_string()).collect();
        uncovered_site_hostnames(cert, site, &alternate)
    }

    /// The gate on `POST /certificates/{id}/bindings`: every hostname the site
    /// answers for has to be covered, so the handshake cannot fail on a name the
    /// operator forgot.
    #[test]
    fn a_certificate_covering_the_site_hostnames_binds() {
        assert!(
            uncovered_hosts("a.example.com", "a.example.com", &[]).is_empty()
        );
        // Alternates are part of the site's hostname set.
        assert!(uncovered_hosts(
            "a.example.com,www.example.com",
            "a.example.com",
            &["www.example.com"]
        )
        .is_empty());
        // A certificate wildcard covers one label under it (TLS standard).
        assert!(
            uncovered_hosts("*.example.com", "a.example.com", &[]).is_empty()
        );
        // A site wildcard serves any depth, mirroring the data plane.
        assert!(
            uncovered_hosts("a.b.example.com", "*.example.com", &[]).is_empty()
        );
    }

    /// ... and a certificate issued for somewhere else is refused, naming the
    /// hostnames it cannot validate. The primary hostname comes first, which is
    /// the one the operator has to fix.
    #[test]
    fn an_uncovered_site_hostname_is_reported_for_a_bind() {
        assert_eq!(
            uncovered_hosts("other.com", "a.example.com", &[]),
            vec!["a.example.com".to_string()]
        );
        assert_eq!(
            uncovered_hosts(
                "a.example.com",
                "a.example.com",
                &["b.example.com", "c.example.com"]
            ),
            vec!["b.example.com".to_string(), "c.example.com".to_string()]
        );
    }

    /// The direction that would have been wrong: a certificate naming *extra*
    /// hostnames is only an advisory warning on the upload path, never a bind
    /// failure. One PEM with SANs for three sites has to bind to each of them.
    #[test]
    fn extra_certificate_hostnames_do_not_block_a_bind() {
        assert!(uncovered_hosts(
            "a.example.com,b.example.com",
            "a.example.com",
            &[]
        )
        .is_empty());
        // ... while the same certificate is still "uncovered" in the warning
        // direction, which is why the two functions are not interchangeable.
        assert_eq!(
            warnings_for("a.example.com,b.example.com", "a.example.com", &[]),
            vec!["b.example.com".to_string()]
        );
    }

    /// Uploaded certificates that nothing serves yet have no edge report to
    /// correct their status, so the write records what the dates say.
    #[test]
    fn an_unbound_upload_gets_its_status_from_its_validity() {
        let now = Utc::now();
        assert_eq!(
            uploaded_cert_status(Some(now + chrono::Duration::days(1)), now),
            cert_status::ACTIVE
        );
        // No expiry recorded: nothing says it lapsed.
        assert_eq!(uploaded_cert_status(None, now), cert_status::ACTIVE);
        // Boundary: expiring exactly now is expired.
        assert_eq!(uploaded_cert_status(Some(now), now), cert_status::EXPIRED);
        assert_eq!(
            uploaded_cert_status(Some(now - chrono::Duration::days(1)), now),
            cert_status::EXPIRED
        );
    }

    /// `bound_sites` / `bound_count` are filled from one index, and every
    /// response starts empty rather than absent.
    #[test]
    fn bindings_are_copied_onto_the_response() {
        let bound_id = Uuid::new_v4();
        let mut bound = BoundSites::new();
        bound.insert(
            bound_id,
            vec![
                BoundSiteRef {
                    id: Uuid::new_v4(),
                    domain: "a.example.com".to_string(),
                },
                BoundSiteRef {
                    id: Uuid::new_v4(),
                    domain: "b.example.com".to_string(),
                },
            ],
        );

        let owners = HashMap::from([(bound_id, None)]);
        let mut response =
            CertificateResponse::from(cert_model(bound_id, None));
        assert_eq!(response.bound_count, 0);
        assert!(response.bound_sites.is_empty());

        attach_bindings(&mut response, &bound, &owners);
        assert_eq!(response.bound_count, 2);
        assert_eq!(response.bound_sites[1].domain, "b.example.com");

        // An unbound certificate keeps the empty pair, so a client can tell
        // "checked, nothing bound" from a missing field.
        let mut unbound =
            CertificateResponse::from(cert_model(Uuid::new_v4(), None));
        attach_bindings(&mut unbound, &bound, &owners);
        assert_eq!(unbound.bound_count, 0);
        assert!(unbound.bound_sites.is_empty());
    }

    /// An owner's link row pointing at itself is not a share.
    /// `m20240101_000085_site_certificate_links` backfilled one for every site
    /// that had already selected its certificate, so a certificate nobody else
    /// uses still carries a row. Counting it made the console report "shared
    /// with 1 site" and name the owner — the demo hit exactly that
    /// (`mirror.shede.cn`, `bound_count: 1` for zero shares).
    #[test]
    fn an_owner_self_link_does_not_block_deletion() {
        let owner = Uuid::new_v4();
        let other = Uuid::new_v4();
        assert!(is_owner_self_link(owner, Some(owner)));
        assert!(!is_owner_self_link(other, Some(owner)));
        assert!(!is_owner_self_link(other, None));
    }

    #[test]
    fn owner_self_link_is_not_counted_as_a_share() {
        let owner = Uuid::new_v4();
        let cert_id = Uuid::new_v4();
        let mut bound = BoundSites::new();
        bound.insert(
            cert_id,
            vec![BoundSiteRef {
                id: owner,
                domain: "mirror.shede.cn".to_string(),
            }],
        );
        let owners = HashMap::from([(cert_id, Some(owner))]);

        let mut response =
            CertificateResponse::from(cert_model(cert_id, Some(owner)));
        attach_bindings(&mut response, &bound, &owners);

        assert_eq!(response.bound_count, 0);
        assert!(response.bound_sites.is_empty());
        // The owner relation itself is untouched: it is `site_id`, and the
        // row is still in `site_certificate_links`.
        assert_eq!(response.site_id, Some(owner));
    }

    /// The filter drops the owner only: a genuine share next to it survives,
    /// so the fix cannot hide real grants.
    #[test]
    fn a_real_share_is_still_counted() {
        let owner = Uuid::new_v4();
        let shared = Uuid::new_v4();
        let cert_id = Uuid::new_v4();
        let mut bound = BoundSites::new();
        bound.insert(
            cert_id,
            vec![
                BoundSiteRef {
                    id: owner,
                    domain: "mirror.shede.cn".to_string(),
                },
                BoundSiteRef {
                    id: shared,
                    domain: "www.shede.cn".to_string(),
                },
            ],
        );
        let owners = HashMap::from([(cert_id, Some(owner))]);

        let mut response =
            CertificateResponse::from(cert_model(cert_id, Some(owner)));
        attach_bindings(&mut response, &bound, &owners);

        assert_eq!(response.bound_count, 1);
        assert_eq!(
            response.bound_sites,
            vec![BoundSiteRef {
                id: shared,
                domain: "www.shede.cn".to_string(),
            }]
        );
    }

    /// A certificate model carrying no material beyond what these assertions
    /// read; `owner` is `site_certificates.site_id`.
    fn cert_model(id: Uuid, owner: Option<Uuid>) -> site_certificates::Model {
        site_certificates::Model {
            id,
            site_id: owner,
            domain: "a.example.com".to_string(),
            cert_pem: None,
            key_pem: None,
            issuer: None,
            not_before: None,
            expires_at: None,
            auto_renew: true,
            acme_email: None,
            acme_challenge_type: acme_challenge::HTTP_01.to_string(),
            acme_dns_provider: None,
            acme_dns_config: None,
            status: cert_status::ACTIVE.to_string(),
            failure_reason: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /* ── Certificate / site hostname mismatch warnings ─────────────── */

    fn warnings_for(cert: &str, site: &str, alternate: &[&str]) -> Vec<String> {
        let alternate: Vec<String> =
            alternate.iter().map(|d| (*d).to_string()).collect();
        uncovered_certificate_domains(cert, site, &alternate)
    }

    /// A certificate whose hostnames are a subset of the site's must not warn:
    /// this is the overwhelmingly common case and a false positive here would
    /// train operators to ignore the field.
    #[test]
    fn a_certificate_inside_the_site_hostnames_warns_about_nothing() {
        assert!(
            warnings_for("api.example.com", "api.example.com", &[]).is_empty()
        );
        assert!(warnings_for(
            "api.example.com,www.example.com",
            "api.example.com",
            &["www.example.com"]
        )
        .is_empty());
        // Same name on both sides, in any order.
        assert!(warnings_for(
            "www.example.com,api.example.com",
            "api.example.com",
            &["www.example.com"]
        )
        .is_empty());
    }

    /// A certificate covering a hostname the site does not serve is reported
    /// but never rejected — the caller saves it and decides.
    #[test]
    fn a_superset_certificate_warns_about_each_extra_hostname() {
        assert_eq!(
            warnings_for(
                "api.example.com,api.other.com",
                "api.example.com",
                &[]
            ),
            vec!["api.other.com".to_string()]
        );
        // Certificate order is preserved, duplicates collapse.
        assert_eq!(
            warnings_for(
                "b.other.com,a.other.com,b.other.com",
                "api.example.com",
                &[]
            ),
            vec!["b.other.com".to_string(), "a.other.com".to_string()]
        );
    }

    /// Certificate wildcards cover exactly one label (TLS standard);
    /// site wildcards cover any number of labels, matching the agent's
    /// data-plane rule lookup.
    #[test]
    fn a_wildcard_covers_exactly_one_label_in_both_directions() {
        // Certificate wildcard, concrete site hostname: covered.
        assert!(warnings_for("*.example.com", "a.example.com", &[]).is_empty());
        // ... but not the parent itself, and not a deeper name (single label).
        assert_eq!(
            warnings_for("*.example.com", "example.com", &[]),
            vec!["*.example.com".to_string()]
        );
        assert_eq!(
            warnings_for("*.example.com", "a.b.example.com", &[]),
            vec!["*.example.com".to_string()]
        );
        // Site wildcard, concrete certificate hostname: covered at any
        // label depth, mirroring lookup_with_wildcard on the data plane.
        assert!(warnings_for("a.example.com", "*.example.com", &[]).is_empty());
        assert!(
            warnings_for("a.b.example.com", "*.example.com", &[]).is_empty()
        );
        assert!(warnings_for("*.example.com", "*.example.com", &[]).is_empty());
        // ... but the site wildcard does not serve its own parent.
        assert_eq!(
            warnings_for("example.com", "*.example.com", &[]),
            vec!["example.com".to_string()]
        );
        // A different parent is never covered.
        assert_eq!(
            warnings_for("*.example.com", "a.other.com", &[]),
            vec!["*.example.com".to_string()]
        );
        // Alternate hostnames widen the site side just like the primary one.
        assert!(warnings_for(
            "api.example.com",
            "other.com",
            &["*.example.com"]
        )
        .is_empty());
    }

    /// Hostnames are compared case-insensitively: a certificate stored in
    /// mixed case must not produce a warning against a lower-case site.
    #[test]
    fn hostname_comparison_is_case_insensitive() {
        assert!(
            warnings_for("API.Example.COM", "api.example.com", &[]).is_empty()
        );
        assert!(
            warnings_for("api.example.com", "API.EXAMPLE.COM", &[]).is_empty()
        );
        assert!(warnings_for("*.Example.com", "a.example.com", &[]).is_empty());
        // A real mismatch still reports the normalised (lower-case) name.
        assert_eq!(
            warnings_for("API.Other.COM", "api.example.com", &[]),
            vec!["api.other.com".to_string()]
        );
    }

    /* ── H3: warnings on the read path ─────────────────────────────── */

    /// Builds the batch index the list endpoints pass in, without a database.
    fn served_index(entries: &[(Uuid, &str, &[&str])]) -> SiteHostnames {
        entries
            .iter()
            .map(|(id, domain, alternate)| {
                (
                    *id,
                    (
                        (*domain).to_string(),
                        alternate.iter().map(|d| (*d).to_string()).collect(),
                    ),
                )
            })
            .collect()
    }

    /// H3: the list/show read path used to leave `warnings` at `None`, so the
    /// dashboard badge never rendered on a page load — only right after a save.
    /// A superset certificate must now come back with the warning populated.
    #[test]
    fn read_path_warnings_are_populated_for_a_superset_certificate() {
        let site_id = Uuid::new_v4();
        let served = served_index(&[(site_id, "api.example.com", &[])]);
        assert_eq!(
            certificate_warnings_for(
                &served,
                site_id,
                "api.example.com,api.other.com"
            ),
            vec![
                "certificate domain api.other.com is not served by this site"
                    .to_string()
            ]
        );
    }

    /// ... and stays empty (not null) for the common case, so a client can
    /// tell "checked, nothing wrong" from "not checked".
    #[test]
    fn read_path_warnings_are_empty_for_a_subset_certificate() {
        let site_id = Uuid::new_v4();
        let served =
            served_index(&[(site_id, "api.example.com", &["www.example.com"])]);
        assert!(certificate_warnings_for(
            &served,
            site_id,
            "api.example.com,www.example.com"
        )
        .is_empty());
    }

    /// A multi-site page is served by one index: every certificate is checked
    /// against **its own** site's hostnames, never a neighbour's.
    #[test]
    fn read_path_warnings_use_the_certificate_own_site() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let served = served_index(&[
            (a, "a.example.com", &[]),
            (b, "b.example.com", &["alt.example.com"]),
        ]);
        // Correct on site A, wrong on site B — the same hostname flips.
        assert!(
            certificate_warnings_for(&served, a, "a.example.com").is_empty()
        );
        assert_eq!(
            certificate_warnings_for(&served, b, "a.example.com"),
            vec![
                "certificate domain a.example.com is not served by this site"
                    .to_string()
            ]
        );
        assert!(
            certificate_warnings_for(&served, b, "alt.example.com").is_empty()
        );
    }

    /// A certificate whose site is missing from the index (deleted between the
    /// paging query and the lookup) must not panic and must not invent a
    /// warning out of an empty hostname set.
    #[test]
    fn read_path_warnings_do_not_panic_for_an_unknown_site() {
        let site_id = Uuid::new_v4();
        let served = served_index(&[]);
        assert!(
            certificate_warnings_for(&served, site_id, "api.example.com")
                .is_empty()
        );
        // ... nor for a multi-site list where only some sites resolved.
        let known = Uuid::new_v4();
        let served = served_index(&[(known, "a.example.com", &[])]);
        assert!(certificate_warnings_for(&served, known, "a.example.com")
            .is_empty());
        assert!(certificate_warnings_for(&served, site_id, "a.example.com")
            .is_empty());
    }

    /// `split_domains` (not a single-hostname parse) feeds the comparison, so
    /// a comma-separated certificate list is checked hostname by hostname.
    #[test]
    fn a_certificate_list_is_checked_hostname_by_hostname() {
        assert_eq!(
            warnings_for(
                "api.example.com, api.other.com ,www.other.com",
                "api.example.com",
                &[]
            ),
            vec!["api.other.com".to_string(), "www.other.com".to_string()]
        );
    }

    /* ── `GET /certificates/{id}/usage` ────────────────────────────── */

    /// Builds the `(domain, name)` index `visible_sites` returns. The site name
    /// is irrelevant to the usage answer, so it mirrors the domain.
    fn visible_index(entries: &[(Uuid, &str)]) -> VisibleSites {
        entries
            .iter()
            .map(|(id, domain)| {
                (*id, ((*domain).to_string(), (*domain).to_string()))
            })
            .collect()
    }

    /// An unbound certificate with no grants and no site serving it is an
    /// empty answer — not a 404, and not a missing field. The console branches
    /// on `owner === null` plus two empty arrays, so the wire shape is part of
    /// the contract and is asserted literally.
    #[test]
    fn certificate_usage_of_an_unused_certificate_is_empty() {
        let usage = certificate_usage_snapshot(
            None,
            Vec::new(),
            Vec::new(),
            &visible_index(&[]),
        );
        assert!(usage.owner.is_none());
        assert!(usage.linked_sites.is_empty());
        assert!(usage.serving_sites.is_empty());
        assert_eq!(
            serde_json::to_string(&usage).unwrap(),
            r#"{"owner":null,"linked_sites":[],"serving_sites":[]}"#
        );
    }

    /// The owner's own link row is a duplicate of the owner relation, not a
    /// grant: migration 85 backfilled it for every site that had already
    /// selected its certificate, so leaving it in `linked_sites` offers the
    /// operator an "unlink" button for a share that does not exist.
    /// `serving_sites` keeps the owner — a site serving its own certificate is
    /// the normal case, and it is the delete blocker the dialog has to explain.
    #[test]
    fn usage_excludes_the_owner_self_link() {
        let owner = Uuid::new_v4();
        let shared = Uuid::new_v4();
        let sites = visible_index(&[
            (owner, "mirror.shede.cn"),
            (shared, "www.shede.cn"),
        ]);

        // `site_ssl` has the owner selected — the state migration 85 turned
        // into the self-link row.
        let usage = certificate_usage_snapshot(
            Some(owner),
            vec![owner, shared],
            vec![owner],
            &sites,
        );

        assert_eq!(
            usage.linked_sites,
            vec![LinkedSiteRef {
                id: shared,
                domain: "www.shede.cn".to_string(),
                selected: false,
            }]
        );
        assert_eq!(
            usage.serving_sites,
            vec![BoundSiteRef {
                id: owner,
                domain: "mirror.shede.cn".to_string(),
            }]
        );
        // The owner is still reported as the owner.
        assert_eq!(
            usage.owner,
            Some(BoundSiteRef {
                id: owner,
                domain: "mirror.shede.cn".to_string(),
            })
        );
    }

    /// Nothing but the self-link: the certificate the demo reported as shared
    /// with one site is shared with none, and the owner relation is what is
    /// left. Guards the empty case against the filter turning into one that
    /// also drops real grants (see `a_real_share_is_still_counted` for the
    /// other direction).
    #[test]
    fn usage_without_links_is_empty() {
        let owner = Uuid::new_v4();
        let usage = certificate_usage_snapshot(
            Some(owner),
            vec![owner],
            Vec::new(),
            &visible_index(&[(owner, "mirror.shede.cn")]),
        );
        assert!(usage.linked_sites.is_empty());
        assert!(usage.serving_sites.is_empty());
        assert_eq!(
            usage.owner,
            Some(BoundSiteRef {
                id: owner,
                domain: "mirror.shede.cn".to_string(),
            })
        );
    }

    /// `selected` is `site_ssl.certificate_id == cert` — exactly the state
    /// `DELETE /certificates/{id}/bindings/{site}` and
    /// `DELETE /certificates/{id}` refuse with a `409` — so it must come from
    /// `site_ssl`, not from the presence of a link row.
    #[test]
    fn certificate_usage_marks_only_the_serving_link_as_selected() {
        let owner = Uuid::new_v4();
        let shared = Uuid::new_v4();
        let idle = Uuid::new_v4();
        let sites = visible_index(&[
            (owner, "a.example.com"),
            (shared, "b.example.com"),
            (idle, "c.example.com"),
        ]);

        let usage = certificate_usage_snapshot(
            Some(owner),
            vec![idle, shared],
            vec![shared],
            &sites,
        );

        assert_eq!(
            usage.owner,
            Some(BoundSiteRef {
                id: owner,
                domain: "a.example.com".to_string(),
            })
        );
        assert_eq!(
            usage.linked_sites,
            vec![
                LinkedSiteRef {
                    id: shared,
                    domain: "b.example.com".to_string(),
                    selected: true,
                },
                LinkedSiteRef {
                    id: idle,
                    domain: "c.example.com".to_string(),
                    selected: false,
                },
            ]
        );
        // The same data, filtered on `selected`.
        assert_eq!(
            usage.serving_sites,
            vec![BoundSiteRef {
                id: shared,
                domain: "b.example.com".to_string(),
            }]
        );
    }

    /// A site can select a certificate whose grant row was never written (its
    /// own certificate via `PUT /sites/{id}/ssl-settings`), and that site
    /// still blocks deletion. Deriving `serving_sites` from the link table
    /// would report "nobody uses it" for exactly the case the delete dialog
    /// has to explain.
    #[test]
    fn certificate_usage_reports_a_serving_site_without_a_link_row() {
        let owner = Uuid::new_v4();
        let shared = Uuid::new_v4();
        let sites = visible_index(&[
            (owner, "a.example.com"),
            (shared, "b.example.com"),
        ]);

        let usage = certificate_usage_snapshot(
            Some(owner),
            Vec::new(),
            vec![owner],
            &sites,
        );
        assert!(usage.linked_sites.is_empty());
        assert_eq!(
            usage.serving_sites,
            vec![BoundSiteRef {
                id: owner,
                domain: "a.example.com".to_string(),
            }]
        );

        // ... and the mirrored case: a grant with no selection is linked only.
        // The grant has to belong to *another* site: the owner's own link row
        // is not a share (`usage_excludes_the_owner_self_link`).
        let usage = certificate_usage_snapshot(
            Some(owner),
            vec![shared],
            Vec::new(),
            &sites,
        );
        assert_eq!(
            usage.linked_sites,
            vec![LinkedSiteRef {
                id: shared,
                domain: "b.example.com".to_string(),
                selected: false,
            }]
        );
        assert!(usage.serving_sites.is_empty());
    }

    /// An owner outside the caller's visible set degrades to `null` instead of
    /// failing the read (or 404ing a certificate that is perfectly readable)
    /// and instead of inventing a domain-less entry the console cannot render.
    #[test]
    fn certificate_usage_ignores_an_owner_site_outside_the_visible_set() {
        let owner = Uuid::new_v4();
        let usage = certificate_usage_snapshot(
            Some(owner),
            vec![owner],
            vec![owner],
            &visible_index(&[]),
        );
        assert!(usage.owner.is_none());
        assert!(usage.linked_sites.is_empty());
        assert!(usage.serving_sites.is_empty());
    }

    /// Both arrays are ordered by domain so two identical requests return the
    /// same body: `site_certificate_links` has no natural order and `site_ssl`
    /// is only unique per site.
    #[test]
    fn certificate_usage_is_ordered_by_domain() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let sites = visible_index(&[
            (a, "z.example.com"),
            (b, "a.example.com"),
            (c, "m.example.com"),
        ]);

        let usage =
            certificate_usage_snapshot(None, vec![a, b, c], vec![c, a], &sites);
        let linked: Vec<&str> = usage
            .linked_sites
            .iter()
            .map(|s| s.domain.as_str())
            .collect();
        assert_eq!(
            linked,
            vec!["a.example.com", "m.example.com", "z.example.com"]
        );
        let serving: Vec<&str> = usage
            .serving_sites
            .iter()
            .map(|s| s.domain.as_str())
            .collect();
        assert_eq!(serving, vec!["m.example.com", "z.example.com"]);
    }

    /* ── `GET /sites/{id}/certificates/available` filter ───────────── */

    /// Renders the filter as the `site_certificates` query would run it.
    /// Values are inlined as literals by the Postgres builder, so the clauses
    /// can be asserted without depending on placeholder numbering.
    fn available_condition_sql(
        site_id: Uuid,
        visible: Vec<Uuid>,
        linked: Vec<Uuid>,
    ) -> String {
        use sea_orm::sea_query::{Alias, PostgresQueryBuilder, Query};

        Query::select()
            .column((Alias::new("site_certificates"), Alias::new("id")))
            .from(Alias::new("site_certificates"))
            .cond_where(available_certificate_condition(
                site_id, visible, linked,
            ))
            .to_string(PostgresQueryBuilder)
    }

    /// The three origins of a certificate row, and which of them this endpoint
    /// may offer. The site's own is already in its own list, an unbound upload
    /// is attached through `POST /certificates/{id}/bindings`, and a grant
    /// this site already holds is not "available" any more — only another
    /// site's unlinked certificate is.
    #[test]
    fn available_condition_excludes_own_unbound_and_linked_certificates() {
        let site = Uuid::new_v4();
        let other = Uuid::new_v4();
        let linked = Uuid::new_v4();

        let sql =
            available_condition_sql(site, vec![site, other], vec![linked]);

        // Own certificates are dropped ...
        assert!(
            sql.contains(&format!(
                "\"site_certificates\".\"site_id\" <> '{site}'"
            )),
            "{sql}"
        );
        // ... rows visible to the caller only ...
        assert!(
            sql.contains(&format!(
                "\"site_certificates\".\"site_id\" IN ('{site}', '{other}')"
            )),
            "{sql}"
        );
        // ... and an already-held grant is not offered a second time.
        assert!(
            sql.contains(&format!(
                "\"site_certificates\".\"id\" NOT IN ('{linked}')"
            )),
            "{sql}"
        );
        // Unbound uploads (`site_id IS NULL`) stay out. Written explicitly:
        // `NULL <> x` and `NULL IN (…)` are NULL, so the two clauses above
        // already exclude them, and this is the clause that keeps that true
        // when either of them is rewritten.
        assert!(
            sql.contains("\"site_certificates\".\"site_id\" IS NOT NULL"),
            "{sql}"
        );
    }

    /// With no grants yet the `NOT IN` clause is omitted entirely — an
    /// `IN ()`-shaped fragment is how an "empty list" turns into a query that
    /// matches nothing.
    #[test]
    fn available_condition_omits_the_not_in_clause_without_links() {
        let site = Uuid::new_v4();
        let sql = available_condition_sql(site, vec![site], Vec::new());
        assert!(!sql.contains("NOT IN"), "{sql}");
        assert!(
            sql.contains("\"site_certificates\".\"site_id\" IS NOT NULL"),
            "{sql}"
        );
    }
}
