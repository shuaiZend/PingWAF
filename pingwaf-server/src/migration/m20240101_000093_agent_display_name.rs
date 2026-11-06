//! Adds an editable display name to agent rows.
//!
//! An agent's identity is the hostname it reports at registration, and the
//! registration path matches on `hostname + api_key_id` to reclaim its row.
//! Renaming `hostname` would therefore orphan the row and create a duplicate on
//! the next heartbeat, so operators get a separate, nullable label instead:
//! empty means "show the hostname", which is what the console did before.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Agents::Table)
                    .add_column(
                        ColumnDef::new(Agents::DisplayName)
                            .string_len(100)
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
                    .table(Agents::Table)
                    .drop_column(Agents::DisplayName)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Agents {
    Table,
    DisplayName,
}
