//! Translates database rows into the protocol messages agents consume.
//!
//! Everything the agents need — WAF rules, rate limits, cache policy, upstreams
//! and TLS material — is assembled here, which keeps the gRPC service layer free
//! of SQL and lets the REST API reuse the exact same builder when it pushes a
//! configuration update to a connected agent.

use std::collections::HashMap;

use chrono::{DateTime, TimeZone, Utc};
use pingwaf_proto::control_plane::{
    CacheRule, ChallengeConfig, CustomErrorPage, GeoConfig, HeaderOperation, IpAccessRule,
    RateLimitRule, RewriteRule, RuleBundle, Site, SiteConfig, SslConfig, UpstreamConfig,
    UpstreamPeer, WafConfig, WafRule,
};
use prost::Message;
use prost_types::Timestamp;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use uuid::Uuid;

use crate::models::{
    acme_challenge, action, characteristic, challenge_settings, error_pages, geo_rules,
    ip_access_rules, cache_rules, mode, rate_limit_rules, rewrite_rules, rule, rule_groups,
    site, site_ssl, site_status, site_upstreams,
};
use crate::api::challenge::challenge_level;
use crate::api::ip_rules::ip_action;

/// `pingwaf.WafMode` values from control_plane.proto.
///
/// The numbers are spelled out instead of referencing the generated enum
/// variants: prost strips a value prefix that matches the enum name, and the
/// control plane should not depend on that naming rule.
const WAF_MODE_OFF: i32 = 0;
const WAF_MODE_MONITOR: i32 = 1;
const WAF_MODE_BLOCK: i32 = 2;

/// Converts a `DateTime<Utc>` into the protocol's timestamp.
pub fn to_timestamp(value: DateTime<Utc>) -> Option<Timestamp> {
    Some(Timestamp {
        seconds: value.timestamp(),
        nanos: value.timestamp_subsec_nanos() as i32,
    })
}

/// `to_timestamp` for "now".
pub fn now_timestamp() -> Option<Timestamp> {
    to_timestamp(Utc::now())
}

/// Converts a protocol timestamp back into a `DateTime<Utc>`, defaulting to the
/// current time when the agent omitted it.
pub fn from_timestamp(value: Option<&Timestamp>) -> DateTime<Utc> {
    match value {
        Some(ts) => Utc
            .timestamp_opt(ts.seconds, ts.nanos.max(0) as u32)
            .single()
            .unwrap_or_else(Utc::now),
        None => Utc::now(),
    }
}

/// Maps the persisted site status onto `pingwaf.SiteStatusEnum`.
///
/// The numeric values are used directly because prost's variant naming for
/// prefixed enum entries is not something the control plane should depend on.
pub fn site_status_proto(status: &str) -> i32 {
    match status {
        site_status::ACTIVE => 0, // SITE_STATUS_ACTIVE
        site_status::PAUSED => 1, // SITE_STATUS_PAUSED
        site_status::PENDING => 2, // SITE_STATUS_PENDING
        _ => 0,
    }
}

/// Maps the persisted ACME challenge type onto `pingwaf.AcmeChallengeType`.
pub fn acme_challenge_proto(challenge: Option<&str>) -> i32 {
    match challenge {
        Some(acme_challenge::DNS_01) => 1, // ACME_DNS_01
        _ => 0,                            // ACME_HTTP_01
    }
}

/// FNV-1a over 64 bits; used to fingerprint a configuration without pulling in
/// a hashing crate.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Stable 16-hex-character fingerprint of any protobuf message.
pub fn fingerprint<T: Message>(message: &T) -> String {
    let mut buf = Vec::with_capacity(message.encoded_len());
    // `encode` only fails when the buffer is too small, which cannot happen for
    // a `Vec` sized with `encoded_len()`.
    let _ = message.encode(&mut buf);
    format!("{:016x}", fnv1a64(&buf))
}

