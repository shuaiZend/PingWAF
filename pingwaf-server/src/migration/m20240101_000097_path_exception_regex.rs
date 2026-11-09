//! Allows `regex` path exceptions in the database.
//!
//! Migration 000086 created `site_path_exceptions.match_type` with a CHECK
//! limited to prefix/exact/wildcard. The API, proto, console and data plane
//! all support regex, so saving one failed the constraint and surfaced as a
//! generic 500. Widen the constraint to the full wire vocabulary.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        conn.execute_unprepared(
            "ALTER TABLE site_path_exceptions \
             DROP CONSTRAINT IF EXISTS ck_site_path_exceptions_match_type",
        )
        .await?;
        conn.execute_unprepared(
            "ALTER TABLE site_path_exceptions \
             ADD CONSTRAINT ck_site_path_exceptions_match_type \
             CHECK (match_type IN ('prefix', 'exact', 'wildcard', 'regex'))",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Rows saved with `regex` would violate the old constraint, so the
        // narrower CHECK cannot be restored safely. The wider constraint is
        // backward compatible with every row the old schema accepted.
        Ok(())
    }
}
