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

#[derive(Debug, Clone)]
pub struct SignaturePattern {
    pub id: String,
    pub category: AttackCategory,
    /// 1 (info) … 5 (critical).
    pub severity: u8,
    pub description: String,
    /// Literal needle registered in the Aho-Corasick automaton.
    pub needle: &'static str,
}

#[derive(Debug, Clone)]
pub struct SignatureHit {
    pub pattern_id: String,
    pub category: AttackCategory,
    pub severity: u8,
    pub description: String,
    /// Byte offset where the needle was found.
    pub offset: usize,
    /// Length of the matched needle in bytes.
    pub length: usize,
}

/// Built-in needle table. Patterns are kept short and high-signal so the
/// automaton stays compact and false positives remain rare.
fn builtin_patterns() -> Vec<SignaturePattern> {
    let mut v: Vec<SignaturePattern> = Vec::with_capacity(120);
    // Scoped so the closure's mutable borrow of `v` ends before it is moved
    // out below.
    {
        let mut push = |id: &str,
                        cat: AttackCategory,
                        sev: u8,
                        desc: &str,
                        needle: &'static str| {
            v.push(SignaturePattern {
                id: id.to_string(),
                category: cat,
                severity: sev,
                description: desc.to_string(),
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
        push(
            "XXE-001",
            AttackCategory::Xxe,
            5,
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
            4,
            "BENCHMARK function",
            "benchmark(",
        );
        push(
            "SQL-005",
            AttackCategory::SqlInjection,
            4,
            "pg_sleep function",
            "pg_sleep(",
        );
        push(
            "SQL-006",
            AttackCategory::SqlInjection,
            4,
            "waitfor delay",
            "waitfor delay",
        );
        push(
            "SQL-007",
            AttackCategory::SqlInjection,
            4,
            "load_file function",
            "load_file(",
        );
        push(
            "SQL-008",
            AttackCategory::SqlInjection,
            4,
            "INTO OUTFILE",
            "into outfile",
        );
        push(
            "SQL-009",
            AttackCategory::SqlInjection,
            4,
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
    v
}

/// Aho-Corasick automaton over the built-in pattern table.
pub struct SignatureEngine {
    aho: AhoCorasick,
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
    /// Build the engine with the built-in pattern table. The automaton is
    /// constructed once and reused for every request.
    pub fn new() -> Self {
        let patterns = builtin_patterns();
        let needles: Vec<&str> = patterns.iter().map(|p| p.needle).collect();
        let aho = AhoCorasickBuilder::new()
            .ascii_case_insensitive(true)
            .build(&needles)
            .expect("aho-corasick build cannot fail with valid UTF-8 needles");
        Self { aho, patterns }
    }

    pub fn pattern_count(&self) -> usize {
        self.patterns.len()
    }

    /// Scan `haystack` and return every signature that fired. Results are
    /// de-duplicated by pattern id so a payload repeating the same needle
    /// doesn't multiply the score.
    pub fn scan(&self, haystack: &str) -> Vec<SignatureHit> {
        if haystack.is_empty() {
            return Vec::new();
        }
        let mut hits: Vec<SignatureHit> = Vec::new();
        let mut seen: Vec<u32> = Vec::new();
        for m in self.aho.find_iter(haystack) {
            let pid = m.pattern().as_u32();
            if seen.contains(&pid) {
                continue;
            }
            seen.push(pid);
            let p = &self.patterns[pid as usize];
            hits.push(SignatureHit {
                pattern_id: p.id.clone(),
                category: p.category,
                severity: p.severity,
                description: p.description.clone(),
                offset: m.start(),
                length: m.end() - m.start(),
            });
        }
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

/// Detect SQL injection using token fingerprinting plus targeted regex checks.
///
/// Returns `(is_sqli, fingerprint)`. The fingerprint encodes the
/// libinjection-style token signature followed by the names of every check
/// that fired. Only *strong* signals (structural SQL syntax) flip `is_sqli`;
/// weak signals such as a trailing comment are recorded in the fingerprint
/// but never block a request on their own, which keeps prose like
/// `"C# programming"` or `"well--done"` from tripping the detector.
pub fn detect_sqli(input: &str) -> (bool, String) {
    if input.len() < 3 {
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

/// Detect XSS by looking for HTML tags / event handlers / dangerous URIs in
/// contexts that should not contain them.
pub fn detect_xss(input: &str) -> (bool, String) {
    if input.len() < 3 {
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
    fn engine_finds_ssrf_metadata() {
        let e = SignatureEngine::new();
        let hits = e.scan("http://169.254.169.254/latest/meta-data/");
        assert!(hits.iter().any(|h| h.pattern_id == "SSRF-001"));
    }

    #[test]
    fn engine_case_insensitive() {
        let e = SignatureEngine::new();
        let hits = e.scan("UNION SELECT password FROM users");
        assert!(hits.iter().any(|h| h.pattern_id == "SQL-001"));
    }

    #[test]
    fn engine_dedupes() {
        let e = SignatureEngine::new();
        let hits = e.scan("../ ../ ../");
        assert_eq!(hits.iter().filter(|h| h.pattern_id == "PT-001").count(), 1);
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
}
