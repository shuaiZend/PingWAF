use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arc_swap::ArcSwap;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use pingap_cache::quota;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use pingwaf_proto::control_plane as proto;

// ─────────────────────────────────────────────────────────────
// Serializable cache types (local mirrors of proto types)
// ─────────────────────────────────────────────────────────────

/// Top-level cached state that is swapped atomically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedRules {
    /// Site rules keyed by site_id
    pub sites: HashMap<String, SiteRules>,
    /// Domain → site_id index for fast lookup
    pub domain_index: HashMap<String, String>,
    /// When the cache was last updated from the server
    pub updated_at: DateTime<Utc>,
    /// Hash of the configuration (for delta sync)
    pub config_hash: String,
}

impl Default for CachedRules {
    fn default() -> Self {
        Self {
            sites: HashMap::new(),
            domain_index: HashMap::new(),
            updated_at: Utc::now(),
            config_hash: String::new(),
        }
    }
}

/// Rules for a single site/domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteRules {
    pub site_id: String,
    pub domain: String,
    pub alternate_domains: Vec<String>,
    pub waf_config: Option<WafConfig>,
    pub rate_limit_rules: Vec<RateLimitRule>,
    pub ip_access_rules: Vec<IpAccessRule>,
    pub geo_config: Option<GeoConfig>,
    pub cache_rules: Vec<CacheRule>,
    pub challenge_config: Option<ChallengeConfig>,
    pub rewrite_rules: Vec<RewriteRule>,
    pub error_pages: Vec<CustomErrorPage>,
    pub ssl_config: Option<SslConfig>,
    pub upstreams: Vec<UpstreamConfig>,
}

impl SiteRules {
    /// All domains this site responds to.
    pub fn all_domains(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.domain.as_str())
            .chain(self.alternate_domains.iter().map(|s| s.as_str()))
    }
}

// ─── WAF ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WafConfig {
    pub enabled: bool,
    pub mode: WafMode,
    pub paranoia_level: u32,
    pub sqli_detection: bool,
    pub xss_detection: bool,
    pub rce_detection: bool,
    pub lfi_detection: bool,
    pub ssrf_detection: bool,
    pub bot_detection: bool,
    pub custom_rules: Vec<WafRule>,
    pub managed_overrides: Vec<RuleOverride>,
    pub ml_enabled: bool,
    pub ml_model_path: String,
    pub ml_threshold: f64,
    pub anomaly_threshold: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WafMode {
    Off,
    Monitor,
    Block,
}

