//! Certificate sharing: a certificate row may be used by more than one site.
//!
//! `site_certificates.site_id` stays the *owner* (it drives config-version
//! scoping, backup/restore layering and ACME bookkeeping). This table is the
//! authorisation layer: a site can select a certificate when it owns it or when
//! a link row exists. No backfill of the owner relation is needed — owners stay
//! visible through `site_id` — but the currently selected certificate of every
//! existing site is linked explicitly so the relation survives a later change
//! of ownership semantics and shows up in the site's "shared" list.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS site_certificate_links (\
                 site_id uuid NOT NULL REFERENCES sites(id) ON DELETE CASCADE, \
                 certificate_id uuid NOT NULL, \
                 linked_at timestamptz NOT NULL DEFAULT now(), \
                 PRIMARY KEY (site_id, certificate_id)\
             )",
        )
        .await?;

        // Reverse lookup: "which sites use this certificate" without scanning.
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_site_certificate_links_certificate_id \
             ON site_certificate_links (certificate_id)",
        )
        .await?;

        // Idempotent backfill of the certificate each site has already chosen.
        db.execute_unprepared(
            "INSERT INTO site_certificate_links (site_id, certificate_id) \
             SELECT site_id, certificate_id FROM site_ssl \
             WHERE certificate_id IS NOT NULL \
             ON CONFLICT DO NOTHING",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE IF EXISTS site_certificate_links")
            .await?;
        Ok(())
    }
}
