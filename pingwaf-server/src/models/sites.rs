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