impl From<i32> for WafMode {
    fn from(v: i32) -> Self {
        match v {
            1 => Self::Monitor,
            2 => Self::Block,
            _ => Self::Off,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WafRule {
    pub id: String,
    pub name: String,
    pub description: String,
    pub expression: String,
    pub action: WafAction,
    pub severity: u32,
    pub tags: Vec<String>,
    pub enabled: bool,
    pub mode: WafMode,
    pub priority: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WafAction {
    Block,
    Log,
    Challenge,
    JsChallenge,
    Allow,
}

impl From<i32> for WafAction {
    fn from(v: i32) -> Self {
        match v {
            1 => Self::Log,
            2 => Self::Challenge,
            3 => Self::JsChallenge,
            4 => Self::Allow,
            _ => Self::Block,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleOverride {
    pub rule_id: String,
    pub tag: String,
    pub action: WafAction,
    pub enabled: bool,
}

// ─── Rate Limiting ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitRule {
    pub id: String,
    pub name: String,
    pub expression: String,
    pub characteristics: Vec<String>,
    pub period_seconds: u32,
    pub threshold: u32,
    pub action: WafAction,
    pub mitigation_timeout_seconds: u32,
    pub enabled: bool,
    pub priority: u32,
}

// ─── IP Access ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpAccessRule {
    pub id: String,
    pub name: String,
    pub ip_ranges: Vec<String>,
    pub action: IpAccessAction,
    pub note: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IpAccessAction {
    Block,
    Challenge,
    JsChallenge,
    Allow,
}

impl From<i32> for IpAccessAction {
    fn from(v: i32) -> Self {
        match v {
            1 => Self::Challenge,
            2 => Self::JsChallenge,
            3 => Self::Allow,
            _ => Self::Block,
        }
    }
}

// ─── Geo ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoConfig {
    pub enabled: bool,
    pub blocked_countries: Vec<String>,
    pub allowed_countries: Vec<String>,
    pub blocked_asns: Vec<String>,
    pub block_unknown: bool,
    pub action: WafAction,
}

// ─── Cache Rules ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheRule {
    pub id: String,
    pub name: String,
    pub match_expression: String,
    pub edge_ttl_seconds: u32,
    pub browser_ttl_seconds: u32,
    pub disk_quota_mb: u32,
    pub cache_eligible: bool,
    pub cache_key_headers: Vec<String>,
    pub respect_origin_headers: bool,
    pub stale_while_revalidate_seconds: u32,
    pub enabled: bool,
}

// ─── Challenge ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeConfig {
    pub enabled: bool,
    pub under_attack_mode: bool,
    pub default_level: ChallengeLevel,
    pub clearance_duration_seconds: u32,
    pub exempt_paths: Vec<String>,
    pub request_threshold: u32,
    pub browser_integrity_check: bool,
    pub tls_fingerprint_check: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChallengeLevel {
    None,
    NonInteractive,
    Managed,
    Interactive,
}

impl From<i32> for ChallengeLevel {
    fn from(v: i32) -> Self {
        match v {
            1 => Self::NonInteractive,
            2 => Self::Managed,
            3 => Self::Interactive,
            _ => Self::None,
        }
    }
}

// ─── Rewrite ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewriteRule {
    pub id: String,
    pub name: String,
    pub match_expression: String,
    pub direction: RewriteDirection,
    pub header_operations: Vec<HeaderOperation>,
    pub path_rewrite: String,
    pub path_rewrite_to: String,
    pub query_rewrite: String,
    pub body_search: String,
    pub body_replace: String,
    pub enabled: bool,
    pub priority: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RewriteDirection {
    Request,
    Response,
}

impl From<i32> for RewriteDirection {
    fn from(v: i32) -> Self {
        match v {
            1 => Self::Response,
            _ => Self::Request,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderOperation {
    pub op_type: HeaderOpType,
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeaderOpType {
    Set,
    Add,
    Remove,
}

impl From<i32> for HeaderOpType {
    fn from(v: i32) -> Self {
        match v {
            1 => Self::Add,
            2 => Self::Remove,
            _ => Self::Set,
        }
    }
}

// ─── Error Pages ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomErrorPage {
    pub id: String,
    pub status_code: u32,
    pub content_type: String,
    pub body_template: String,
    pub name: String,
    pub enabled: bool,
}

// ─── SSL ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SslConfig {
    pub cert_pem: String,
    pub key_pem: String,
    pub acme_enabled: bool,
    pub acme_email: String,
    pub acme_challenge_type: String,
    pub acme_dns_provider: String,
    pub acme_dns_config: HashMap<String, String>,
    pub min_tls_version: String,
    pub hsts_enabled: bool,
    pub hsts_max_age: u32,
    pub always_use_https: bool,
}

// ─── Upstream ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamConfig {
    pub name: String,
    pub peers: Vec<UpstreamPeer>,
    pub algorithm: String,
    pub health_check: Option<HealthCheckConfig>,
    pub connection_timeout_ms: u32,
    pub read_timeout_ms: u32,
    pub write_timeout_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamPeer {
    pub address: String,
    pub weight: u32,
    pub tls: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckConfig {
    pub enabled: bool,
    pub path: String,
    pub interval_seconds: u32,
    pub timeout_ms: u32,
    pub unhealthy_threshold: u32,
    pub healthy_threshold: u32,
}

// ─────────────────────────────────────────────────────────────
// Blocked IP tracking (with TTL)
// ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockedIpEntry {
    pub ip: String,
    pub site_id: String,
    pub reason: String,
    /// None = permanent block
    pub expires_at: Option<DateTime<Utc>>,
}

impl BlockedIpEntry {
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(exp) => Utc::now() >= exp,
            None => false,
        }
    }
}

// ─────────────────────────────────────────────────────────────
// Disk persistence metadata
// ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheMetadata {
    config_hash: String,
    updated_at: DateTime<Utc>,
    agent_id: String,
    version: u32,
}

// ─────────────────────────────────────────────────────────────
// RuleCache — the main public interface
// ─────────────────────────────────────────────────────────────

/// Thread-safe local rule cache with disk persistence.
///
/// Uses `ArcSwap` for lock-free reads on the hot path and a separate
/// `DashMap` for dynamically-blocked IPs (which change more frequently
/// than rule bundles).
pub struct RuleCache {
    /// In-memory rule storage (fast, lock-free reads via ArcSwap)
    inner: ArcSwap<CachedRules>,
    /// Dynamically blocked IPs: key = "site_id:ip"
    blocked_ips: DashMap<String, BlockedIpEntry>,
    /// Disk persistence directory
    cache_dir: PathBuf,
    /// Agent ID for metadata
    agent_id: String,
    /// Domains this agent currently holds a ceiling for.
    ///
    /// The ledger is shared with the `cache` plugins built from the static
    /// config, which set their own `disk_quota_mb`. Sweeping only what is
    /// recorded here is what keeps a bundle naming no site at all - the first
    /// one, before the control plane has answered - from wiping those.
    quota_domains: Mutex<HashSet<String>>,
}

impl RuleCache {
    /// Create a new rule cache, loading from disk if a previous cache exists.
    pub fn new(
        cache_dir: PathBuf,
        agent_id: String,
    ) -> anyhow::Result<Arc<Self>> {
        let cache = Arc::new(Self {
            inner: ArcSwap::new(Arc::new(CachedRules::default())),
            blocked_ips: DashMap::new(),
            cache_dir: cache_dir.clone(),
            agent_id,
            quota_domains: Mutex::new(HashSet::new()),
        });

        // Ensure cache directory exists
        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            warn!(error = %e, "Failed to create cache directory, running without persistence");
        } else {
            // Try to load existing cache from disk
            if let Err(e) = cache.load_from_disk() {
                debug!(error = %e, "No existing cache found on disk or failed to load");
            } else {
                info!("Loaded rule cache from disk");
            }
        }

        // Quotas persisted with the rules take effect before the first push
        // from the control plane arrives.
        cache.apply_cache_quotas();

        Ok(cache)
    }

    /// Applies the `disk_quota_mb` of every enabled, cache-eligible rule to the
    /// edge's cache ledger, and drops the domains that are no longer
    /// configured.
    ///
    /// The ledger is the process-wide one the proxy's file cache writes
    /// through (`pingap_cache::global_disk_quota`), *not* a private copy: the
    /// cached objects live wherever the `cache` plugin's `directory` points,
    /// and a second ledger rooted elsewhere would only ever read zero and
    /// purge nothing. Because this runs as soon as the rules are known -
    /// possibly before the proxy has built that backend - the ceilings are
    /// parked and replayed by `pingap_cache::quota` once it exists.
    ///
    /// A site answers on several domains (primary plus alternates) while the
    /// rules are per site, so every domain of a site gets the largest ceiling
    /// configured for it. Several sites sharing a domain get the largest of
    /// theirs: the ledger accounts per domain, and the smaller ceiling would
    /// otherwise be unenforceable anyway.
    pub fn apply_cache_quotas(&self) {
        let rules = self.inner.load();
        let mut tracked: HashMap<String, u64> = HashMap::new();
        for site in rules.sites.values() {
            let quota_mb = site
                .cache_rules
                .iter()
                .filter(|rule| rule.enabled && rule.cache_eligible)
                .map(|rule| u64::from(rule.disk_quota_mb))
                .max()
                .unwrap_or(0);
            for domain in site.all_domains() {
                if domain.is_empty() {
                    continue;
                }
                let entry = tracked.entry(domain.to_string()).or_insert(0);
                *entry = (*entry).max(quota_mb);
            }
        }
        for (domain, quota_mb) in &tracked {
            quota::set_quota(domain, *quota_mb);
        }
        // Ceilings this agent set for a domain no rule names any more. Only
        // its own are swept: a `cache` plugin configured from the static file
        // keeps the ceiling it asked for, even when the control plane has
        // nothing to say about that namespace.
        let mut owned = self
            .quota_domains
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for domain in owned.iter() {
            if !tracked.contains_key(domain) {
                quota::forget_quota(domain);
            }
        }
        *owned = tracked.keys().cloned().collect();
        debug!(domains = owned.len(), "applied cache disk quotas");
    }

    /// Per-site cache disk usage for the heartbeat, so the control plane can
    /// answer `/api/v1/cache/status` without polling every edge.
    ///
    /// `cache_hits`/`cache_misses` stay at zero: this agent holds the rules, not
    /// the data path, so it has nothing to count. The control plane derives hit
    /// ratios from the access logs instead of trusting a constant zero here.
    pub fn cache_statuses(&self) -> Vec<proto::SiteStatus> {
        let rules = self.inner.load();
        // Usage comes from the shared ledger; before the proxy has built a
        // file cache backend there is none, and nothing on disk to report.
        // The configured ceiling is still reported, so the control plane can
        // show "0 of 512 MB" rather than "unconfigured" for a site whose quota
        // simply has not started filling yet.
        let ledger = pingap_cache::global_disk_quota();
        let mut out = Vec::with_capacity(rules.sites.len());
        for (site_id, site) in rules.sites.iter() {
            // A site answers on its primary domain and every alias, and the
            // ledger accounts each one separately. The ceiling is the largest
            // rather than the sum: `apply_cache_quotas` gives every hostname of
            // a site the same budget, so adding them would inflate it.
            let mut disk_bytes = 0u64;
            let mut items = 0u64;
            let mut evictions = 0u64;
            let mut quota_mb = 0u64;
            for domain in site.all_domains() {
                if domain.is_empty() {
                    continue;
                }
                if let Some(usage) = ledger.and_then(|l| l.get_usage(domain)) {
                    disk_bytes = disk_bytes.saturating_add(usage.current_bytes);
                    items = items.saturating_add(usage.item_count);
                    evictions = evictions.saturating_add(usage.evictions_total);
                }
                quota_mb = quota_mb
                    .max(quota::configured_quota_mb(domain).unwrap_or(0));
            }
            out.push(proto::SiteStatus {
                site_id: site_id.clone(),
                domain: site.domain.clone(),
                requests_total: 0,
                requests_blocked: 0,
                cache_hits: 0,
                cache_misses: 0,
                ssl_status: String::new(),
                ssl_expires_at: None,
                cache_disk_bytes: disk_bytes,
                cache_items: items,
                cache_evictions: evictions,
                cache_quota_bytes: quota_mb.saturating_mul(quota::BYTES_PER_MB),
            });
        }
        out
    }

    /// Purges this agent's edge cache for a site.
    ///
    /// An empty `urls` list purges the whole site (every domain it answers
    /// on); otherwise each entry is matched against the ledger keys. Returns
    /// the number of cache objects removed.
    pub async fn purge_site(
        &self,
        site_id: &str,
        urls: &[String],
    ) -> anyhow::Result<usize> {
        let domains: Vec<String> = {
            let rules = self.inner.load();
            match rules.sites.get(site_id) {
                Some(site) => site
                    .all_domains()
                    .filter(|domain| !domain.is_empty())
                    .map(str::to_string)
                    .collect(),
                None => Vec::new(),
            }
        };
        if domains.is_empty() {
            anyhow::bail!(
                "no cached rules for site {site_id}, nothing to purge"
            );
        }
        let Some(ledger) = pingap_cache::global_disk_quota() else {
            // Memory-only or not-yet-initialised edge: there is no disk cache
            // to delete from, which answers the purge rather than failing it.
            debug!(site_id, "no file cache backend, nothing to purge");
            return Ok(0);
        };
        let mut purged = 0;
        for domain in &domains {
            purged += if urls.is_empty() {
                ledger.purge_domain(domain).await.purged_count
            } else {
                ledger.purge_urls(domain, urls).await.purged_count
            };
        }
        info!(
            site_id,
            domains = domains.len(),
            purged,
            "purged edge cache for site"
        );
        Ok(purged)
    }

    /// Update the cache from a proto RuleBundle received from the control plane.
    pub fn update_from_bundle(
        &self,
        bundle: &proto::RuleBundle,
    ) -> anyhow::Result<()> {
        let site_rules = Self::convert_bundle(bundle);
        let site_id = bundle.site_id.clone();
        let domain = site_rules.domain.clone();
        let alternate_domains = site_rules.alternate_domains.clone();

        // Swap in updated rules
        self.inner.rcu(|current| {
            let mut updated = (**current).clone();
            // Remove old domain index entries for this site
            if let Some(old_site) = updated.sites.get(&site_id) {
                updated.domain_index.remove(&old_site.domain);
                for alt in &old_site.alternate_domains {
                    updated.domain_index.remove(alt);
                }
            }
            // Insert new site
            updated.domain_index.insert(domain.clone(), site_id.clone());
            for alt in &alternate_domains {
                updated.domain_index.insert(alt.clone(), site_id.clone());
            }
            updated.sites.insert(site_id.clone(), site_rules.clone());
            updated.config_hash = bundle.config_hash.clone();
            updated.updated_at = Utc::now();
            Arc::new(updated)
        });

        info!(site_id = %site_id, domain = %domain, "Updated rule cache from bundle");

        // The bundle may have changed a site's cache rules, so the per-domain
        // ceilings have to follow before the next write is accounted.
        self.apply_cache_quotas();

        // Persist asynchronously (best effort)
        if let Err(e) = self.persist_to_disk() {
            warn!(error = %e, "Failed to persist rule cache to disk");
        }

        Ok(())
    }

    /// Update cache from a full SiteConfig (used on initial registration).
    pub fn update_from_site_config(
        &self,
        config: &proto::SiteConfig,
    ) -> anyhow::Result<()> {
        self.inner.rcu(|current| {
            let mut updated = (**current).clone();
            updated.config_hash = config.config_hash.clone();
            updated.updated_at = Utc::now();

            for site in &config.sites {
                if let Some(ref bundle) = site.rules {
                    let mut site_rules = Self::convert_bundle(bundle);
                    // Override domain from site-level field
                    if !site.domain.is_empty() {
                        // Remove old domain index
                        updated.domain_index.remove(&site_rules.domain);
                        site_rules.domain = site.domain.clone();
                    }
                    site_rules.site_id = site.id.clone();
                    site_rules.alternate_domains =
                        site.alternate_domains.clone();

                    // Update domain index
                    updated
                        .domain_index
                        .insert(site_rules.domain.clone(), site.id.clone());
                    for alt in &site_rules.alternate_domains {
                        updated
                            .domain_index
                            .insert(alt.clone(), site.id.clone());
                    }
                    updated.sites.insert(site.id.clone(), site_rules);
                }
            }

            Arc::new(updated)
        });

        info!(
            sites = self.inner.load().sites.len(),
            "Updated rule cache from full site config"
        );

        self.apply_cache_quotas();

        if let Err(e) = self.persist_to_disk() {
            warn!(error = %e, "Failed to persist rule cache to disk");
        }

        Ok(())
    }

    /// Look up site rules by domain name.
    pub fn get_site_rules(&self, domain: &str) -> Option<Arc<SiteRules>> {
        let rules = self.inner.load();
        let site_id = rules.domain_index.get(domain)?;
        rules.sites.get(site_id).cloned().map(Arc::new)
    }

    /// Look up site rules by site ID.
    pub fn get_site_rules_by_id(
        &self,
        site_id: &str,
    ) -> Option<Arc<SiteRules>> {
        let rules = self.inner.load();
        rules.sites.get(site_id).cloned().map(Arc::new)
    }

    /// Get all cached site rules (snapshot).
    pub fn all_sites(&self) -> Arc<CachedRules> {
        self.inner.load_full()
    }

    /// Get current config hash for delta sync.
    pub fn config_hash(&self) -> String {
        self.inner.load().config_hash.clone()
    }

    // ─── IP Blocking ────────────────────────────────────────

    /// Check if an IP is dynamically blocked for a given site.
    pub fn is_ip_blocked(&self, site_id: &str, ip: &str) -> bool {
        let key = format!("{}:{}", site_id, ip);
        match self.blocked_ips.get(&key) {
            Some(entry) => {
                if entry.is_expired() {
                    drop(entry);
                    self.blocked_ips.remove(&key);
                    false
                } else {
                    true
                }
            },
            None => false,
        }
    }

    /// Block an IP for a site, optionally with a duration.
    pub fn block_ip(
        &self,
        site_id: &str,
        ip: &str,
        duration: Option<Duration>,
        reason: &str,
    ) {
        let key = format!("{}:{}", site_id, ip);
        let expires_at = duration.map(|d| {
            Utc::now()
                + chrono::Duration::from_std(d)
                    .unwrap_or(chrono::Duration::hours(1))
        });

        let entry = BlockedIpEntry {
            ip: ip.to_string(),
            site_id: site_id.to_string(),
            reason: reason.to_string(),
            expires_at,
        };

        self.blocked_ips.insert(key.clone(), entry);
        info!(site_id = %site_id, ip = %ip, reason = %reason, "Blocked IP");

        // Persist blocked IPs
        if let Err(e) = self.persist_blocked_ips() {
            warn!(error = %e, "Failed to persist blocked IPs");
        }
    }

    /// Unblock an IP for a site.
    pub fn unblock_ip(&self, site_id: &str, ip: &str) {
        let key = format!("{}:{}", site_id, ip);
        if self.blocked_ips.remove(&key).is_some() {
            info!(site_id = %site_id, ip = %ip, "Unblocked IP");
            if let Err(e) = self.persist_blocked_ips() {
                warn!(error = %e, "Failed to persist blocked IPs");
            }
        }
    }

    /// Clean up expired IP blocks.
    pub fn cleanup_expired_blocks(&self) {
        let mut expired_keys = Vec::new();
        for entry in self.blocked_ips.iter() {
            if entry.value().is_expired() {
                expired_keys.push(entry.key().clone());
            }
        }
        for key in &expired_keys {
            self.blocked_ips.remove(key);
        }
        if !expired_keys.is_empty() {
            debug!(count = expired_keys.len(), "Cleaned up expired IP blocks");
            let _ = self.persist_blocked_ips();
        }
    }

    // ─── Disk Persistence ───────────────────────────────────

    /// Persist current rules to disk.
    pub fn persist_to_disk(&self) -> anyhow::Result<()> {
        if !self.cache_dir.exists() {
            return Ok(());
        }

        let rules = self.inner.load();
        let sites_path = self.cache_dir.join("sites.json");
        let metadata_path = self.cache_dir.join("metadata.json");

        // Write sites
        let sites_json = serde_json::to_string_pretty(&**rules)?;
        std::fs::write(&sites_path, sites_json)?;

        // Write metadata
        let metadata = CacheMetadata {
            config_hash: rules.config_hash.clone(),
            updated_at: rules.updated_at,
            agent_id: self.agent_id.clone(),
            version: 1,
        };
        let meta_json = serde_json::to_string_pretty(&metadata)?;
        std::fs::write(&metadata_path, meta_json)?;

        // Write blocked IPs
        self.persist_blocked_ips()?;

        debug!("Persisted rule cache to disk");
        Ok(())
    }

    /// Load rules from disk.
    pub fn load_from_disk(&self) -> anyhow::Result<()> {
        let sites_path = self.cache_dir.join("sites.json");
        let blocked_path = self.cache_dir.join("blocked_ips.json");

        if !sites_path.exists() {
            anyhow::bail!("No cache file found at {:?}", sites_path);
        }

        let sites_json = std::fs::read_to_string(&sites_path)?;
        let cached: CachedRules = serde_json::from_str(&sites_json)?;

        info!(
            sites = cached.sites.len(),
            config_hash = %cached.config_hash,
            "Loaded rule cache from disk"
        );

        self.inner.store(Arc::new(cached));

        // Load blocked IPs
        if blocked_path.exists() {
            let blocked_json = std::fs::read_to_string(&blocked_path)?;
            let blocked: Vec<BlockedIpEntry> =
                serde_json::from_str(&blocked_json)?;
            let mut loaded = 0;
            for entry in blocked {
                if !entry.is_expired() {
                    let key = format!("{}:{}", entry.site_id, entry.ip);
                    self.blocked_ips.insert(key, entry);
                    loaded += 1;
                }
            }
            debug!(count = loaded, "Loaded blocked IPs from disk");
        }

        Ok(())
    }

    /// Persist blocked IPs to disk.
    fn persist_blocked_ips(&self) -> anyhow::Result<()> {
        if !self.cache_dir.exists() {
            return Ok(());
        }

        let blocked_path = self.cache_dir.join("blocked_ips.json");
        let entries: Vec<BlockedIpEntry> = self
            .blocked_ips
            .iter()
            .filter(|e| !e.value().is_expired())
            .map(|e| e.value().clone())
            .collect();

        let json = serde_json::to_string_pretty(&entries)?;
        std::fs::write(&blocked_path, json)?;
        Ok(())
    }

    // ─── Proto Conversion ───────────────────────────────────

    /// Convert a proto RuleBundle into local SiteRules.
    fn convert_bundle(bundle: &proto::RuleBundle) -> SiteRules {
        SiteRules {
            site_id: bundle.site_id.clone(),
            domain: String::new(), // Will be set from Site if available
            alternate_domains: Vec::new(),
            waf_config: bundle.waf.as_ref().map(Self::convert_waf_config),
            rate_limit_rules: bundle
                .rate_limit_rules
                .iter()
                .map(Self::convert_rate_limit_rule)
                .collect(),
            ip_access_rules: bundle
                .ip_access_rules
                .iter()
                .map(Self::convert_ip_access_rule)
                .collect(),
            geo_config: bundle.geo.as_ref().map(Self::convert_geo_config),
            cache_rules: bundle
                .cache_rules
                .iter()
                .map(Self::convert_cache_rule)
                .collect(),
            challenge_config: bundle
                .challenge
                .as_ref()
                .map(Self::convert_challenge_config),
            rewrite_rules: bundle
                .rewrite_rules
                .iter()
                .map(Self::convert_rewrite_rule)
                .collect(),
            error_pages: bundle
                .error_pages
                .iter()
                .map(Self::convert_error_page)
                .collect(),
            ssl_config: bundle.ssl.as_ref().map(Self::convert_ssl_config),
            upstreams: bundle
                .upstreams
                .iter()
                .map(Self::convert_upstream_config)
                .collect(),
        }
    }

    fn convert_waf_config(w: &proto::WafConfig) -> WafConfig {
        WafConfig {
            enabled: w.enabled,
            mode: WafMode::from(w.mode),
            paranoia_level: w.paranoia_level,
            sqli_detection: w.sqli_detection,
            xss_detection: w.xss_detection,
            rce_detection: w.rce_detection,
            lfi_detection: w.lfi_detection,
            ssrf_detection: w.ssrf_detection,
            bot_detection: w.bot_detection,
            custom_rules: w
                .custom_rules
                .iter()
                .map(|r| WafRule {
                    id: r.id.clone(),
                    name: r.name.clone(),
                    description: r.description.clone(),
                    expression: r.expression.clone(),
                    action: WafAction::from(r.action),
                    severity: r.severity,
                    tags: r.tags.clone(),
                    enabled: r.enabled,
                    mode: WafMode::from(r.mode),
                    priority: r.priority,
                })
                .collect(),
            managed_overrides: w
                .managed_overrides
                .iter()
                .map(|o| RuleOverride {
                    rule_id: o.rule_id.clone(),
                    tag: o.tag.clone(),
                    action: WafAction::from(o.action),
                    enabled: o.enabled,
                })
                .collect(),
            ml_enabled: w.ml_enabled,
            ml_model_path: w.ml_model_path.clone(),
            ml_threshold: w.ml_threshold,
            anomaly_threshold: w.anomaly_threshold,
        }
    }

    fn convert_rate_limit_rule(r: &proto::RateLimitRule) -> RateLimitRule {
        RateLimitRule {
            id: r.id.clone(),
            name: r.name.clone(),
            expression: r.expression.clone(),
            characteristics: r
                .characteristics
                .iter()
                .map(|c| {
                    format!(
                        "{:?}",
                        proto::RateLimitCharacteristics::try_from(*c)
                            .unwrap_or(
                            proto::RateLimitCharacteristics::RateLimitCharIp
                        )
                    )
                })
                .collect(),
            period_seconds: r.period_seconds,
            threshold: r.threshold,
            action: WafAction::from(r.action),
            mitigation_timeout_seconds: r.mitigation_timeout_seconds,
            enabled: r.enabled,
            priority: r.priority,
        }
    }

    fn convert_ip_access_rule(r: &proto::IpAccessRule) -> IpAccessRule {
        IpAccessRule {
            id: r.id.clone(),
            name: r.name.clone(),
            ip_ranges: r.ip_ranges.clone(),
            action: IpAccessAction::from(r.action),
            note: r.note.clone(),
            enabled: r.enabled,
        }
    }

    fn convert_geo_config(g: &proto::GeoConfig) -> GeoConfig {
        GeoConfig {
            enabled: g.enabled,
            blocked_countries: g.blocked_countries.clone(),
            allowed_countries: g.allowed_countries.clone(),
            blocked_asns: g.blocked_asns.clone(),
            block_unknown: g.block_unknown,
            action: WafAction::from(g.action),
        }
    }

    fn convert_cache_rule(c: &proto::CacheRule) -> CacheRule {
        CacheRule {
            id: c.id.clone(),
            name: c.name.clone(),
            match_expression: c.match_expression.clone(),
            edge_ttl_seconds: c.edge_ttl_seconds,
            browser_ttl_seconds: c.browser_ttl_seconds,
            disk_quota_mb: c.disk_quota_mb,
            cache_eligible: c.cache_eligible,
            cache_key_headers: c.cache_key_headers.clone(),
            respect_origin_headers: c.respect_origin_headers,
            stale_while_revalidate_seconds: c.stale_while_revalidate_seconds,
            enabled: c.enabled,
        }
    }

    fn convert_challenge_config(c: &proto::ChallengeConfig) -> ChallengeConfig {
        ChallengeConfig {
            enabled: c.enabled,
            under_attack_mode: c.under_attack_mode,
            default_level: ChallengeLevel::from(c.default_level),
            clearance_duration_seconds: c.clearance_duration_seconds,
            exempt_paths: c.exempt_paths.clone(),
            request_threshold: c.request_threshold,
            browser_integrity_check: c.browser_integrity_check,
            tls_fingerprint_check: c.tls_fingerprint_check,
        }
    }

    fn convert_rewrite_rule(r: &proto::RewriteRule) -> RewriteRule {
        RewriteRule {
            id: r.id.clone(),
            name: r.name.clone(),
            match_expression: r.match_expression.clone(),
            direction: RewriteDirection::from(r.direction),
            header_operations: r
                .header_operations
                .iter()
                .map(|h| HeaderOperation {
                    op_type: HeaderOpType::from(h.r#type),
                    name: h.name.clone(),
                    value: h.value.clone(),
                })
                .collect(),
            path_rewrite: r.path_rewrite.clone(),
            path_rewrite_to: r.path_rewrite_to.clone(),
            query_rewrite: r.query_rewrite.clone(),
            body_search: r.body_search.clone(),
            body_replace: r.body_replace.clone(),
            enabled: r.enabled,
            priority: r.priority,
        }
    }

    fn convert_error_page(e: &proto::CustomErrorPage) -> CustomErrorPage {
        CustomErrorPage {
            id: e.id.clone(),
            status_code: e.status_code,
            content_type: e.content_type.clone(),
            body_template: e.body_template.clone(),
            name: e.name.clone(),
            enabled: e.enabled,
        }
    }

    fn convert_ssl_config(s: &proto::SslConfig) -> SslConfig {
        SslConfig {
            cert_pem: s.cert_pem.clone(),
            key_pem: s.key_pem.clone(),
            acme_enabled: s.acme_enabled,
            acme_email: s.acme_email.clone(),
            acme_challenge_type: format!(
                "{:?}",
                proto::AcmeChallengeType::try_from(s.acme_challenge_type)
                    .unwrap_or(proto::AcmeChallengeType::AcmeHttp01)
            ),
            acme_dns_provider: s.acme_dns_provider.clone(),
            acme_dns_config: s.acme_dns_config.clone(),
            min_tls_version: s.min_tls_version.clone(),
            hsts_enabled: s.hsts_enabled,
            hsts_max_age: s.hsts_max_age,
            always_use_https: s.always_use_https,
        }
    }

    fn convert_upstream_config(u: &proto::UpstreamConfig) -> UpstreamConfig {
        UpstreamConfig {
            name: u.name.clone(),
            peers: u
                .peers
                .iter()
                .map(|p| UpstreamPeer {
                    address: p.address.clone(),
                    weight: p.weight,
                    tls: p.tls,
                })
                .collect(),
            algorithm: format!(
                "{:?}",
                proto::LoadBalanceAlgorithm::try_from(u.algorithm)
                    .unwrap_or(proto::LoadBalanceAlgorithm::LbRoundRobin)
            ),
            health_check: u.health_check.as_ref().map(|h| HealthCheckConfig {
                enabled: h.enabled,
                path: h.path.clone(),
                interval_seconds: h.interval_seconds,
                timeout_ms: h.timeout_ms,
                unhealthy_threshold: h.unhealthy_threshold,
                healthy_threshold: h.healthy_threshold,
            }),
            connection_timeout_ms: u.connection_timeout_ms,
            read_timeout_ms: u.read_timeout_ms,
            write_timeout_ms: u.write_timeout_ms,
        }
    }
}
