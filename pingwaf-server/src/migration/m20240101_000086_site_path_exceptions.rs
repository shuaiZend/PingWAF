//! Path exceptions: a site may whitelist request paths that skip selected
//! protection layers (WAF, bot protection, access rules, ...).
//!
//! `skip_layers` holds short tokens (`*`, `waf`, `bot`, `access`, `basic_auth`,
//! `mtls`, `ai_bot`, `rate_limit`, `challenge`) rather than one column per
//! layer: the data plane already dispatches on a token string, and adding a
//! layer later must not require a migration.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS site_path_exceptions (\
                 id uuid PRIMARY KEY, \
                 site_id uuid NOT NULL REFERENCES sites(id) ON DELETE CASCADE, \
                 path varchar(512) NOT NULL, \
                 match_type varchar(10) NOT NULL DEFAULT 'prefix', \
                 skip_layers text[] NOT NULL DEFAULT '{}', \
                 note varchar(200), \
                 log_hits boolean NOT NULL DEFAULT false, \
                 enabled boolean NOT NULL DEFAULT true, \
                 created_at timestamptz NOT NULL DEFAULT now(), \
                 updated_at timestamptz NOT NULL DEFAULT now(), \
                 CONSTRAINT uq_site_path_exceptions_site_path UNIQUE (site_id, path), \
                 CONSTRAINT ck_site_path_exceptions_match_type \
                     CHECK (match_type IN ('prefix', 'exact', 'wildcard'))\
             )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_site_path_exceptions_site_id \
             ON site_path_exceptions (site_id)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE IF EXISTS site_path_exceptions")
            .await?;
        Ok(())
    }
}
