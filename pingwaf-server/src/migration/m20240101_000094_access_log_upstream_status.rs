//! Adds the origin status to access logs.
//!
//! `status_code` is the status the client received, which the edge may have
//! generated itself (a WAF block, a challenge, a cache hit). Operators
//! debugging an upstream need the status the origin returned, so it gets its
//! own column; when no origin was contacted the data plane repeats
//! `status_code`, which keeps the field present for every row.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(AccessLogs::Table)
                    .add_column(
                        ColumnDef::new(AccessLogs::UpstreamStatus)
                            .integer()
                            .null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(AccessLogs::Table)
                    .drop_column(AccessLogs::UpstreamStatus)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum AccessLogs {
    Table,
    UpstreamStatus,
}
