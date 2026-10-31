//! Access-log filtering for static assets.
//!
//! Adds the two columns of `log_retention_settings` that steer the data
//! plane's static-asset access-log skip: the master switch and the
//! user-editable suffix list. Both carry defaults so existing deployments
//! start with the feature on (the setting's documented default) without any
//! backfill; requests that triggered a security rule are never skipped,
//! regardless of these values.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Raw SQL: a Postgres array literal default is awkward to express
        // through the schema builder.
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE log_retention_settings \
                 ADD COLUMN IF NOT EXISTS skip_static_access_logs \
                 boolean NOT NULL DEFAULT true",
            )
            .await?;
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE log_retention_settings \
                 ADD COLUMN IF NOT EXISTS static_suffixes text[] NOT NULL \
                 DEFAULT '{css,js,mjs,map,png,jpg,jpeg,gif,webp,avif,svg,ico,\
 bmp,woff,woff2,ttf,otf,eot}'::text[]",
            )
            .await
            .map(|_| ())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE log_retention_settings \
                 DROP COLUMN IF EXISTS static_suffixes, \
                 DROP COLUMN IF EXISTS skip_static_access_logs",
            )
            .await
            .map(|_| ())
    }
}
