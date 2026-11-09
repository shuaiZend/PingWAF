//! Moves existing `allow` rules off `mode = 'off'`.
//!
//! `off` removes a rule from the engine, so an allow rule (the whitelist
//! exemption itself) disappeared and later block rules started matching
//! again. The API now rejects that pair and the console only offers monitor;
//! this backfill keeps rows saved before the change working without a manual
//! edit.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "UPDATE rules SET mode = 'monitor' \
                 WHERE action = 'allow' AND mode = 'off'",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Which rows were 'off' before the backfill was not recorded; putting
        // them back would reintroduce the broken whitelist, so rollback leaves
        // them on monitor.
        Ok(())
    }
}
