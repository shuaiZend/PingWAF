//! `error_pages` — the global custom error page templates.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Page applies to every matched site (historic behaviour).
pub const SCOPE_SITE: &str = "site";
/// Page is served when no location matches the request's Host.
pub const SCOPE_UNMATCHED_HOST: &str = "unmatched_host";

#[derive(
    Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize,
)]
#[sea_orm(table_name = "error_pages")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// The only page per `(scope, status_code)` pair.
    pub status_code: i32,
    /// `site` (default) replaces error responses for matched sites;
    /// `unmatched_host` is served when no location matches the Host.
    pub scope: String,
    pub name: String,
    pub content_type: String,
    #[sea_orm(column_type = "Text")]
    pub body_template: String,
    pub enabled: bool,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
