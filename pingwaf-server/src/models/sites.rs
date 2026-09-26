//! Entities describing a protected site: the site row itself, its origin
//! servers and its TLS material.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Lifecycle of a site.
pub mod site_status {
    pub const ACTIVE: &str = "active";
    pub const PAUSED: &str = "paused";
    pub const PENDING: &str = "pending";

    pub fn is_valid(status: &str) -> bool {
        matches!(status, ACTIVE | PAUSED | PENDING)
    }
}

/// `sites` — one protected domain.
pub mod sites {
    use super::*;

    #[derive(
        Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize,
    )]
    #[sea_orm(table_name = "sites")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub user_id: Uuid,
        pub name: String,
        #[sea_orm(unique)]
        pub domain: String,
        pub status: String,
        pub plan: String,
        /// Disk budget, in MiB, the agents may use for this site's cache.
        pub cache_quota_mb: i32,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `site_upstreams` — origin servers the agents load balance across.
pub mod site_upstreams {
    use super::*;

    #[derive(
        Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize,
    )]
    #[sea_orm(table_name = "site_upstreams")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub site_id: Uuid,
        pub name: String,
        pub address: String,
        pub weight: i32,
        pub tls: bool,
        pub health_status: String,
        pub created_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `site_ssl` — uploaded certificate material or ACME settings.
///
/// `key_pem` is a private key; it is serialised nowhere (the API strips it) and
/// only leaves the control plane inside the gRPC config pushed to agents.
pub mod site_ssl {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "site_ssl")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        #[sea_orm(unique)]
        pub site_id: Uuid,
        pub cert_pem: Option<String>,
        #[sea_orm(column_type = "Text")]
        pub key_pem: Option<String>,
        pub issuer: Option<String>,
        pub domain: String,
        pub expires_at: Option<DateTimeUtc>,
        pub auto_renew: bool,
        pub acme_email: Option<String>,
        pub acme_challenge_type: Option<String>,
        pub acme_dns_provider: Option<String>,
        #[sea_orm(column_type = "JsonBinary")]
        pub acme_dns_config: Option<Json>,
        /// Whether the domain is served over HTTPS at all.
        pub https_enabled: bool,
        pub min_tls_version: String,
        /// `None` means "up to whatever the agent's TLS stack supports".
        pub max_tls_version: Option<String>,
        /// Fall back to a generated certificate when none is configured.
        pub self_signed: bool,
        /// Certificate picked from `site_certificates` for this site.
        pub certificate_id: Option<Uuid>,
        pub mtls_enabled: bool,
        #[sea_orm(column_type = "Text")]
        pub mtls_client_ca: Option<String>,
        pub hsts_enabled: bool,
        pub hsts_max_age: i32,
        pub always_use_https: bool,
        pub created_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// ACME challenge types accepted in `site_ssl.acme_challenge_type`.
pub mod acme_challenge {
    pub const HTTP_01: &str = "http-01";
    pub const DNS_01: &str = "dns-01";

    pub fn is_valid(challenge: &str) -> bool {
        matches!(challenge, HTTP_01 | DNS_01)
    }
}

/// TLS protocol versions accepted in `site_ssl.min_tls_version` and
/// `site_ssl.max_tls_version`.
pub mod tls_version {
    pub const TLS_10: &str = "1.0";
    pub const TLS_11: &str = "1.1";
    pub const TLS_12: &str = "1.2";
    pub const TLS_13: &str = "1.3";

    pub const ALL: [&str; 4] = [TLS_10, TLS_11, TLS_12, TLS_13];

    pub fn is_valid(value: &str) -> bool {
        ALL.contains(&value)
    }

    /// Position in `ALL`, used to check that a lower bound is not above the
    /// upper bound.
    pub fn rank(value: &str) -> Option<usize> {
        ALL.iter().position(|item| *item == value)
    }

    /// Canonicalises `TLSv1.2`, `tls1.2` and `1.2` to `1.2`.
    pub fn normalise(value: &str) -> Option<String> {
        let lower = value.trim().to_ascii_lowercase();
        let stripped = lower
            .strip_prefix("tlsv")
            .or_else(|| lower.strip_prefix("tls"))
            .unwrap_or(&lower);
        is_valid(stripped).then(|| stripped.to_string())
    }
}
