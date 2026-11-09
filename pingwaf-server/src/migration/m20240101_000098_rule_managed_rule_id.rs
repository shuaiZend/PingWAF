//! Add `managed_rule_id` to `rules`.
//!
//! The built-in WAF rules seeded for every site duplicate rules that the data
//! plane already loads as its managed set (`PINGWAF-*` in
//! `pingwaf-waf/src/rules/managed.rs`). Until now a seeded row only differed
//! from a custom rule by its *name*, and the gRPC layer's
//! `id.starts_with("PINGWAF-")` filter never fired because `rules.id` is a
//! UUID — so every site shipped the same detections twice: once from the
//! managed engine, once as a per-site custom rule that bypassed the managed
//! rule's category/stack monitor switches.
//!
//! The column stores the stable managed identifier (e.g. `PINGWAF-1010`) on
//! the seed row, which lets the config builder treat those rows as
//! *override-only*: they no longer travel as custom rules, and a rule switched
//! off genuinely disables the managed rule on that site.
//!
//! Backfill matches the historical seed names to managed ids. Seed rows were
//! idempotent on the name, so a name match is exactly the population this fix
//! needs; rows whose name does not appear in the map (operator-created rules,
//! renamed seeds) are left alone — an unmatched row keeps its current
//! custom-rule behaviour rather than being dropped or guessed at.

use sea_orm_migration::prelude::*;

/// Historical seed name → stable managed id.
///
/// Only pairs whose runtime semantics genuinely overlap are mapped. The seed
/// rules without a managed counterpart (命令注入检测, 路径穿越/文件包含检测,
/// SSRF 检测) have no `PINGWAF-*` equivalent and stay honest custom rules.
const BACKFILL: &[(&str, &str)] = &[
    ("SQL 注入检测", "PINGWAF-1002"),
    ("XSS 跨站脚本检测", "PINGWAF-1003"),
    ("恶意扫描器识别", "PINGWAF-1010"),
    ("敏感文件探测", "PINGWAF-1020"),
    ("备份文件探测", "PINGWAF-1021"),
    ("危险 HTTP 方法", "PINGWAF-1030"),
    ("Log4Shell 检测", "PINGWAF-1050"),
    ("超长 URI", "PINGWAF-1051"),
];

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Rules::Table)
                    .add_column(
                        ColumnDef::new(Rules::ManagedRuleId)
                            .string_len(64)
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        // Name-based backfill, guarded so it never overwrites a value a
        // concurrent path may already have written.
        let values: Vec<String> = BACKFILL
            .iter()
            .map(|(name, id)| {
                format!("('{}', '{}')", name.replace('\'', "''"), id)
            })
            .collect();
        let conn = manager.get_connection();
        conn.execute_unprepared(&format!(
            "UPDATE rules SET managed_rule_id = v.mid \
             FROM (VALUES {}) AS v(name, mid) \
             WHERE rules.name = v.name \
               AND rules.managed_rule_id IS NULL \
               AND EXISTS ( \
                   SELECT 1 FROM rule_groups g \
                   WHERE g.id = rules.group_id \
                     AND g.name = 'PingWAF 内置防护' \
                     AND g.phase = 'request' \
               )",
            values.join(", ")
        ))
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Rules::Table)
                    .drop_column(Rules::ManagedRuleId)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Rules {
    Table,
    ManagedRuleId,
}

#[cfg(test)]
mod tests {
    use super::BACKFILL;

    #[test]
    fn backfill_pairs_are_well_formed() {
        assert!(!BACKFILL.is_empty());
        let mut ids: Vec<&str> = BACKFILL.iter().map(|(_, id)| *id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), BACKFILL.len(), "duplicate managed id");
        for (name, id) in BACKFILL {
            assert!(!name.is_empty() && name.len() <= 200, "{name}");
            assert!(id.starts_with("PINGWAF-"), "{id}");
        }
    }
}
