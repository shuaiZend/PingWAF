//! Entities holding the security policy of a site: WAF rule groups and rules,
//! rate limit rules and cache rules.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Actions a rule may take, mirroring `pingwaf::WafAction` in the protocol.
pub mod action {
    pub const BLOCK: &str = "block";
    pub const LOG: &str = "log";
    pub const CHALLENGE: &str = "challenge";
    pub const JS_CHALLENGE: &str = "js_challenge";
    pub const ALLOW: &str = "allow";

    pub const ALL: [&str; 5] = [BLOCK, LOG, CHALLENGE, JS_CHALLENGE, ALLOW];

    pub fn is_valid(action: &str) -> bool {
        ALL.contains(&action)
    }

    /// `pingwaf::WafAction` enum values as defined in control_plane.proto.
    pub fn to_proto(action: &str) -> i32 {
        match action {
            BLOCK => 0,
            LOG => 1,
            CHALLENGE => 2,
            JS_CHALLENGE => 3,
            ALLOW => 4,
            _ => 0,
        }
    }
}

/// Evaluation mode of a rule: disabled, log only, or enforcing.
pub mod mode {
    pub const OFF: &str = "off";
    pub const MONITOR: &str = "monitor";
    pub const BLOCK: &str = "block";

    pub fn is_valid(mode: &str) -> bool {
        matches!(mode, OFF | MONITOR | BLOCK)
    }

    /// `pingwaf::WafMode` enum values as defined in control_plane.proto.
    pub fn to_proto(mode: &str) -> i32 {
        match mode {
            OFF => 0,
            MONITOR => 1,
            BLOCK => 2,
            _ => 0,
        }
    }
}

/// `rule_groups` — named, ordered collections of rules.
pub mod rule_groups {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "rule_groups")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub site_id: Uuid,
        pub name: String,
        pub phase: String,
        pub priority: i32,
        pub enabled: bool,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `rules` — individual WAF rules written in the PingWAF rules language.
pub mod rules {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "rules")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub group_id: Option<Uuid>,
        pub site_id: Uuid,
        pub name: String,
        pub description: Option<String>,
        pub expression: String,
        pub action: String,
        pub severity: i32,
        pub tags: Vec<String>,
        pub enabled: bool,
        pub mode: String,
        pub priority: i32,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `rate_limit_rules` — request counting and mitigation.
pub mod rate_limit_rules {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "rate_limit_rules")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub site_id: Uuid,
        pub name: String,
        pub expression: String,
        /// Values of `pingwaf::RateLimitCharacteristics`, e.g. `ip`, `path`.
        pub characteristics: Vec<String>,
        pub period_seconds: i32,
        pub threshold: i32,
        pub action: String,
        pub mitigation_timeout_seconds: i32,
        pub enabled: bool,
        pub priority: i32,
        pub created_at: DateTimeUtc,
        pub updated_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `cache_rules` — edge caching policy per site.
pub mod cache_rules {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
    #[sea_orm(table_name = "cache_rules")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub site_id: Uuid,
        pub name: String,
        pub match_expression: String,
        pub edge_ttl_seconds: i32,
        pub browser_ttl_seconds: i32,
        pub disk_quota_mb: i32,
        pub cache_eligible: bool,
        pub respect_origin: bool,
        pub enabled: bool,
        pub created_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// Counter keys accepted in `rate_limit_rules.characteristics`.
pub mod characteristic {
    pub const IP: &str = "ip";
    pub const IP_NAT: &str = "ip_nat";
    pub const HOST: &str = "host";
    pub const PATH: &str = "path";
    pub const HEADER: &str = "header";
    pub const COOKIE: &str = "cookie";
    pub const QUERY: &str = "query";
    pub const ASN: &str = "asn";
    pub const COUNTRY: &str = "country";
    pub const JA3: &str = "ja3";

    pub fn is_valid(value: &str) -> bool {
        matches!(
            value,
            IP | IP_NAT
                | HOST
                | PATH
                | HEADER
                | COOKIE
                | QUERY
                | ASN
                | COUNTRY
                | JA3
        )
    }

    /// `pingwaf::RateLimitCharacteristics` values from control_plane.proto.
    ///
    /// The proto enum uses `RATE_LIMIT_CHAR_*` names, which prost may or may not
    /// shorten when generating Rust variants, so the numbers are spelled out
    /// here instead of referencing the generated constants.
    pub fn to_proto(value: &str) -> i32 {
        match value {
            IP => 0,
            IP_NAT => 1,
            HOST => 2,
            PATH => 3,
            HEADER => 4,
            COOKIE => 5,
            QUERY => 6,
            ASN => 7,
            COUNTRY => 8,
            JA3 => 9,
            _ => 0,
        }
    }
}
