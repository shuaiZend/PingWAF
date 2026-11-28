//! Per-site image optimization, served by the `image_optim` plugin.
//!
//! The data plane already has the plugin (`pingap-imageoptim`); what was
//! missing is a per-site policy to switch it on and tune it. The policy is one
//! nullable JSONB document on the site row — the same shape `cache_type_policy`
//! uses — because it is read and written as a unit and never queried by field.
//!
//! NULL means "never configured": the data plane then leaves responses
//! untouched. An object with `enabled = false` is a deliberate off, which the
//! console shows as a switched-off card rather than an unconfigured one.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Sites::Table)
                    .add_column(
                        ColumnDef::new(Sites::ImageOptimization).json_binary(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Sites::Table)
                    .drop_column(Sites::ImageOptimization)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Sites {
    Table,
    ImageOptimization,
}
