//! Record the transport peer of an agent's heartbeat stream.
//!
//! `ip_address` is what the agent reports about itself and `public_ip` is what
//! its own egress probe guessed; neither is authoritative when the agent sits
//! behind NAT or reports a stale interface list. The address the control plane
//! actually accepted the TCP connection from ("the four-layer peer") is the one
//! an operator needs when they have to reach the node or explain why it looks
//! offline.
//!
//! `peer_ip_seen_at` is refreshed with the address so the console can show how
//! current the observation is: a peer recorded days ago says nothing about the
//! connection in front of it now.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Agents::Table)
                    .add_column(
                        ColumnDef::new(Agents::PeerIp).string_len(45).null(),
                    )
                    .add_column(
                        ColumnDef::new(Agents::PeerIpSeenAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Agents::Table)
                    .drop_column(Agents::PeerIpSeenAt)
                    .drop_column(Agents::PeerIp)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Agents {
    Table,
    PeerIp,
    PeerIpSeenAt,
}