/// Flattens a JSON object into the `map<string, string>` shape the protocol
/// expects; non-object or non-string values are stringified.
fn json_to_string_map(value: &Option<serde_json::Value>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(serde_json::Value::Object(object)) = value {
        for (key, raw) in object {
            let text = match raw {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            map.insert(key.clone(), text);
        }
    }
    map
}

/// Loads the WAF rule rows of a site, ordered by priority then name so that the
/// fingerprint does not change between otherwise identical configurations.
async fn load_rules(
    db: &DatabaseConnection,
    site_id: Uuid,
) -> Result<Vec<rule::Model>, sea_orm::DbErr> {
    rule::Entity::find()
        .filter(rule::Column::SiteId.eq(site_id))
        .order_by_asc(rule::Column::Priority)
        .order_by_asc(rule::Column::Name)
        .all(db)
        .await
}

async fn load_rule_groups(
    db: &DatabaseConnection,
    site_id: Uuid,
) -> Result<Vec<rule_groups::Model>, sea_orm::DbErr> {
    rule_groups::Entity::find()
        .filter(rule_groups::Column::SiteId.eq(site_id))
        .order_by_asc(rule_groups::Column::Priority)
        .all(db)
        .await
}

async fn load_rate_limits(
    db: &DatabaseConnection,
    site_id: Uuid,
) -> Result<Vec<rate_limit_rules::Model>, sea_orm::DbErr> {
    rate_limit_rules::Entity::find()
        .filter(rate_limit_rules::Column::SiteId.eq(site_id))
        .order_by_asc(rate_limit_rules::Column::Priority)
        .all(db)
        .await
}

async fn load_cache_rules(
    db: &DatabaseConnection,
    site_id: Uuid,
) -> Result<Vec<cache_rules::Model>, sea_orm::DbErr> {
    cache_rules::Entity::find()
        .filter(cache_rules::Column::SiteId.eq(site_id))
        .all(db)
        .await
}

async fn load_upstreams(
    db: &DatabaseConnection,
    site_id: Uuid,
) -> Result<Vec<site_upstreams::Model>, sea_orm::DbErr> {
    site_upstreams::Entity::find()
        .filter(site_upstreams::Column::SiteId.eq(site_id))
        .order_by_desc(site_upstreams::Column::Weight)
        .all(db)
        .await
}

async fn load_ssl(
    db: &DatabaseConnection,
    site_id: Uuid,
) -> Result<Option<site_ssl::Model>, sea_orm::DbErr> {
    site_ssl::Entity::find()
        .filter(site_ssl::Column::SiteId.eq(site_id))
        .one(db)
        .await
}

async fn load_ip_access_rules(
    db: &DatabaseConnection,
    site_id: Uuid,
) -> Result<Vec<ip_access_rules::Model>, sea_orm::DbErr> {
    ip_access_rules::Entity::find()
        .filter(ip_access_rules::Column::SiteId.eq(site_id))
        .filter(ip_access_rules::Column::Enabled.eq(true))
        .order_by_asc(ip_access_rules::Column::Priority)
        .all(db)
        .await
}

async fn load_geo_rules(
    db: &DatabaseConnection,
    site_id: Uuid,
) -> Result<Option<geo_rules::Model>, sea_orm::DbErr> {
    geo_rules::Entity::find()
        .filter(geo_rules::Column::SiteId.eq(site_id))
        .one(db)
        .await
}

async fn load_challenge_settings(
    db: &DatabaseConnection,
    site_id: Uuid,
) -> Result<Option<challenge_settings::Model>, sea_orm::DbErr> {
    challenge_settings::Entity::find()
        .filter(challenge_settings::Column::SiteId.eq(site_id))
        .one(db)
        .await
}

async fn load_rewrite_rules(
    db: &DatabaseConnection,
    site_id: Uuid,
) -> Result<Vec<rewrite_rules::Model>, sea_orm::DbErr> {
    rewrite_rules::Entity::find()
        .filter(rewrite_rules::Column::SiteId.eq(site_id))
        .filter(rewrite_rules::Column::Enabled.eq(true))
        .order_by_asc(rewrite_rules::Column::Priority)
        .all(db)
        .await
}

async fn load_error_pages(
    db: &DatabaseConnection,
    site_id: Uuid,
) -> Result<Vec<error_pages::Model>, sea_orm::DbErr> {
    error_pages::Entity::find()
        .filter(error_pages::Column::SiteId.eq(site_id))
        .filter(error_pages::Column::Enabled.eq(true))
        .order_by_asc(error_pages::Column::StatusCode)
        .all(db)
        .await
}

/// Turns a stored WAF rule into its protocol representation.
pub fn rule_to_proto(row: &rule::Model) -> WafRule {
    WafRule {
        id: row.id.to_string(),
        name: row.name.clone(),
        description: row.description.clone().unwrap_or_default(),
        expression: row.expression.clone(),
        action: action::to_proto(&row.action),
        severity: row.severity.max(0) as u32,
        tags: row.tags.clone(),
        enabled: row.enabled,
        mode: mode::to_proto(&row.mode),
        priority: row.priority.max(0) as u32,
    }
}

fn rate_limit_to_proto(row: &rate_limit_rules::Model) -> RateLimitRule {
    RateLimitRule {
        id: row.id.to_string(),
        name: row.name.clone(),
        expression: row.expression.clone(),
        characteristics: row
            .characteristics
            .iter()
            .map(|value| characteristic::to_proto(value))
            .collect(),
        period_seconds: row.period_seconds.max(0) as u32,
        threshold: row.threshold.max(0) as u32,
        action: action::to_proto(&row.action),
        mitigation_timeout_seconds: row.mitigation_timeout_seconds.max(0) as u32,
        enabled: row.enabled,
        priority: row.priority.max(0) as u32,
    }
}

fn cache_rule_to_proto(row: &cache_rules::Model) -> CacheRule {
    CacheRule {
        id: row.id.to_string(),
        name: row.name.clone(),
        match_expression: row.match_expression.clone(),
        edge_ttl_seconds: row.edge_ttl_seconds.max(0) as u32,
        browser_ttl_seconds: row.browser_ttl_seconds.max(0) as u32,
        disk_quota_mb: row.disk_quota_mb.max(0) as u32,
        cache_eligible: row.cache_eligible,
        cache_key_headers: Vec::new(),
        respect_origin_headers: row.respect_origin,
        stale_while_revalidate_seconds: 0,
        enabled: row.enabled,
    }
}

fn ssl_to_proto(row: &site_ssl::Model) -> SslConfig {
    let acme_enabled = row.auto_renew && row.acme_email.is_some();
    SslConfig {
        cert_pem: row.cert_pem.clone().unwrap_or_default(),
        key_pem: row.key_pem.clone().unwrap_or_default(),
        acme_enabled,
        acme_email: row.acme_email.clone().unwrap_or_default(),
        acme_challenge_type: acme_challenge_proto(row.acme_challenge_type.as_deref()),
        acme_dns_provider: row.acme_dns_provider.clone().unwrap_or_default(),
        acme_dns_config: json_to_string_map(&row.acme_dns_config),
        min_tls_version: String::new(),
        hsts_enabled: false,
        hsts_max_age: 0,
        always_use_https: false,
    }
}

fn upstreams_to_proto(site_name: &str, rows: &[site_upstreams::Model]) -> Vec<UpstreamConfig> {
    if rows.is_empty() {
        return Vec::new();
    }
    let peers = rows
        .iter()
        .map(|row| UpstreamPeer {
            address: row.address.clone(),
            weight: row.weight.max(0) as u32,
            tls: row.tls,
        })
        .collect();
    vec![UpstreamConfig {
        name: site_name.to_string(),
        peers,
        // LB_ROUND_ROBIN — the only algorithm the schema records today.
        algorithm: 0,
        health_check: None,
        connection_timeout_ms: 0,
        read_timeout_ms: 0,
        write_timeout_ms: 0,
    }]
}

/// Derives the site-wide WAF switches from the rules that are actually enabled.
fn waf_config_to_proto(rules: &[WafRule], groups: &[rule_groups::Model]) -> WafConfig {
    let active: Vec<&WafRule> = rules.iter().filter(|r| r.enabled).collect();

    // Blocking wins over monitoring, monitoring wins over off.
    let mode = if active.iter().any(|r| r.mode == WAF_MODE_BLOCK) {
        WAF_MODE_BLOCK
    } else if active.iter().any(|r| r.mode == WAF_MODE_MONITOR) {
        WAF_MODE_MONITOR
    } else {
        WAF_MODE_OFF
    };

    let has_tag = |needle: &str| {
        active.iter().any(|rule| {
            rule.tags
                .iter()
                .any(|tag| tag.to_ascii_lowercase().contains(needle))
        })
    };

    let any_group_enabled = groups.is_empty() || groups.iter().any(|group| group.enabled);

    WafConfig {
        enabled: !active.is_empty() && any_group_enabled,
        mode,
        // Paranoia level is derived from the highest severity in use (1-5 maps
        // onto the 1-4 range the protocol allows).
        paranoia_level: active
            .iter()
            .map(|rule| rule.severity)
            .max()
            .map(|severity| severity.clamp(1, 4))
            .unwrap_or(1),
        sqli_detection: has_tag("sqli") || has_tag("sql-injection"),
        xss_detection: has_tag("xss"),
        rce_detection: has_tag("rce") || has_tag("command-injection"),
        lfi_detection: has_tag("lfi") || has_tag("file-inclusion"),
        ssrf_detection: has_tag("ssrf"),
        bot_detection: has_tag("bot"),
        custom_rules: rules.to_vec(),
        managed_overrides: Vec::new(),
        ml_enabled: false,
        ml_model_path: String::new(),
        ml_threshold: 0.0,
        anomaly_threshold: 0,
    }
}

/// Converts a stored IP access rule into its protocol representation.
fn ip_access_rule_to_proto(row: &ip_access_rules::Model) -> IpAccessRule {
    IpAccessRule {
        id: row.id.to_string(),
        name: row.name.clone(),
        ip_ranges: row.ip_ranges.clone(),
        action: ip_action::to_proto(&row.action),
        note: row.note.clone().unwrap_or_default(),
        enabled: row.enabled,
    }
}

/// Converts stored geo rules into the protocol representation.
fn geo_to_proto(row: Option<&geo_rules::Model>) -> GeoConfig {
    match row {
        Some(g) => GeoConfig {
            enabled: g.enabled,
            blocked_countries: if g.mode == "block_list" { g.countries.clone() } else { Vec::new() },
            allowed_countries: if g.mode == "allow_list" { g.countries.clone() } else { Vec::new() },
            blocked_asns: g.blocked_asns.clone(),
            block_unknown: g.block_unknown,
            action: action::to_proto(&g.action),
        },
        None => GeoConfig {
            enabled: false,
            blocked_countries: Vec::new(),
            allowed_countries: Vec::new(),
            blocked_asns: Vec::new(),
            block_unknown: false,
            action: action::to_proto(action::BLOCK),
        },
    }
}

/// Converts stored challenge settings into the protocol representation.
fn challenge_to_proto(row: Option<&challenge_settings::Model>) -> ChallengeConfig {
    match row {
        Some(c) => ChallengeConfig {
            enabled: c.enabled,
            under_attack_mode: c.under_attack_mode,
            default_level: challenge_level::to_proto(&c.default_level),
            clearance_duration_seconds: c.clearance_duration_secs.max(0) as u32,
            exempt_paths: c.exempt_paths.clone(),
            request_threshold: c.rate_threshold.max(0) as u32,
            browser_integrity_check: c.browser_integrity_check,
            tls_fingerprint_check: c.tls_fingerprint_check,
        },
        None => ChallengeConfig {
            enabled: false,
            under_attack_mode: false,
            default_level: 0,
            clearance_duration_seconds: 0,
            exempt_paths: Vec::new(),
            request_threshold: 0,
            browser_integrity_check: false,
            tls_fingerprint_check: false,
        },
    }
}

/// Converts a stored rewrite rule into its protocol representation.
fn rewrite_rule_to_proto(row: &rewrite_rules::Model) -> RewriteRule {
    // Parse the JSON operations array into HeaderOperation protos
    let header_operations = parse_header_operations(&row.operations);

    RewriteRule {
        id: row.id.to_string(),
        name: row.name.clone(),
        match_expression: row.condition_expr.clone().unwrap_or_default(),
        direction: if row.direction == "response" { 1 } else { 0 },
        header_operations,
        path_rewrite: String::new(),
        path_rewrite_to: String::new(),
        query_rewrite: String::new(),
        body_search: String::new(),
        body_replace: String::new(),
        enabled: row.enabled,
        priority: row.priority.max(0) as u32,
    }
}

/// Parses JSON operations into proto HeaderOperation messages.
fn parse_header_operations(ops: &serde_json::Value) -> Vec<HeaderOperation> {
    let mut result = Vec::new();
    if let Some(array) = ops.as_array() {
        for item in array {
            if let Some(obj) = item.as_object() {
                let op_type = obj.get("type")
                    .and_then(|v| v.as_str())
                    .map(|t| match t {
                        "set" => 0,
                        "add" => 1,
                        "remove" => 2,
                        _ => 0,
                    })
                    .unwrap_or(0);
                result.push(HeaderOperation {
                    r#type: op_type,
                    name: obj.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                    value: obj.get("value").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                });
            }
        }
    }
    result
}

/// Converts a stored error page into its protocol representation.
fn error_page_to_proto(row: &error_pages::Model) -> CustomErrorPage {
    CustomErrorPage {
        id: row.id.to_string(),
        status_code: row.status_code.max(0) as u32,
        content_type: row.content_type.clone(),
        body_template: row.body_template.clone(),
        name: row.name.clone(),
        enabled: row.enabled,
    }
}

/// Builds the complete [`RuleBundle`] for one site.
pub async fn build_rule_bundle(
    db: &DatabaseConnection,
    site_row: &site::Model,
) -> Result<RuleBundle, sea_orm::DbErr> {
    let rules_rows = load_rules(db, site_row.id).await?;
    let groups = load_rule_groups(db, site_row.id).await?;
    let rate_limits = load_rate_limits(db, site_row.id).await?;
    let caches = load_cache_rules(db, site_row.id).await?;
    let upstreams = load_upstreams(db, site_row.id).await?;
    let ssl = load_ssl(db, site_row.id).await?;
    let ip_rules = load_ip_access_rules(db, site_row.id).await?;
    let geo = load_geo_rules(db, site_row.id).await?;
    let challenge = load_challenge_settings(db, site_row.id).await?;
    let rewrites = load_rewrite_rules(db, site_row.id).await?;
    let err_pages = load_error_pages(db, site_row.id).await?;

    let custom_rules: Vec<WafRule> = rules_rows.iter().map(rule_to_proto).collect();

    let mut bundle = RuleBundle {
        site_id: site_row.id.to_string(),
        // Filled in below, once every other field is final.
        config_hash: String::new(),
        updated_at: to_timestamp(site_row.updated_at),
        waf: Some(waf_config_to_proto(&custom_rules, &groups)),
        rate_limit_rules: rate_limits.iter().map(rate_limit_to_proto).collect(),
        ip_access_rules: ip_rules.iter().map(ip_access_rule_to_proto).collect(),
        geo: Some(geo_to_proto(geo.as_ref())),
        cache_rules: caches.iter().map(cache_rule_to_proto).collect(),
        challenge: Some(challenge_to_proto(challenge.as_ref())),
        rewrite_rules: rewrites.iter().map(rewrite_rule_to_proto).collect(),
        error_pages: err_pages.iter().map(error_page_to_proto).collect(),
        ssl: ssl.as_ref().map(ssl_to_proto),
        upstreams: upstreams_to_proto(&site_row.name, &upstreams),
    };

    bundle.config_hash = fingerprint(&bundle);
    Ok(bundle)
}

/// Builds a [`SiteConfig`] for the given sites, or for every site when
/// `site_ids` is `None`.
pub async fn build_site_config(
    db: &DatabaseConnection,
    site_ids: Option<&[Uuid]>,
) -> Result<SiteConfig, sea_orm::DbErr> {
    let mut query = site::Entity::find().order_by_asc(site::Column::Domain);
    if let Some(ids) = site_ids {
        if ids.is_empty() {
            // Nothing requested: return an empty configuration instead of
            // accidentally serving every site.
            let mut config = SiteConfig {
                sites: Vec::new(),
                updated_at: to_timestamp(Utc.timestamp_opt(0, 0).unwrap()),
                config_hash: String::new(),
            };
            config.config_hash = fingerprint(&config);
            return Ok(config);
        }
        query = query.filter(site::Column::Id.is_in(ids.iter().copied()));
    }
    let rows = query.all(db).await?;

    let mut sites = Vec::with_capacity(rows.len());
    let mut updated_at = Utc.timestamp_opt(0, 0).unwrap();
    for row in &rows {
        let bundle = build_rule_bundle(db, row).await?;
        if row.updated_at > updated_at {
            updated_at = row.updated_at;
        }
        sites.push(Site {
            id: row.id.to_string(),
            name: row.name.clone(),
            domain: row.domain.clone(),
            alternate_domains: Vec::new(),
            status: site_status_proto(&row.status),
            rules: Some(bundle),
        });
    }

    let mut config = SiteConfig {
        sites,
        updated_at: to_timestamp(updated_at),
        config_hash: String::new(),
    };
    config.config_hash = fingerprint(&config);
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_roundtrip() {
        let value = Utc::now();
        let ts = to_timestamp(value).expect("timestamp");
        let back = from_timestamp(Some(&ts));
        assert_eq!(back.timestamp(), value.timestamp());
        assert!(from_timestamp(None) <= Utc::now());
    }

    #[test]
    fn status_mappings_match_the_proto() {
        assert_eq!(site_status_proto(site_status::ACTIVE), 0);
        assert_eq!(site_status_proto(site_status::PAUSED), 1);
        assert_eq!(site_status_proto(site_status::PENDING), 2);
        assert_eq!(acme_challenge_proto(Some(acme_challenge::DNS_01)), 1);
        assert_eq!(acme_challenge_proto(Some(acme_challenge::HTTP_01)), 0);
        assert_eq!(acme_challenge_proto(None), 0);
    }

    #[test]
    fn fingerprints_are_stable_and_order_sensitive() {
        let bundle = RuleBundle {
            site_id: Uuid::nil().to_string(),
            ..Default::default()
        };
        assert_eq!(fingerprint(&bundle), fingerprint(&bundle));
        assert_eq!(fingerprint(&bundle).len(), 16);

        let mut other = bundle.clone();
        other.site_id = Uuid::new_v4().to_string();
        assert_ne!(fingerprint(&bundle), fingerprint(&other));
    }

    #[test]
    fn json_objects_become_string_maps() {
        let value = serde_json::json!({ "token": "abc", "retries": 3 });
        let map = json_to_string_map(&Some(value));
        assert_eq!(map.get("token").map(String::as_str), Some("abc"));
        assert_eq!(map.get("retries").map(String::as_str), Some("3"));
        assert!(json_to_string_map(&None).is_empty());
    }

    #[test]
    fn waf_mode_follows_the_strongest_rule() {
        let base = rule::Model {
            id: Uuid::new_v4(),
            group_id: None,
            site_id: Uuid::nil(),
            name: "r".into(),
            description: None,
            expression: "true".into(),
            action: action::BLOCK.into(),
            severity: 3,
            tags: vec!["sqli".into()],
            enabled: true,
            mode: mode::MONITOR.into(),
            priority: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let monitoring = vec![rule_to_proto(&base)];
        let config = waf_config_to_proto(&monitoring, &[]);
        assert!(config.enabled);
        assert_eq!(config.mode, WAF_MODE_MONITOR);
        assert!(config.sqli_detection);
        assert_eq!(config.paranoia_level, 3);

        let mut blocking = base.clone();
        blocking.mode = mode::BLOCK.into();
        let config = waf_config_to_proto(&[rule_to_proto(&blocking)], &[]);
        assert_eq!(config.mode, WAF_MODE_BLOCK);

        let mut disabled = base.clone();
        disabled.enabled = false;
        let config = waf_config_to_proto(&[rule_to_proto(&disabled)], &[]);
        assert!(!config.enabled);
        assert_eq!(config.mode, WAF_MODE_OFF);
    }

    #[test]
    fn upstreams_collapse_into_a_single_pool() {
        let rows = vec![site_upstreams::Model {
            id: Uuid::new_v4(),
            site_id: Uuid::nil(),
            name: "origin".into(),
            address: "10.0.0.1:8080".into(),
            weight: 3,
            tls: true,
            health_status: "unknown".into(),
            created_at: Utc::now(),
        }];
        let pools = upstreams_to_proto("example.com", &rows);
        assert_eq!(pools.len(), 1);
        assert_eq!(pools[0].peers.len(), 1);
        assert_eq!(pools[0].peers[0].weight, 3);
        assert!(upstreams_to_proto("example.com", &[]).is_empty());
    }
}
