//! Adds an explicit exception expression to rate-limit rules.
//!
//! `expression` has always been an *inclusion* filter: a request must match it
//! before the rule counts. Operators reasonably read the UI's single expression
//! field as an exclusion ("do not count /api"), which made the rule count only
//! `/api`. This column is evaluated first: when it matches, the rule is skipped
//! entirely for that request. Keeping it separate avoids changing the meaning
//! of every existing rule.

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
                        ColumnDef::new(RateLimitRules::ExceptionExpression)
                            .text()
                            .not_null()
                            .default(""),
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
                    .drop_column(RateLimitRules::ExceptionExpression)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum RateLimitRules {
    Table,
    ExceptionExpression,
}
