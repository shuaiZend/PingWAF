//! Adds a path scope to the four access restrictions.
//!
//! An empty `path` keeps the historical behaviour: the rule/gate applies to
//! the whole site. A non-empty path limits it to requests matching
//! `match_type` (prefix/exact/wildcard/regex), using the same matcher as path
//! exceptions. Existing rows are not backfilled.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            AccessRestrictionTable::IpAccessRules,
            AccessRestrictionTable::GeoRules,
            AccessRestrictionTable::SiteBasicAuth,
            AccessRestrictionTable::SiteOidc,
        ] {
            manager
                .alter_table(
                    Table::alter()
                        .table(table)
                        .add_column(
                            ColumnDef::new(AccessRestriction::Path)
                                .text()
                                .not_null()
                                .default(""),
                        )
                        .add_column(
                            ColumnDef::new(AccessRestriction::MatchType)
                                .text()
                                .not_null()
                                .default("prefix"),
                        )
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            AccessRestrictionTable::IpAccessRules,
            AccessRestrictionTable::GeoRules,
            AccessRestrictionTable::SiteBasicAuth,
            AccessRestrictionTable::SiteOidc,
        ] {
            manager
                .alter_table(
                    Table::alter()
                        .table(table)
                        .drop_column(AccessRestriction::Path)
                        .drop_column(AccessRestriction::MatchType)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}

#[derive(DeriveIden)]
enum AccessRestrictionTable {
    IpAccessRules,
    GeoRules,
    SiteBasicAuth,
    SiteOidc,
}

#[derive(DeriveIden)]
enum AccessRestriction {
    Path,
    MatchType,
}
