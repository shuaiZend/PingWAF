//! Adds the `scope` column to `error_pages`.
//!
//! `site` (the default) keeps the historic behaviour: a deployment-wide page
//! that replaces error responses for every matched site. `unmatched_host`
//! marks pages served by the edge when no location matches the request's
//! Host — the fallback page rendered by the `fallback_page` plugin behind the
//! generated `pingwaf_unmatched_host` location.
//!
//! Uniqueness moves from `status_code` alone to `(scope, status_code)`: the
//! two scopes serve disjoint request paths, so a 404 fallback and a 404 site
//! page can coexist.
//!
//! `down` drops the `scope` rows that would collide with the restored
//! `(status_code)` unique index (everything not `site`), because the old
//! schema had no way to represent them; the remaining rows are untouched.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(ErrorPages::Table)
                    .add_column(
                        ColumnDef::new(ErrorPages::Scope)
                            .string_len(20)
                            .not_null()
                            .default("site"),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx_error_pages_status")
                    .table(ErrorPages::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .unique()
                    .name("idx_error_pages_scope_status")
                    .table(ErrorPages::Table)
                    .col(ErrorPages::Scope)
                    .col(ErrorPages::StatusCode)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Non-`site` rows cannot be represented by the old unique index
        // (`status_code` alone), so they are removed before the index comes
        // back. This is the only lossy step and it only touches rows created
        // by the new feature.
        let db = manager.get_connection();
        db.execute_unprepared("DELETE FROM error_pages WHERE scope <> 'site'")
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx_error_pages_scope_status")
                    .table(ErrorPages::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .unique()
                    .name("idx_error_pages_status")
                    .table(ErrorPages::Table)
                    .col(ErrorPages::StatusCode)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(ErrorPages::Table)
                    .drop_column(ErrorPages::Scope)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum ErrorPages {
    Table,
    Scope,
    StatusCode,
}
