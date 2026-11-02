//! Reverse lookup from a certificate to the sites that still select it.
//!
//! `site_ssl.certificate_id` (added in `m20240101_000007`) was never indexed:
//! the table is keyed on `site_id` and every write path arrives from the site
//! side, so that column was only ever written, never searched. Two read paths
//! do search it, by certificate:
//!
//! 1. `GET /api/v1/certificates/{cert_id}/usage` (`api/ssl.rs`) — the
//!    `selected` flag and the serving list behind the bind/unbind/delete
//!    dialog, run on every dialog open;
//! 2. `delete_certificate_row` (`api/ssl.rs`) — the first delete blocker,
//!    "is any site still selecting this certificate".
//!
//! Both are equality predicates on that one column, which a plain b-tree on
//! `certificate_id` serves; without it each lookup is a sequential scan over
//! every site's TLS row — `site_ssl` holds one row per site, so that scan
//! grows with the fleet while the answer usually has a handful of rows.
//!
//! **Partial — `WHERE certificate_id IS NOT NULL` — because NULL is the
//! common value**: the column is nullable, and a site that uploaded its own
//! PEM, runs ACME or falls back to a self-signed certificate never selects a
//! `site_certificates` row. A NULL row can never satisfy `col = $1`, so
//! leaving those rows out keeps the index proportional to the sites that
//! actually share a certificate instead of to every site in the fleet. The
//! clause stays usable from an equality on the same column, because
//! `certificate_id = $1` is strict: a NULL row cannot match, so it implies the
//! index predicate and the planner still picks this index.
//!
//! `CREATE INDEX` (not `CONCURRENTLY`) is in line with the rest of this
//! migration set: it runs inside the migrator's transaction. PostgreSQL-only,
//! like every other migration here.

use sea_orm_migration::prelude::*;

/// The index this migration owns; `api/ssl.rs` names it in the comments above
/// the two `site_ssl` lookups that ride it.
pub const INDEX_NAME: &str = "idx_site_ssl_certificate_id";

/// One column, one predicate. The partial clause is the point of the
/// definition — dropping it back to a full index would index every site that
/// selects no certificate at all.
pub const CREATE_SQL: &str = "CREATE INDEX IF NOT EXISTS \
     idx_site_ssl_certificate_id \
     ON site_ssl (certificate_id) \
     WHERE certificate_id IS NOT NULL";

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(CREATE_SQL)
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(&format!("DROP INDEX IF EXISTS {INDEX_NAME}"))
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The DDL is the contract with the two queries in `api/ssl.rs`: equality
    /// on `certificate_id` and nothing else. Indexing a different column, or
    /// losing the partial clause, silently restores the sequential scan this
    /// migration exists to remove.
    #[test]
    fn index_ddl_matches_the_queries_it_serves() {
        assert!(CREATE_SQL.contains(INDEX_NAME), "{CREATE_SQL}");
        assert!(CREATE_SQL.contains("IF NOT EXISTS"), "{CREATE_SQL}");
        assert!(
            CREATE_SQL.contains("ON site_ssl (certificate_id)"),
            "{CREATE_SQL}"
        );
        // The partial clause keeps NULL rows (sites with no selected
        // certificate) out of the index; `down` must be able to drop exactly
        // what `up` created, hence the shared `INDEX_NAME`.
        assert!(
            CREATE_SQL.contains("WHERE certificate_id IS NOT NULL"),
            "{CREATE_SQL}"
        );
        // `CONCURRENTLY` cannot run inside the migrator's transaction.
        assert!(!CREATE_SQL.contains("CONCURRENTLY"), "{CREATE_SQL}");
    }
}
