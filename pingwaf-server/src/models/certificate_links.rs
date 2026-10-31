//! `site_certificate_links` — many-to-many association that lets a site select
//! a certificate it does not own (certificate sharing).
//!
//! `site_certificates.site_id` stays the owner; this table is the usage grant.
//! Composite primary key, like `ip_group_sites`: a link has no identity of its
//! own and must be idempotent.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize,
)]
#[sea_orm(table_name = "site_certificate_links")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub site_id: Uuid,
    #[sea_orm(primary_key, auto_increment = false)]
    pub certificate_id: Uuid,
    pub linked_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
