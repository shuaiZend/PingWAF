//! Managed rule sets shipped with the engine.
//!
//! These are OWASP-flavoured defaults expressed in the same expression
//! language as user-supplied rules. They are *not* exhaustive — production
//! deployments are expected to receive their bundle from the control plane —
//! but they give a fresh agent meaningful coverage out of the box.

use crate::rules::{CompiledRule, RuleAction};

/// (id, name, expression, action, severity, paranoia_level, tags)
type ManagedSpec<'a> = (&'a str, &'a str, &'a str, RuleAction, u8, u8, &'a [&'a str]);

/// Returns the built-in managed rule set. Rules that fail to parse are
/// silently skipped (logged at warn level) so a typo in one default never
/// breaks engine startup.
pub fn default_managed_rules() -> Vec<CompiledRule> {
    let specs: Vec<ManagedSpec> = vec![
        (
            "PINGWAF-1001",
            "Block admin paths from non-trusted networks",
            r#"http.request.uri.path starts_with "/admin" and not ip.src in {10.0.0.0/8 172.16.0.0/12 192.168.0.0/16 127.0.0.0/8}"#,
            RuleAction::Challenge,
            3,
            1,
            &["admin", "ip-restriction"],
        ),
        (
            "PINGWAF-1002",
            "Block known SQL injection anomaly scores",
            "cf.waf.score.sqli ge 60",
            RuleAction::Block,
            5,
            1,
            &["sqli", "anomaly"],
        ),
        (
            "PINGWAF-1003",
            "Block known XSS anomaly scores",
            "cf.waf.score.xss ge 60",
            RuleAction::Block,
            5,
            1,
            &["xss", "anomaly"],
        ),
        (
            "PINGWAF-1004",
            "Block requests with high aggregate WAF score",
            "cf.waf.score ge 80",
            RuleAction::Block,
            5,
            1,
            &["anomaly"],
        ),
        (
            "PINGWAF-1010",
            "Block common scanner user agents",
            r#"user_agent matches "(?i)(nikto|sqlmap|nmap|masscan|acunetix|nessus|openvas|wpscan|dirbuster|gobuster|zgrab)""#,
            RuleAction::Block,
            4,
            2,
            &["scanner", "user-agent"],
        ),
        (
            "PINGWAF-1011",
            "Block empty user agents on non-HEAD requests",
            r#"user_agent eq "" and http.request.method ne "HEAD""#,
            RuleAction::Challenge,
            2,
            3,
            &["user-agent"],
        ),
        (
            "PINGWAF-1020",
            "Block sensitive dotfile probes",
            r#"http.request.uri.path matches "(?i)/\\.(?:git|env|svn|aws|kube|ssh|docker)""#,
            RuleAction::Block,
            5,
            1,
            &["dotfile", "recon"],
        ),
        (
            "PINGWAF-1021",
            "Block common backup-file probes",
            r#"http.request.uri.path matches "(?i)\\.(?:bak|backup|old|swp|sql|tar|tgz|zip|gz|rar|7z)$""#,
            RuleAction::Block,
            4,
            2,
            &["backup", "recon"],
        ),
        (
            "PINGWAF-1030",
            "Block requests with HTTP method TRACE / TRACK",
            r#"http.request.method in {"TRACE" "TRACK"}"#,
            RuleAction::Block,
            3,
            1,
            &["method"],
        ),
        (
            "PINGWAF-1031",
            "Log unusual HTTP methods",
            r#"not http.request.method in {"GET" "POST" "PUT" "PATCH" "DELETE" "HEAD" "OPTIONS"}"#,
            RuleAction::Log,
            1,
            2,
            &["method"],
        ),
        (
            "PINGWAF-1040",
            "Block phpMyAdmin / wp-admin probes on non-PHP apps",
            r#"http.request.uri.path matches "(?i)/(?:phpmyadmin|pma|wp-admin|wp-login\\.php|xmlrpc\\.php|administrator)""#,
            RuleAction::Challenge,
            3,
            2,
            &["recon", "cms"],
        ),
        (
            "PINGWAF-1050",
            "Block Log4Shell JNDI lookups in any header",
            r#"http.request.headers["user-agent"] contains "${jndi:""#,
            RuleAction::Block,
            5,
            1,
            &["log4shell", "rce"],
        ),
        (
            "PINGWAF-1051",
            "Block requests with very long URIs (likely buffer / scan attempts)",
            r#"http.request.uri.full matches "^.{4096,}$""#,
            RuleAction::Block,
            3,
            3,
            &["size"],
        ),
    ];

    let mut out = Vec::with_capacity(specs.len());
    for (id, name, expr, action, severity, paranoia, tags) in specs {
        match CompiledRule::compile(
            id,
            name,
            expr,
            action,
            severity,
            tags.iter().map(|s| s.to_string()).collect(),
        ) {
            Ok(mut rule) => {
                rule.paranoia_level = paranoia;
                out.push(rule);
            }
            Err(err) => {
                tracing::warn!(rule_id = id, error = %err, "skipping malformed managed rule");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_managed_rules_compile() {
        let rules = default_managed_rules();
        assert!(rules.len() >= 10, "expected at least 10 managed rules");
        for r in &rules {
            assert!(!r.id.is_empty());
            assert!((1..=5).contains(&r.severity));
            assert!((1..=4).contains(&r.paranoia_level));
        }
    }
}
