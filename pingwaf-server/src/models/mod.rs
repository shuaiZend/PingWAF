//! SeaORM entities for every table created by [`crate::migration`].
//!
//! Each table lives in an inner module named after the table (the shape
//! `DeriveEntityModel` generates: `Entity`, `Model`, `ActiveModel`, `Column`,
//! `Relation`). The inner modules are re-exported here under their singular
//! names so call sites read as `models::site::Entity` instead of
//! `models::sites::sites::Entity`.

pub mod agents;
pub mod bot_protection;
pub mod certificates;
pub mod challenge_settings;
pub mod error_pages;
pub mod geo_rules;
pub mod ip_access_rules;
pub mod rate_limit_stats;
pub mod rewrite_rules;
pub mod rules;
pub mod security_events;
pub mod sites;
pub mod users;

pub use agents::{agent_status, agents as agent};
pub use certificates::site_certificates;
pub use rules::{
    action, cache_rules, characteristic, mode, rate_limit_rules, rule_groups, rules as rule,
};
pub use security_events::{access_logs as access_log, security_events as security_event};
pub use sites::{acme_challenge, site_ssl, site_status, site_upstreams, sites as site};
pub use users::{api_keys as api_key, permission, role, users as user};

/// Convenience alias used across the API and gRPC layers.
pub type DbErr = sea_orm::DbErr;
