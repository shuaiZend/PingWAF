//! Adds response-based counting to rate limit rules.
//!
//! Existing rules retain request counting by default.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(RateLimitRules::Table)
                    .add_column(
                        ColumnDef::new(RateLimitRules::CountingMode)
                            .string()
                            .not_null()
                            .default("request"),
                    )
                    .add_column(
                        ColumnDef::new(RateLimitRules::ResponseStatusCodes)
                            .json()
                            .not_null()
                            .default("[]"),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(RateLimitRules::Table)
                    .drop_column(RateLimitRules::ResponseStatusCodes)
                    .drop_column(RateLimitRules::CountingMode)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum RateLimitRules {
    Table,
    CountingMode,
    ResponseStatusCodes,
}
