//! Schema smoke test: run every migration, then read every model.
//!
//! A migration and the model it backs are written by hand, in different files,
//! and nothing in the unit suite executes SQL. 0.33.0 shipped with a model that
//! selected `agents.tunnel_listener_error` while its migration never created the
//! column: every query touching the table failed, the console answered 500 and
//! no agent could register. This test is the missing gate — it runs the real
//! migrator against a real PostgreSQL, then issues one query per entity, so a
//! column the migrations never created fails loudly and names the table.
//!
//! It is `#[ignore]`d because it needs a database. Run it against a *throwaway*
//! one (the migrator writes to it):
//!
//! ```text
//! PINGWAF_TEST_DB_URL=postgres://pingwaf:pingwaf@127.0.0.1:5432/pingwaf_schema \
//!   cargo test -p pingwaf-server --test schema_matches_models -- --ignored --nocapture
//! ```
//!
//! `scripts/tunnel-smoke.sh` runs it as part of its setup.

use pingwaf_server::migration::Migrator;
use sea_orm::{Database, EntityTrait, QuerySelect};
use sea_orm_migration::MigratorTrait;

/// Reads one row from every entity: any column the migrations did not create
/// makes the query fail at prepare time.
macro_rules! probe {
    ($db:expr, $($entity:path),+ $(,)?) => {
        $(
            if let Err(err) = <$entity>::find().limit(1).all($db).await {
                panic!(
                    "{} does not match the migrated schema: {err}",
                    stringify!($entity)
                );
            }
        )+
    };
}

async fn connected() -> sea_orm::DatabaseConnection {
    let url = std::env::var("PINGWAF_TEST_DB_URL")
        .expect("set PINGWAF_TEST_DB_URL to a throwaway PostgreSQL database");
    let db = Database::connect(&url)
        .await
        .expect("connect to PINGWAF_TEST_DB_URL");
    Migrator::up(&db, None).await.expect("run the migrations");
    db
}

#[tokio::test]
#[ignore = "needs PINGWAF_TEST_DB_URL pointing at a throwaway PostgreSQL"]
async fn every_model_matches_the_migrated_schema() {
    // One test, not two: a second test would run the migrator concurrently
    // against the same empty database and collide on `seaql_migrations`.
    let db = connected().await;

    verify_response_rate_migration(&db).await;

    probe!(
        &db,
        pingwaf_server::models::agent_blocked_ip::Entity,
        pingwaf_server::models::agent_metric::Entity,
        pingwaf_server::models::agent::Entity,
        pingwaf_server::models::ai_conversation::Entity,
        pingwaf_server::models::ai_message::Entity,
        pingwaf_server::models::ai_setting::Entity,
        pingwaf_server::models::site_ai_bot_policies::Entity,
        pingwaf_server::models::site_ai_crawl_settings::Entity,
        pingwaf_server::models::api_protection_setting::Entity,
        pingwaf_server::models::control_plane_access_log::Entity,
        pingwaf_server::models::site_certificates::Entity,
        pingwaf_server::models::config_backup_target::Entity,
        pingwaf_server::models::config_backup::Entity,
        pingwaf_server::models::config_version::Entity,
        pingwaf_server::models::control_plane_boot::Entity,
        pingwaf_server::models::control_plane_stop::Entity,
        pingwaf_server::models::control_plane_certificate::Entity,
        pingwaf_server::models::host_sample::Entity,
        pingwaf_server::models::instance_setting::Entity,
        pingwaf_server::models::login_history::Entity,
        pingwaf_server::models::mtls_ca::Entity,
        pingwaf_server::models::mtls_client_certificate::Entity,
        pingwaf_server::models::notification_channel::Entity,
        pingwaf_server::models::notification_event::Entity,
        pingwaf_server::models::oidc_provider::Entity,
        pingwaf_server::models::passkey_credential::Entity,
        pingwaf_server::models::passkey_state::Entity,
        pingwaf_server::models::refresh_token::Entity,
        pingwaf_server::models::cache_rules::Entity,
        pingwaf_server::models::rate_limit_rules::Entity,
        pingwaf_server::models::rule_groups::Entity,
        pingwaf_server::models::rule::Entity,
        pingwaf_server::models::access_log::Entity,
        pingwaf_server::models::security_event::Entity,
        pingwaf_server::models::site_visitor::Entity,
        pingwaf_server::models::site_routes::Entity,
        pingwaf_server::models::site_ssl::Entity,
        pingwaf_server::models::site_upstream_pools::Entity,
        pingwaf_server::models::site_upstreams::Entity,
        pingwaf_server::models::site::Entity,
        pingwaf_server::models::tunnel_connector::Entity,
        pingwaf_server::models::tunnel_session::Entity,
        pingwaf_server::models::tunnel_setting::Entity,
        pingwaf_server::models::api_key::Entity,
        pingwaf_server::models::user::Entity,
        pingwaf_server::models::visitor_auth_state::Entity,
        pingwaf_server::models::visitor_passkey_state::Entity,
        pingwaf_server::models::visitor_passkey::Entity,
        pingwaf_server::models::visitor_site_grant::Entity,
    );

    // The specific regression 0.33.0 shipped: a model column no migration
    // created. Asserted by name so the failure says which column and why.
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let row = db
        .query_one(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT column_name FROM information_schema.columns \
             WHERE table_name = 'agents' AND column_name = 'tunnel_listener_error'"
                .to_string(),
        ))
        .await
        .expect("read information_schema");
    assert!(
        row.is_some(),
        "agents.tunnel_listener_error is missing: the 0.33.0 regression \
         (a model column no migration creates) has come back"
    );
}

