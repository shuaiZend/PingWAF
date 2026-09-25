//! Database schema migrations.
//!
//! The migrations are applied at startup by [`crate::start_server`] through
//! [`Migrator::up`]; `sea-orm-migration` records the applied versions in the
//! `seaql_migrations` table, so restarting the control plane is a no-op once the
//! schema is current.

pub mod m20240101_000001_create_users;
pub mod m20240101_000002_create_sites;
pub mod m20240101_000003_create_rules;
pub mod m20240101_000004_create_agents;
pub mod m20240101_000005_create_logs;
pub mod m20240101_000006_create_additional_tables;

use sea_orm_migration::prelude::*;

/// Ordered list of every schema change the control plane knows about.
pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20240101_000001_create_users::Migration),
            Box::new(m20240101_000002_create_sites::Migration),
            Box::new(m20240101_000003_create_rules::Migration),
            Box::new(m20240101_000004_create_agents::Migration),
            Box::new(m20240101_000005_create_logs::Migration),
            Box::new(m20240101_000006_create_additional_tables::Migration),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_ordered_and_unique() {
        let migrations = Migrator::migrations();
        assert_eq!(migrations.len(), 6);
        let names: Vec<String> = migrations.iter().map(|m| m.name().to_owned()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "migrations must be listed in name order");
        let unique: std::collections::BTreeSet<&String> = names.iter().collect();
        assert_eq!(unique.len(), names.len(), "duplicate migration name");
    }
}
