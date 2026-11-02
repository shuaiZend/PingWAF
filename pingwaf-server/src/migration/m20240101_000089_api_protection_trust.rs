//! Reverse-proxy trust for the control plane listener.
//!
//! The 9080 access log and WAF used the TCP peer address only, which records
//! the proxy instead of the client when the console sits behind a load
//! balancer. These four columns let an operator name the trusted proxy ranges
//! and the forwarded header to read.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for sql in [
            "ALTER TABLE api_protection_settings \
             ADD COLUMN IF NOT EXISTS trust_proxy_headers boolean NOT NULL DEFAULT false",
            "ALTER TABLE api_protection_settings \
             ADD COLUMN IF NOT EXISTS trusted_header varchar(64) NOT NULL DEFAULT 'x-forwarded-for'",
            "ALTER TABLE api_protection_settings \
             ADD COLUMN IF NOT EXISTS trust_last_hop boolean NOT NULL DEFAULT true",
            "ALTER TABLE api_protection_settings \
             ADD COLUMN IF NOT EXISTS trusted_proxy_ranges text[] NOT NULL DEFAULT '{}'::text[]",
        ] {
            manager.get_connection().execute_unprepared(sql).await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for sql in [
            "ALTER TABLE api_protection_settings DROP COLUMN IF EXISTS trusted_proxy_ranges",
            "ALTER TABLE api_protection_settings DROP COLUMN IF EXISTS trust_last_hop",
            "ALTER TABLE api_protection_settings DROP COLUMN IF EXISTS trusted_header",
            "ALTER TABLE api_protection_settings DROP COLUMN IF EXISTS trust_proxy_headers",
        ] {
            manager.get_connection().execute_unprepared(sql).await?;
        }
        Ok(())
    }
}