/// Execute migration 111 against an isolated temporary table,
/// using the actual migration implementation.
async fn verify_response_rate_migration(db: &sea_orm::DatabaseConnection) {
    use sea_orm::{
        ConnectionTrait, DatabaseBackend, Statement, TransactionTrait,
    };
    use sea_orm_migration::{MigrationTrait, SchemaManager};

    let migrations = Migrator::migrations();

    let migration = migrations
        .iter()
        .find(|m| m.name().contains("000111_rate_limit_response_counting"))
        .expect("response counting migration is registered");

    let txn = db.begin().await.unwrap();

    txn.execute_unprepared(
        "CREATE TEMP TABLE rate_limit_rules (
            id integer PRIMARY KEY,
            name text NOT NULL,
            enabled boolean NOT NULL DEFAULT TRUE
        ) ON COMMIT DROP",
    )
    .await
    .unwrap();

    txn.execute_unprepared(
        "INSERT INTO rate_limit_rules (id, name, enabled)
         VALUES
            (1, 'existing-enabled-request', TRUE),
            (2, 'existing-disabled-request', FALSE)",
    )
    .await
    .unwrap();

    let manager = SchemaManager::new(&txn);

    // Apply the real migration.
    migration.up(&manager).await.unwrap();

    let rows = txn
        .query_all(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT id, counting_mode,
                response_status_codes::text AS statuses,
                enabled
         FROM rate_limit_rules
         ORDER BY id"
                .to_string(),
        ))
        .await
        .unwrap();

    assert_eq!(rows.len(), 2);

    for row in &rows {
        let mode: String = row.try_get("", "counting_mode").unwrap();

        let statuses: String = row.try_get("", "statuses").unwrap();

        assert_eq!(mode, "request");
        assert_eq!(statuses, "[]");
    }

    assert!(rows[0].try_get::<bool>("", "enabled").unwrap());
    assert!(!rows[1].try_get::<bool>("", "enabled").unwrap());

    println!("Migration 111 preserves existing request rules");

    // Insert response rules and an unknown counting mode.
    txn.execute_unprepared(
        "INSERT INTO rate_limit_rules
            (id, name, enabled,
             counting_mode, response_status_codes)
         VALUES
            (3, 'enabled-response', TRUE,
                'response', '[404]'),
            (4, 'disabled-response', FALSE,
                'response', '[403,404]'),
            (5, 'unknown-mode', TRUE,
                'unknown', '[]')",
    )
    .await
    .unwrap();

    // Execute the actual rollback migration.
    migration.down(&manager).await.unwrap();

    let rows = txn
        .query_all(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT id, enabled
         FROM rate_limit_rules
         ORDER BY id"
                .to_string(),
        ))
        .await
        .unwrap();

    assert_eq!(rows.len(), 5);

    let expected = [(1, true), (2, false), (3, false), (4, false), (5, false)];

    for (row, (expected_id, expected_enabled)) in rows.iter().zip(expected) {
        let id: i32 = row.try_get("", "id").unwrap();

        let enabled: bool = row.try_get("", "enabled").unwrap();

        assert_eq!(id, expected_id);
        assert_eq!(
            enabled, expected_enabled,
            "unexpected enabled state after rollback for rule {id}"
        );
    }

    println!("Migration 111 rollback safely disables response rules");

    // Verify that the migration can be reapplied.
    migration.up(&manager).await.unwrap();

    let rows = txn
        .query_all(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT id, enabled, counting_mode
         FROM rate_limit_rules
         ORDER BY id"
                .to_string(),
        ))
        .await
        .unwrap();

    for row in &rows {
        assert_eq!(
            row.try_get::<String>("", "counting_mode").unwrap(),
            "request"
        );
    }

    for (row, (_, expected_enabled)) in rows.iter().zip(expected) {
        assert_eq!(
            row.try_get::<bool>("", "enabled").unwrap(),
            expected_enabled
        );
    }

    println!("Migration 111 reapplies without reactivating response rules");

    txn.rollback().await.unwrap();
}
