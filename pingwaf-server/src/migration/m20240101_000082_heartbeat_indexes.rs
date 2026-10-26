//! Indexes for heartbeat retention sweeps and liveness queries.
//!
//! The heartbeat path is read/write hot; these indexes keep the periodic
//! retention DELETEs and the stale-pending certificate sweep from scanning
//! whole tables.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const UP_SQL: &[&str] = &[
    "CREATE INDEX IF NOT EXISTS idx_host_samples_sampled_at \
     ON host_samples (sampled_at)",
    "CREATE INDEX IF NOT EXISTS idx_agent_metrics_recorded_at \
     ON agent_metrics (recorded_at)",
    "CREATE INDEX IF NOT EXISTS idx_site_certificates_stale_pending \
     ON site_certificates (updated_at) \
     WHERE status = 'pending' AND cert_pem IS NULL AND acme_email IS NOT NULL",
    "CREATE INDEX IF NOT EXISTS idx_agents_status_heartbeat \
     ON agents (status, last_heartbeat)",
];

const DOWN_SQL: &[&str] = &[
    "DROP INDEX IF EXISTS idx_agents_status_heartbeat",
    "DROP INDEX IF EXISTS idx_site_certificates_stale_pending",
    "DROP INDEX IF EXISTS idx_agent_metrics_recorded_at",
    "DROP INDEX IF EXISTS idx_host_samples_sampled_at",
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for sql in UP_SQL {
            manager.get_connection().execute_unprepared(sql).await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for sql in DOWN_SQL {
            manager.get_connection().execute_unprepared(sql).await?;
        }
        Ok(())
    }
}
