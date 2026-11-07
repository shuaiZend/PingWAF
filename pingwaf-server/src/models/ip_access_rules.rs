//! `ip_access_rules` — per-site IP allow/block/challenge rules.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize,
)]
#[sea_orm(table_name = "ip_access_rules")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub site_id: Uuid,
    /// When set, the rule targets the referenced IP group instead of
    /// enumerating `ip_ranges` itself.
    pub group_id: Option<Uuid>,
    pub name: String,
    pub ip_ranges: Vec<String>,
    pub action: String,
    pub note: Option<String>,
    pub enabled: bool,
    /// Empty = the whole site; otherwise the rule only applies when the
    /// request path matches `match_type`.
    #[serde(default)]
    pub path: String,
    /// `prefix` (default), `exact`, `wildcard` or `regex`.
    #[serde(default = "default_match_type")]
    pub match_type: String,
    pub priority: i32,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

fn default_match_type() -> String {
    "prefix".to_string()
}
