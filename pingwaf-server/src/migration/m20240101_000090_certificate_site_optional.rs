//! Certificates can exist without an owning site.
//!
//! An uploaded certificate used to be created through
//! `POST /sites/{id}/certificates`, so `site_certificates.site_id` was never
//! null: the row *was* the binding. That forced an operator to pick a site (and
//! a matching domain) before uploading, which is backwards when the PEM is
//! already at hand — the certificate has no intrinsic site, and the same
//! certificate may later be served by several sites
//! (`site_certificate_links`).
//!
//! Dropping NOT NULL lets the upload land first and be bound afterwards. The
//! existing rows keep their site, so every query that filters on `site_id`
//! behaves exactly as before; an unbound row simply never matches, which is
//! what "not served anywhere yet" means.
//!
//! The `down` migration refuses to run while unbound certificates exist rather
//! than deleting them: a rollback should not destroy PEM material the operator
//! uploaded and has not bound yet.
//!
//! PostgreSQL-only, like the rest of this migration set.

use sea_orm_migration::prelude::*;

/// Re-adding NOT NULL is only possible when every row has an owner; refuse
/// (instead of deleting) when an unbound certificate would be lost.
const REFUSE_UNBOUND_SQL: &str = "DO $$ BEGIN \
     IF EXISTS (SELECT 1 FROM site_certificates WHERE site_id IS NULL) THEN \
         RAISE EXCEPTION \
             'cannot restore NOT NULL: unbind-free certificates exist; bind or delete them first'; \
     END IF; \
     END $$";

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(SiteCertificates::Table)
                    .modify_column(
                        ColumnDef::new(SiteCertificates::SiteId).uuid().null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(REFUSE_UNBOUND_SQL)
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(SiteCertificates::Table)
                    .modify_column(
                        ColumnDef::new(SiteCertificates::SiteId)
                            .uuid()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum SiteCertificates {
    Table,
    SiteId,
}
