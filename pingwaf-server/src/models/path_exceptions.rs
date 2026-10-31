//! `site_path_exceptions` — per-path whitelist that skips selected protection
//! layers. See `api::path_exceptions` for the validation rules and the data
//! plane contract (`RuleBundle.path_exceptions`).

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize,
)]
#[sea_orm(table_name = "site_path_exceptions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub site_id: Uuid,
    pub path: String,
    /// `prefix` (default), `exact` or `wildcard` (trailing `*` only).
    pub match_type: String,
    /// Layer tokens: `*`, `waf`, `bot`, `access`, `basic_auth`, `mtls`,
    /// `ai_bot`, `rate_limit`, `challenge`.
    pub skip_layers: Vec<String>,
    pub note: Option<String>,
    /// Write a `security_events` row on every hit. Off by default: a health
    /// probe would otherwise flood the event table.
    pub log_hits: bool,
    pub enabled: bool,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
