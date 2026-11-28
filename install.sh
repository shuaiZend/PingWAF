#!/bin/bash
# ─────────────────────────────────────────────────────────────────────────────
# PingWAF Installation Script
# ─────────────────────────────────────────────────────────────────────────────
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/shuaiZend/PingWAF/main/install.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/shuaiZend/PingWAF/main/install.sh | bash -s -- --mode agent --server-url https://... --api-key pwk_...
#   ./install.sh                     # interactive prompts (needs a terminal)
#   ./install.sh [--version X.Y.Z] [--mode all-in-one|server|agent|tunnel]
#
# Options:
#   --version         Install a specific version (default: latest)
#   --mode            Operating mode: all-in-one, server, agent, tunnel
#                     (default: all-in-one). Agent mode installs the agent-only
#                     build (no control plane or dashboard) as
#                     /usr/local/bin/pingwaf; agent mode installs the data-plane
#                     build as /usr/local/bin/pingwaf-agent (unit
#                     pingwaf-agent.service); tunnel mode installs the tunnel
#                     connector as /usr/local/bin/pingwaf-tunnel
#   --server-url      Control plane gRPC URL (agent mode)
#   --api-key         Agent enrollment token (agent mode)
#   --token           One-time tunnel pairing token (tunnel mode), shown in the
#                     console's Edge Agents -> Tunnels tab
#   --token-file      Read the pairing token from a file instead (keeps it out
#                     of the process list)
#   --socks5-proxy    Reach the control plane and the edge ingresses through a
#                     SOCKS5 proxy (a host with no direct egress)
#   --admin-email     Seed administrator e-mail (server / all-in-one)
#   --admin-password  Seed administrator password (omit to generate a random one)
#   --tls-sans        Comma separated names of the dashboard certificate
#   --listen-public   Also bind the agent gRPC endpoint on 0.0.0.0 (the admin
#                     API already listens on every interface; gRPC defaults to
#                     127.0.0.1 and only remote agents need it opened)
#   --no-systemd      Skip systemd service creation
#   --interactive     Prompt even when the flags would normally skip the questions
#   --non-interactive Run with the given values and defaults, never prompt
#   --dry-run         Print the resolved plan, then exit without installing
#   --help            Show this help message
# ─────────────────────────────────────────────────────────────────────────────

set -euo pipefail

# ─── Colors & Helpers ─────────────────────────────────────────────────────────
BOLD="$(tput bold 2>/dev/null || printf '')"
RED="$(tput setaf 1 2>/dev/null || printf '')"
GREEN="$(tput setaf 2 2>/dev/null || printf '')"
YELLOW="$(tput setaf 3 2>/dev/null || printf '')"
BLUE="$(tput setaf 4 2>/dev/null || printf '')"
NO_COLOR="$(tput sgr0 2>/dev/null || printf '')"

info()    { printf '%s\n' "${BLUE}>${NO_COLOR} $*"; }
success() { printf '%s\n' "${GREEN}✓${NO_COLOR} $*"; }
warn()    { printf '%s\n' "${YELLOW}! $*${NO_COLOR}"; }
error()   { printf '%s\n' "${RED}✗ $*${NO_COLOR}" >&2; }
fatal()   { error "$@"; exit 1; }

has() { command -v "$1" >/dev/null 2>&1; }

# Cryptographically random lowercase hex string (default 32 bytes / 64 chars).
random_hex() {
    local bytes="${1:-32}"
    if has openssl; then
        openssl rand -hex "$bytes"
    else
        od -An -tx1 -N "$bytes" /dev/urandom | tr -d ' \n'
    fi
}

# Cryptographically random alphanumeric password (32 characters by default).
random_password() {
    local length="${1:-32}" raw=""
    if has openssl; then
        raw="$(openssl rand -base64 48 2>/dev/null | tr -dc 'A-Za-z0-9' || true)"
    else
        raw="$(head -c 48 /dev/urandom | base64 | tr -dc 'A-Za-z0-9' || true)"
    fi
    if (( ${#raw} < length )); then
        raw="${raw}$(head -c 48 /dev/urandom | base64 | tr -dc 'A-Za-z0-9' || true)"
    fi
    printf '%s' "${raw:0:length}"
}

# Percent-decodes one URL component. Only used for the PostgreSQL connectivity
# probe below, where the value is handed to libpq through the environment.
url_decode() {
    local value="$1" out="" char hex i=0
    if [[ "$value" != *%* ]]; then
        printf '%s' "$value"
        return 0
    fi
    while (( i < ${#value} )); do
        char="${value:i:1}"
        if [[ "$char" == "%" ]] && (( i + 2 < ${#value} )); then
            hex="${value:i+1:2}"
            if [[ "$hex" =~ ^[0-9A-Fa-f]{2}$ ]]; then
                out="${out}$(printf "\\x${hex}")"
                i=$((i + 3))
                continue
            fi
        fi
        out="${out}${char}"
        i=$((i + 1))
    done
    printf '%s' "$out"
}

# PostgreSQL connectivity probe that never puts the password on the command
# line: the URL is split into the standard libpq environment variables and
# `psql` is invoked without `-d`. Returns non-zero when the URL cannot be
# parsed or the connection fails.
postgres_reachable() {
    local url="$1" rest userinfo user pass host port hostport path dbname
    rest="${url#*://}"
    [[ "$rest" == "$url" ]] && return 2
    if [[ "$rest" == *@* ]]; then
        userinfo="${rest%@*}"
        rest="${rest##*@}"
    else
        userinfo=""
    fi
    user="${userinfo%%:*}"
    if [[ "$userinfo" == *:* ]]; then
        pass="${userinfo#*:}"
    else
        pass=""
    fi
    hostport="${rest%%/*}"
    hostport="${hostport%%\?*}"
    path=""
    if [[ "$rest" == */* ]]; then
        path="${rest#*/}"
    fi
    host=""
    port=""
    if [[ "$hostport" == \[*\]* ]]; then
        host="${hostport#[}"
        host="${host%%]*}"
        if [[ "$hostport" == *"]:"* ]]; then
            port="${hostport##*]:}"
        fi
    elif [[ "$hostport" == *:* ]]; then
        host="${hostport%%:*}"
        port="${hostport##*:}"
    else
        host="$hostport"
    fi
    dbname="${path%%\?*}"
    [[ -z "$host" ]] && return 2
    PGPASSWORD="$(url_decode "$pass")" \
    PGHOST="$host" \
    PGPORT="${port:-5432}" \
    PGUSER="$(url_decode "${user:-postgres}")" \
    PGDATABASE="$(url_decode "${dbname:-postgres}")" \
        psql -w -c "SELECT 1" >/dev/null 2>&1
}

# True when the value is one of the usual truthy spellings.
is_truthy() {
    case "$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')" in
        1|true|yes|on) return 0 ;;
        *) return 1 ;;
    esac
}

# Host part of a URL, without scheme, credentials, port or path. Handles both
# `host:port` and `[IPv6]:port`, so the warning below can print a usable
# `openssl s_client -connect host:9090` line.
server_host() {
    local rest="${1#*://}"
    rest="${rest%%/*}"
    rest="${rest##*@}"
    if [[ "$rest" == \[* ]]; then
        rest="${rest#[}"
        printf '%s' "${rest%%]*}"
    else
        printf '%s' "${rest%%:*}"
    fi
}

# Loopback control planes are the one place plaintext gRPC is expected.
# `127.*` is deliberately a dotted-quad match, not a prefix: a host like
# `127.evil.com` resolves over DNS and is not loopback.
is_loopback_host() {
    [[ "$1" == "localhost" || "$1" == "::1" ]] && return 0
    [[ "$1" =~ ^127\.[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}$ ]]
}

# ─── Constants ────────────────────────────────────────────────────────────────
REPO="shuaiZend/PingWAF"
# Name the binary is installed under. It stays `pingwaf` in every mode so the
# systemd unit, the config path and an in-place upgrade keep working; what
# changes is which release asset is downloaded (see ASSET_PREFIX below).
BINARY_NAME="pingwaf"
# Release asset/binary name. Agent mode prefers the agent-only build, which
# links the data plane without the control plane (PostgreSQL, REST API, the
# embedded dashboard); server/all-in-one keep the full binary.
ASSET_PREFIX="pingwaf"
ARCHIVE_BINARY="pingwaf"
# True once the agent-only archive was actually fetched (not the fallback).
AGENT_ONLY_BUILD=false
INSTALL_DIR="/usr/local/bin"
CONFIG_DIR="/etc/pingwaf"
DATA_DIR="/var/lib/pingwaf"
SERVICE_NAME="pingwaf"
SUPPORTED_PLATFORMS="linux/amd64 linux/arm64"

# ─── Default Configuration ────────────────────────────────────────────────────
# Empty on purpose: the environment fallbacks below can only take effect when
# these are unset. Assigning the defaults here made `PINGWAF_MODE` and
# `PINGWAF_DB_URL` silently dead.
VERSION=""
MODE=""
SKIP_SYSTEMD=false
DB_URL=""
SERVER_URL=""
SERVER_CA_CERT=""
SERVER_TLS_NAME=""
API_KEY=""
# One-time tunnel pairing token (tunnel mode). Never echoed back.
TUNNEL_TOKEN=""
TUNNEL_TOKEN_FILE=""
SOCKS5_PROXY=""
SOCKS5_USERNAME=""
SOCKS5_PASSWORD=""
ADMIN_EMAIL=""
ADMIN_PASSWORD=""
TLS_SANS=""
# Bind the admin API / gRPC on all interfaces. Off by default: a fresh install
# with the seeded administrator account must not be reachable from the network
# until the operator has changed its password (or put it behind a proxy).
LISTEN_PUBLIC=false
# Instance key for the at-rest sealing of notification/AI/backup credentials.
# Taken from the environment when set; otherwise generated once and persisted
# in ${CONFIG_DIR}/pingwaf.env (0600). Never rotate it silently: previously
# sealed credentials cannot be opened with a new key.
PINGWAF_SECRET_KEY="${PINGWAF_SECRET_KEY:-}"
ADMIN_PASSWORD_GENERATED=false
DRY_RUN=false
# "" decides from the command line, "true"/"false" are explicit opt-ins.
INTERACTIVE=""
# Any value handed in (flag or PINGWAF_*) means the caller already made a
# choice, so an unattended run must not start asking questions.
PARAMS_GIVEN=false

# ─── Argument Parsing ─────────────────────────────────────────────────────────
show_help() {
    cat <<EOF
PingWAF Installation Script

Usage: $(basename "$0") [OPTIONS]

Running it with no options on a terminal starts the interactive installer;
piping it (curl ... | bash) installs non-interactively with the defaults.
Pass options through the pipe with: bash -s -- <options>

Options:
  --version VERSION        Install a specific version (default: latest release)
  --mode MODE              Operating mode: all-in-one, server, agent, tunnel.
                           Agent mode downloads the agent-only build
                           (pingwaf-agent) and installs it as
                           /usr/local/bin/pingwaf. Agent mode installs the
                           data-plane build as /usr/local/bin/pingwaf-agent.
                           Tunnel mode installs the tunnel connector as
                           /usr/local/bin/pingwaf-tunnel
                           and pairs it with --token.
  --server-url URL         Control plane gRPC URL (agent mode)
  --server-ca-cert PATH    PEM file with the CA that signed the control plane's
                           gRPC certificate (agent mode, https)
  --server-tls-name NAME   Name used for SNI and certificate verification when
                           the certificate has no SAN for the URL host (agent
                           mode, requires a release that knows the flag)
  --api-key KEY            Agent enrollment token (agent mode)
  --token TOKEN            One-time tunnel pairing token (tunnel mode)
  --token-file PATH        Read the pairing token from a file instead of
                           --token (keeps it out of `ps`)
  --socks5-proxy ADDR      SOCKS5 proxy for a host with no direct egress
                           (`host:port` or `socks5://user:pass@host:port`)
  --socks5-username USER   SOCKS5 username (wins over embedded userinfo)
  --socks5-password PASS   SOCKS5 password (prefer PINGWAF_TUNNEL_SOCKS5_PASSWORD)
  --db-url URL             PostgreSQL connection string
  --admin-email EMAIL      Seed administrator e-mail (server / all-in-one)
  --admin-password PASS    Seed administrator password. When omitted (and no
                           PINGWAF_ADMIN_PASSWORD) a random password is
                           generated and printed exactly once at the end.
  --tls-sans NAMES         Comma separated dashboard certificate names
  --listen-public          Also bind the agent gRPC endpoint (9090) on
                           0.0.0.0. The dashboard/REST API (9080) already
                           listens on every interface; gRPC stays on
                           127.0.0.1 unless this is given, and only remote
                           agents need it.
  --no-systemd             Skip systemd service file creation
  --interactive            Ask the questions even when options were given
  --non-interactive, --yes Never prompt; use the given values and defaults
  --dry-run                Print the resolved plan, then exit without changes
  --help                   Show this help message

Environment Variables:
  PINGWAF_VERSION       Same as --version
  PINGWAF_MODE          Same as --mode
  PINGWAF_SERVER_URL    Same as --server-url
  PINGWAF_SERVER_CA_CERT Same as --server-ca-cert
  PINGWAF_SERVER_TLS_NAME Same as --server-tls-name
  PINGWAF_API_KEY       Same as --api-key
  PINGWAF_DB_URL        Same as --db-url
  PINGWAF_ADMIN_EMAIL   Same as --admin-email
  PINGWAF_ADMIN_PASSWORD Same as --admin-password
  PINGWAF_TLS_SANS      Same as --tls-sans
  PINGWAF_LISTEN_PUBLIC Same as --listen-public (1/true/yes/on)
  PINGWAF_SECRET_KEY    Instance key that seals stored credentials; generated
                        when unset and kept in /etc/pingwaf/pingwaf.env (0600)

Examples:
  # Interactive: answer the questions, defaults in brackets
  curl -fsSL https://raw.githubusercontent.com/shuaiZend/PingWAF/main/install.sh -o install.sh
  bash install.sh

  # One line, unattended: all-in-one with the defaults
  curl -fsSL https://raw.githubusercontent.com/shuaiZend/PingWAF/main/install.sh | bash

  # One line, unattended: a node as an agent pointing at the control plane
  # (the control plane serves gRPC over TLS by default — use https:// and
  # download its CA from Settings -> Control plane HTTPS, then set it in
  # /etc/pingwaf/pingwaf.toml as agent.server_ca_cert)
  curl -fsSL https://raw.githubusercontent.com/shuaiZend/PingWAF/main/install.sh \
    | bash -s -- --mode agent --server-url https://10.0.0.1:9090 --api-key pwk_xxx

  # Specific version, custom database and administrator
  ./install.sh --version 0.25.0 --db-url "postgres://user:pass@db:5432/pingwaf" \
    --admin-email ops@example.com --tls-sans waf.example.com

After installation:
  Config: /etc/pingwaf/pingwaf.toml   (edit it, then restart the unit:
          systemctl restart pingwaf — or pingwaf-agent on an agent install)
  Data:   /var/lib/pingwaf
EOF
}

require_value() { # $1 option name, $2 next argument
    if [[ -z "${2:-}" ]]; then fatal "Option $1 requires a value (use --help for usage)"; fi
    # `--mode --dry-run` would otherwise swallow the next flag as the value.
    case "$2" in
        --*) fatal "Option $1 requires a value, but got another option: $2" ;;
    esac
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --version)        require_value "$1" "${2:-}"; VERSION="$2"; PARAMS_GIVEN=true; shift 2 ;;
        --mode)           require_value "$1" "${2:-}"; MODE="$2"; PARAMS_GIVEN=true; shift 2 ;;
        --db-url)         require_value "$1" "${2:-}"; DB_URL="$2"; PARAMS_GIVEN=true; shift 2 ;;
        --server-url)     require_value "$1" "${2:-}"; SERVER_URL="$2"; PARAMS_GIVEN=true; shift 2 ;;
        --server-ca-cert) require_value "$1" "${2:-}"; SERVER_CA_CERT="$2"; PARAMS_GIVEN=true; shift 2 ;;
        --server-tls-name) require_value "$1" "${2:-}"; SERVER_TLS_NAME="$2"; PARAMS_GIVEN=true; shift 2 ;;
        --api-key)        require_value "$1" "${2:-}"; API_KEY="$2"; PARAMS_GIVEN=true; shift 2 ;;
        --token)          require_value "$1" "${2:-}"; TUNNEL_TOKEN="$2"; PARAMS_GIVEN=true; shift 2 ;;
        --token-file)     require_value "$1" "${2:-}"; TUNNEL_TOKEN_FILE="$2"; PARAMS_GIVEN=true; shift 2 ;;
        --socks5-proxy)   require_value "$1" "${2:-}"; SOCKS5_PROXY="$2"; PARAMS_GIVEN=true; shift 2 ;;
        --socks5-username) require_value "$1" "${2:-}"; SOCKS5_USERNAME="$2"; PARAMS_GIVEN=true; shift 2 ;;
        --socks5-password) require_value "$1" "${2:-}"; SOCKS5_PASSWORD="$2"; PARAMS_GIVEN=true; shift 2 ;;
        --admin-email)    require_value "$1" "${2:-}"; ADMIN_EMAIL="$2"; PARAMS_GIVEN=true; shift 2 ;;
        --admin-password) require_value "$1" "${2:-}"; ADMIN_PASSWORD="$2"; PARAMS_GIVEN=true; shift 2 ;;
        --tls-sans)       require_value "$1" "${2:-}"; TLS_SANS="$2"; PARAMS_GIVEN=true; shift 2 ;;
        --listen-public)  LISTEN_PUBLIC=true; PARAMS_GIVEN=true; shift ;;
        --no-systemd)     SKIP_SYSTEMD=true; shift ;;
        --interactive)    INTERACTIVE=true; shift ;;
        --non-interactive|--yes) INTERACTIVE=false; PARAMS_GIVEN=true; shift ;;
        --dry-run)        DRY_RUN=true; shift ;;
        --help|-h)        show_help; exit 0 ;;
        *)                fatal "Unknown option: $1 (use --help for usage)" ;;
    esac
done

# Environment variable fallbacks. The defaults are applied *after* the prompts,
# so an unanswered question can fall back to them while an explicit flag wins.
VERSION="${VERSION:-${PINGWAF_VERSION:-}}"
MODE="${MODE:-${PINGWAF_MODE:-}}"
DB_URL="${DB_URL:-${PINGWAF_DB_URL:-}}"
# Agent mode reads its settings from the command line or the environment, so
# these are what the systemd unit is built from.
SERVER_URL="${SERVER_URL:-${PINGWAF_SERVER_URL:-}}"
SERVER_CA_CERT="${SERVER_CA_CERT:-${PINGWAF_SERVER_CA_CERT:-}}"
SERVER_TLS_NAME="${SERVER_TLS_NAME:-${PINGWAF_SERVER_TLS_NAME:-}}"
API_KEY="${API_KEY:-${PINGWAF_API_KEY:-}}"
TUNNEL_TOKEN="${TUNNEL_TOKEN:-${PINGWAF_TUNNEL_TOKEN:-}}"
TUNNEL_TOKEN_FILE="${TUNNEL_TOKEN_FILE:-${PINGWAF_TUNNEL_TOKEN_FILE:-}}"
SOCKS5_PROXY="${SOCKS5_PROXY:-${PINGWAF_TUNNEL_SOCKS5_PROXY:-}}"
SOCKS5_USERNAME="${SOCKS5_USERNAME:-${PINGWAF_TUNNEL_SOCKS5_USERNAME:-}}"
SOCKS5_PASSWORD="${SOCKS5_PASSWORD:-${PINGWAF_TUNNEL_SOCKS5_PASSWORD:-}}"
ADMIN_EMAIL="${ADMIN_EMAIL:-${PINGWAF_ADMIN_EMAIL:-}}"
ADMIN_PASSWORD="${ADMIN_PASSWORD:-${PINGWAF_ADMIN_PASSWORD:-}}"
TLS_SANS="${TLS_SANS:-${PINGWAF_TLS_SANS:-}}"
if [[ "$LISTEN_PUBLIC" != "true" ]] && is_truthy "${PINGWAF_LISTEN_PUBLIC:-}"; then
    LISTEN_PUBLIC=true
fi

# `--version latest` is the documented spelling for "no pin"; the interactive
# prompt already normalises it, the flag path has to do the same or the download
# URL becomes .../download/vlatest/...
if [[ "$VERSION" == "latest" ]]; then VERSION=""; fi

if [[ "$LISTEN_PUBLIC" == "true" ]]; then PARAMS_GIVEN=true; fi
if [[ -n "$VERSION$MODE$DB_URL$SERVER_URL$SERVER_CA_CERT$SERVER_TLS_NAME$API_KEY$TUNNEL_TOKEN$TUNNEL_TOKEN_FILE$SOCKS5_PROXY$ADMIN_EMAIL$ADMIN_PASSWORD$TLS_SANS" ]]; then
    PARAMS_GIVEN=true
fi

# An existing configuration is never rewritten, so the values collected here
# will not be applied — both summaries must say so instead of claiming otherwise.
CONFIG_PRESENT=false
if [[ -f "${CONFIG_DIR}/pingwaf.toml" ]]; then CONFIG_PRESENT=true; fi

# ─── Interactive Prompts ──────────────────────────────────────────────────────
# Every prompt reads fd 3, never stdin: under `curl | bash` fd 0 is the script
# itself, and consuming it would truncate the run with a syntax error.
PROMPT_FD=3
PROMPT_ON=false

prompt_open_tty() {
    # The braces matter: `exec 3<>/dev/tty 2>/dev/null` would also apply the
    # stderr redirect to the shell permanently, swallowing every prompt and
    # error for the rest of the run.
    if ! { exec 3<>/dev/tty; } 2>/dev/null; then
        return 1
    fi
    return 0
}

prompt_line() { # $1 label, $2 default ("" for none), $3 secret (true/false)
    local label="$1" default_value="$2" secret="${3:-false}"
    local suffix=""
    [[ -n "$default_value" ]] && suffix=" [${default_value}]"

    printf '? %s%s: ' "$label" "$suffix" >&2
    if [[ "$secret" == "true" ]]; then
        if ! IFS= read -r -s -u "$PROMPT_FD" REPLY; then
            printf '\n' >&2
            fatal "Input interrupted while reading: ${label}"
        fi
        printf '\n' >&2
    else
        if ! IFS= read -r -u "$PROMPT_FD" REPLY; then
            fatal "Input interrupted while reading: ${label}"
        fi
    fi

    # Trim surrounding whitespace before applying the default.
    REPLY="${REPLY#"${REPLY%%[![:space:]]*}"}"
    REPLY="${REPLY%"${REPLY##*[![:space:]]}"}"
    [[ -z "$REPLY" ]] && REPLY="$default_value"
    return 0
}

prompt_until() { # $1 label, $2 default, $3 secret, $4 validator
    local label="$1" default_value="$2" secret="${3:-false}" validator="$4"
    local tries=0
    while :; do
        prompt_line "$label" "$default_value" "$secret"
        if "$validator" "$REPLY"; then return 0; fi
        tries=$((tries + 1))
        if (( tries >= 3 )); then
            fatal "Too many invalid answers for: ${label}"
        fi
        warn "That value is not accepted — please try again."
    done
}

validate_mode_input() {
    case "$1" in
        1) REPLY="all-in-one" ;;
        2) REPLY="server" ;;
        3) REPLY="agent" ;;
        4) REPLY="tunnel" ;;
        all-in-one|server|agent|tunnel) ;;
        *) return 1 ;;
    esac
    return 0
}

validate_email() { [[ "$1" == *@*.* ]]; }
validate_db_url() { [[ "$1" == postgres://* || "$1" == postgresql://* ]]; }
validate_server_url() { [[ "$1" == http://* || "$1" == https://* ]] && [[ "$1" == *:* ]]; }
validate_required() { [[ -n "$1" ]]; }

validate_yes_no() {
    case "$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')" in
        y|yes) REPLY="y" ;;
        n|no)  REPLY="n" ;;
        *)     return 1 ;;
    esac
    return 0
}

# Values below end up inside a TOML basic string and inside systemd
# `Environment=` lines. A stray newline or quote would break the generated file
# at best and inject a unit directive at worst, so control characters are
# rejected outright and the remaining specials are escaped per format.
reject_control_chars() { # $1 label, $2 value
    if [[ "$2" == *[[:cntrl:]]* ]]; then
        fatal "$1 must not contain control characters (newline, tab, ...)"
    fi
}

toml_string() { # $1 raw value -> escaped TOML basic-string body
    local value="${1//\\/\\\\}"
    value="${value//\"/\\\"}"
    printf '%s' "$value"
}

# Value of `key` in the [agent] section of a TOML file (empty when absent).
#
# In agent mode the config file is authoritative, so the installer has to read
# it before it can decide whether a flag changes anything — writing a second
# copy into the unit only produced a line that never took effect.
agent_config_value() { # $1 file, $2 key
    [[ -f "$1" ]] || return 0
    awk -v key="$2" '
        /^[[:space:]]*\[/ { in_agent = ($0 ~ /^[[:space:]]*\[agent\][[:space:]]*$/); next }
        in_agent && $0 ~ "^[[:space:]]*" key "[[:space:]]*=" {
            line = $0
            sub(/^[^=]*=[[:space:]]*/, "", line)
            sub(/[[:space:]]*#.*$/, "", line)
            gsub(/^"|"$/, "", line)
            print line
            exit
        }
    ' "$1"
}

# Sets `key` inside [agent], replacing the line or appending it to the section.
# Returns 0 only when the file actually changed.
agent_config_set() { # $1 file, $2 key, $3 value
    local file="$1" key="$2" value="$3"
    [[ -f "$file" && -n "$value" ]] || return 1
    [[ "$(agent_config_value "$file" "$key")" == "$value" ]] && return 1
    local tmp
    tmp="$(mktemp "${file}.XXXXXX")" || return 1
    if ! awk -v key="$key" -v value="$(toml_string "$value")" '
        /^[[:space:]]*\[/ {
            if (in_agent && !done) { printf "%s = \"%s\"\n", key, value; done = 1 }
            in_agent = ($0 ~ /^[[:space:]]*\[agent\][[:space:]]*$/)
            print
            next
        }
        in_agent && !done && $0 ~ "^[[:space:]]*" key "[[:space:]]*=" {
            printf "%s = \"%s\"\n", key, value
            done = 1
            next
        }
        { print }
        END { if (in_agent && !done) printf "%s = \"%s\"\n", key, value }
    ' "$file" > "$tmp"; then
        rm -f "$tmp"
        return 1
    fi
    mv "$tmp" "$file"
    # Confirm the write landed inside [agent]: a file without that section is
    # left alone rather than reported as updated.
    [[ "$(agent_config_value "$file" "$key")" == "$value" ]]
}

systemd_env() { # $1 variable name, $2 value -> Environment="NAME=value"
    local value="${2//\\/\\\\}"
    value="${value//\"/\\\"}"
    # Unit files expand % specifiers, so a literal percent must be doubled.
    value="${value//%/%%}"
    printf 'Environment="%s=%s"' "$1" "$value"
}

# ─── mimalloc arena sizing ────────────────────────────────────────────────────
# mimalloc creates its first arena at load time, before `allocator_tune::apply()`
# can call `mi_option_set`; an arena that already exists cannot be shrunk. On a
# small host the default 1 GiB reserve is therefore what the process keeps (and
# what `pmap` shows), even though the startup log prints the adaptive value.
# The unit must carry the reserve in its **process environment**, which mimalloc
# parses in `_mi_options_init` before the first arena.
#
# The buckets mirror `src/allocator_tune.rs` (`arena_reserve_for` /
# `eager_commit_for`) and use the same memory ceiling: cgroup v2 limit, then
# cgroup v1, then host `MemTotal`. Unknown/failed detection takes the small-host
# bucket (64 MiB, eager commit off), the safe direction.
detect_memory_ceiling_bytes() {
    local raw=""

    # cgroup v2: "max" means no limit; fall through to the host total.
    if [[ -r /sys/fs/cgroup/memory.max ]]; then
        raw="$(cat /sys/fs/cgroup/memory.max 2>/dev/null || true)"
        if [[ "$raw" =~ ^[0-9]+$ ]] && (( raw > 0 )); then
            printf '%s' "$raw"
            return 0
        fi
    fi

    # cgroup v1: "infinity" is the huge sentinel, not a real limit.
    if [[ -r /sys/fs/cgroup/memory/memory.limit_in_bytes ]]; then
        raw="$(cat /sys/fs/cgroup/memory/memory.limit_in_bytes 2>/dev/null || true)"
        if [[ "$raw" =~ ^[0-9]+$ ]] && (( raw > 0 && raw < 1152921504606846976 )); then
            printf '%s' "$raw"
            return 0
        fi
    fi

    # Host total. `MemAvailable` is deliberately not used here: it fluctuates
    # with the install-time workload, while the arena is a virtual reserve that
    # should follow the same ceiling `allocator_tune` reads at runtime.
    if [[ -r /proc/meminfo ]]; then
        local kb
        kb="$(awk '/^MemTotal:/{print $2; exit}' /proc/meminfo 2>/dev/null || true)"
        if [[ "$kb" =~ ^[0-9]+$ ]] && (( kb > 0 )); then
            printf '%s' "$((kb * 1024))"
            return 0
        fi
    fi

    return 1
}

# 64 MiB below 1 GiB, 256 MiB below 4 GiB, 512 MiB up to 16 GiB, else 1 GiB.
allocator_arena_reserve_for() {
    local total="${1:-0}" mib
    mib=$(( total / 1048576 ))
    if (( mib < 1024 )); then
        printf '%s' "$((64 * 1024 * 1024))"
    elif (( mib < 4096 )); then
        printf '%s' "$((256 * 1024 * 1024))"
    elif (( mib < 16384 )); then
        printf '%s' "$((512 * 1024 * 1024))"
    else
        printf '%s' "$((1024 * 1024 * 1024))"
    fi
}

# Eager commit is only worth it when the ceiling is at least 4 GiB; below that
# the reservation must not be committed up front.
allocator_eager_commit_for() {
    local total="${1:-0}"
    if (( total / 1048576 >= 4096 )); then
        printf '1'
    else
        printf '0'
    fi
}

# Two `Environment=` lines for the unit, computed from the same ceiling.
allocator_env_lines() {
    local ceiling reserve eager
    ceiling="$(detect_memory_ceiling_bytes || true)"
    reserve="$(allocator_arena_reserve_for "$ceiling")"
    eager="$(allocator_eager_commit_for "$ceiling")"
    printf '%s\n' "$(systemd_env MIMALLOC_ARENA_RESERVE "$reserve")"
    printf '%s\n' "$(systemd_env MIMALLOC_ARENA_EAGER_COMMIT "$eager")"
}

validate_install_values() {
    reject_control_chars "mode" "$MODE"
    reject_control_chars "--version" "$VERSION"
    reject_control_chars "--db-url" "$DB_URL"
    reject_control_chars "--server-url" "$SERVER_URL"
    reject_control_chars "--server-ca-cert" "$SERVER_CA_CERT"
    reject_control_chars "--server-tls-name" "$SERVER_TLS_NAME"
    reject_control_chars "--api-key" "$API_KEY"
    reject_control_chars "--token" "$TUNNEL_TOKEN"
    reject_control_chars "--token-file" "$TUNNEL_TOKEN_FILE"
    reject_control_chars "--socks5-proxy" "$SOCKS5_PROXY"
    reject_control_chars "--admin-email" "$ADMIN_EMAIL"
    reject_control_chars "--admin-password" "$ADMIN_PASSWORD"
    reject_control_chars "--tls-sans" "$TLS_SANS"
}

prompt_admin_password() {
    local first second
    for _ in 1 2 3; do
        prompt_line "Administrator password (empty generates a random one, printed once)" "" true
        first="$REPLY"
        if [[ -z "$first" ]]; then
            ADMIN_PASSWORD="$(random_password)"
            ADMIN_PASSWORD_GENERATED=true
            warn "A random administrator password will be generated and printed once at the end."
            return 0
        fi
        prompt_line "Repeat administrator password" "" true
        second="$REPLY"
        if [[ "$first" != "$second" ]]; then
            warn "The passwords do not match."
            continue
        fi
        if (( ${#first} < 8 )); then
            warn "Use at least 8 characters."
            continue
        fi
        ADMIN_PASSWORD="$first"
        return 0
    done
    fatal "Too many invalid password attempts."
}

mask_db_url() {
    # Drop the whole userinfo block: a password may itself contain `@` or `:`,
    # which a naive `[^@]*` pattern would leave half-visible.
    printf '%s' "$1" | sed -E 's#^([a-zA-Z][a-zA-Z0-9+.-]*://)[^/]*@#\1***@#'
}

# The release asset to fetch for a mode: agent mode has its own build, the
# other modes share the full binary. Kept as a function so the dry-run summary
# (which runs before the final mode validation) and the download agree.
asset_prefix() {
    case "${1:-}" in
        agent) printf 'pingwaf-agent' ;;
        tunnel) printf 'pingwaf-tunnel' ;;
        *) printf 'pingwaf' ;;
    esac
}

print_plan_summary() {
    local systemd_state="no"
    local password_state="random (generated at install, printed once)"
    local api_key_state="unset"

    if [[ "$SKIP_SYSTEMD" != "true" && "${OS:-}" == "linux" ]]; then
        systemd_state="yes (User=pingwaf, CAP_NET_BIND_SERVICE for 80/443)"
    fi
    if [[ -n "$ADMIN_PASSWORD" && "$ADMIN_PASSWORD_GENERATED" != "true" ]]; then
        password_state="custom"
    fi
    if [[ -n "$API_KEY" ]]; then
        # Never echo key material, not even a prefix: the summary often ends up
        # pasted into an issue or a chat.
        api_key_state="set (${#API_KEY} chars)"
    fi

    echo ""
    echo "${BOLD}Installation plan${NO_COLOR}"
    echo "  mode:        ${MODE:-all-in-one}"
    echo "  version:     ${VERSION:-latest release}"
    if [[ -n "${PLATFORM:-}" ]]; then
        echo "  platform:    ${PLATFORM}"
    fi
    echo "  binary:      ${INSTALL_DIR}/${BINARY_NAME}"
    if [[ -n "${PLATFORM:-}" ]]; then
        echo "  asset:       $(asset_prefix "${MODE:-all-in-one}")-${OS}-${ARCH}.tar.xz"
    fi
    if [[ "${OS:-}" == "linux" ]]; then
        local mem_ceiling mem_reserve mem_eager
        mem_ceiling="$(detect_memory_ceiling_bytes || true)"
        mem_reserve="$(allocator_arena_reserve_for "$mem_ceiling")"
        mem_eager="$(allocator_eager_commit_for "$mem_ceiling")"
        echo "  malloc:      arena_reserve=$((mem_reserve / 1048576))MiB eager_commit=${mem_eager} (ceiling=$(( ${mem_ceiling:-0} / 1048576 ))MiB)"
    fi
    if [[ "${MODE:-}" == "tunnel" ]]; then
        echo "  config:      ${CONFIG_DIR}/tunnel.toml"
    else
        echo "  config:      ${CONFIG_DIR}/pingwaf.toml"
    fi
    echo "  data:        ${DATA_DIR}"
    echo "  systemd:     ${systemd_state}"
    if [[ "${MODE:-all-in-one}" == "tunnel" ]]; then
        echo "  control:     ${SERVER_URL}"
        if [[ -n "$SERVER_CA_CERT" ]]; then
            echo "  control CA:  ${SERVER_CA_CERT} -> ${CONFIG_DIR}/tunnel-ca.pem"
        else
            echo "  control CA:  none (bundled webpki roots)"
        fi
        echo "  tls name:    ${SERVER_TLS_NAME:-URL host}"
        echo "  socks5:      ${SOCKS5_PROXY:-none (direct egress)}"
        if [[ -n "$TUNNEL_TOKEN_FILE" ]]; then
            echo "  token:       from ${TUNNEL_TOKEN_FILE}"
        else
            echo "  token:       set (${#TUNNEL_TOKEN} chars)"
        fi
        echo "  service:     ${SERVICE_NAME}"
    elif [[ "${MODE:-all-in-one}" == "agent" ]]; then
        echo "  service:     ${SERVICE_NAME}"
        # In agent mode the config file wins, so show what the agent will
        # actually dial — the flag's value only when it is about to replace it.
        local effective_url="${SERVER_URL}"
        local url_source=""
        if [[ "$CONFIG_PRESENT" == "true" ]]; then
            local file_url
            file_url="$(agent_config_value "${CONFIG_DIR}/pingwaf.toml" server_url)"
            if [[ -n "$file_url" ]]; then
                if [[ "$AGENT_SERVER_URL_GIVEN" == "true" && "$file_url" != "$SERVER_URL" ]]; then
                    url_source=" (updates [agent].server_url in the existing config)"
                else
                    effective_url="$file_url"
                    url_source=" (from the existing config file)"
                fi
            fi
        fi
        echo "  control:     ${effective_url}${url_source}"
        if [[ -n "$SERVER_CA_CERT" ]]; then
            echo "  control CA:  ${SERVER_CA_CERT} -> ${CONFIG_DIR}/grpc-ca.pem"
        else
            echo "  control CA:  none (bundled webpki roots)"
        fi
        echo "  tls name:    ${SERVER_TLS_NAME:-URL host}"
        echo "  api key:     ${api_key_state}"
    else
        echo "  database:    $(mask_db_url "${DB_URL:-postgres://pingwaf:pingwaf@localhost:5432/pingwaf}")"
        echo "  admin:       ${ADMIN_EMAIL:-admin@pingwaf.local} (password: ${password_state})"
        if [[ "$LISTEN_PUBLIC" == "true" ]]; then
            echo "  listen:      0.0.0.0:9080 (admin) / 0.0.0.0:9090 (gRPC) — restrict 9080 to the admin network"
        else
            echo "  listen:      0.0.0.0:9080 (admin) / 127.0.0.1:9090 (gRPC) — gRPC loopback until --listen-public"
        fi
        echo "  tls sans:    ${TLS_SANS:-none (the generated certificate covers the hostname used to reach it)}"
    fi
    if [[ "$CONFIG_PRESENT" == "true" ]]; then
        echo "  ${YELLOW}!${NO_COLOR} ${CONFIG_DIR}/pingwaf.toml already exists: the admin, TLS and database values above will NOT be written."
    fi
    echo ""
}

run_interactive_prompts() {
    echo ""
    echo "${BOLD}PingWAF interactive installer${NO_COLOR}"
    echo "Press Enter to accept the value in brackets."
    echo ""
    echo "Deployment mode:"
    echo "  1) all-in-one  control plane and agent in one process (needs PostgreSQL)"
    echo "  2) server      control plane only"
    echo "  3) agent       data-plane node, connects to a control plane"
    echo "  4) tunnel      tunnel connector for a service inside a private network"
    echo ""
    prompt_until "Mode [1-4 or name]" "all-in-one" false validate_mode_input
    MODE="$REPLY"

    if [[ "$MODE" == "agent" ]]; then
        prompt_until "Control plane gRPC URL (https://host:9090)" "$SERVER_URL" false validate_server_url
        SERVER_URL="$REPLY"
        prompt_until "Agent enrollment API key" "" true validate_required
        API_KEY="$REPLY"
    else
        prompt_until "Administrator e-mail" "${ADMIN_EMAIL:-admin@pingwaf.local}" false validate_email
        ADMIN_EMAIL="$REPLY"
        prompt_admin_password
        prompt_until "PostgreSQL URL" "${DB_URL:-postgres://pingwaf:pingwaf@localhost:5432/pingwaf}" false validate_db_url
        DB_URL="$REPLY"
        prompt_line "Dashboard certificate names (comma separated, empty for none)" "$TLS_SANS" false
        TLS_SANS="${REPLY// /}"
    fi

    prompt_line "Version (empty or 'latest' = latest release)" "${VERSION:-latest}" false
    VERSION="$REPLY"
    if [[ "$VERSION" == "latest" ]]; then VERSION=""; fi

    print_plan_summary
    prompt_until "Continue? [Y/n]" "y" false validate_yes_no
    if [[ "$REPLY" != "y" ]]; then
        info "Aborted — nothing was changed."
        exit 0
    fi
}

decide_interaction() {
    if [[ "$INTERACTIVE" == "true" ]]; then
        if ! prompt_open_tty; then
            fatal "Interactive mode needs a terminal, but /dev/tty is unavailable."
        fi
        PROMPT_ON=true
        return 0
    fi
    if [[ "$INTERACTIVE" == "false" ]]; then return 0; fi
    if [[ "$PARAMS_GIVEN" == "true" ]]; then return 0; fi
    if [[ ! -t 0 ]]; then
        info "No terminal on stdin (curl | bash): installing non-interactively with the defaults."
        info "Pass options through the pipe with: bash -s -- --mode server --db-url ..."
        return 0
    fi
    if ! prompt_open_tty; then return 0; fi
    PROMPT_ON=true
    return 0
}

decide_interaction
if [[ "$PROMPT_ON" == "true" ]]; then
    run_interactive_prompts
fi

MODE="${MODE:-all-in-one}"
DB_URL="${DB_URL:-postgres://pingwaf:pingwaf@localhost:5432/pingwaf}"
ADMIN_EMAIL="${ADMIN_EMAIL:-admin@pingwaf.local}"
# No built-in default: an empty value is replaced by a generated password after
# the dry-run check below, so a non-interactive install never seeds the public
# `pingwaf123` account.
ADMIN_PASSWORD="${ADMIN_PASSWORD:-}"
TLS_SANS="${TLS_SANS// /}"

# Validate mode
case "$MODE" in
    all-in-one|server|agent|tunnel) ;;
    *) fatal "Invalid mode: $MODE (must be: all-in-one, server, agent, tunnel)" ;;
esac

# The tunnel connector is its own binary with its own unit and config: it
# serves no sites, holds no database and must not replace the data-plane
# binary an edge already runs.
if [[ "$MODE" == "tunnel" ]]; then
    BINARY_NAME="pingwaf-tunnel"
    SERVICE_NAME="pingwaf-tunnel"
fi

# Agent mode installs the data-plane-only build under its own name: the archive
# already carries `pingwaf-agent`, the release names it that way, and an edge
# node that only runs the data plane should not have a `pingwaf` binary that
# looks like it could serve the console. `--mode agent` therefore installs
# /usr/local/bin/pingwaf-agent and pingwaf-agent.service; server/all-in-one keep
# `pingwaf`.
if [[ "$MODE" == "agent" ]]; then
    BINARY_NAME="pingwaf-agent"
    SERVICE_NAME="pingwaf-agent"
fi
ASSET_PREFIX="$(asset_prefix "$MODE")"
ARCHIVE_BINARY="$ASSET_PREFIX"

validate_install_values

# Which agent values the operator supplied (flag or environment) before the
# defaults fill them in: only an explicit value may update an existing config
# file, or a bare `install.sh --mode agent` would rewrite the operator's URL
# with the 127.0.0.1 default.
AGENT_SERVER_URL_GIVEN=false; [[ -n "$SERVER_URL" ]] && AGENT_SERVER_URL_GIVEN=true
AGENT_API_KEY_GIVEN=false; [[ -n "$API_KEY" ]] && AGENT_API_KEY_GIVEN=true
AGENT_SERVER_CA_GIVEN=false; [[ -n "$SERVER_CA_CERT" ]] && AGENT_SERVER_CA_GIVEN=true
AGENT_TLS_NAME_GIVEN=false; [[ -n "$SERVER_TLS_NAME" ]] && AGENT_TLS_NAME_GIVEN=true

# Agents and tunnel connectors talk to the control plane, never to PostgreSQL.
if [[ "$MODE" == "agent" || "$MODE" == "tunnel" ]]; then
    SERVER_URL="${SERVER_URL:-http://127.0.0.1:9090}"
    if [[ "$MODE" == "agent" && -z "$API_KEY" ]]; then
        warn "No --api-key given: start the agent with PINGWAF_API_KEY set, or it cannot register."
    fi
    if [[ "$MODE" == "tunnel" && -z "$TUNNEL_TOKEN" && -z "$TUNNEL_TOKEN_FILE" ]]; then
        fatal "--mode tunnel needs --token (or --token-file): the one-time pairing token from Edge Agents -> Tunnels."
    fi
    if [[ -n "$TUNNEL_TOKEN_FILE" && ! -r "$TUNNEL_TOKEN_FILE" ]]; then
        fatal "--token-file ${TUNNEL_TOKEN_FILE} is not readable."
    fi
    # Schemes are case-insensitive; `HTTP://` must not slip past the warning.
    # `tr` rather than `${var,,}`: macOS ships bash 3.2, which lacks it.
    _cp_scheme="$(printf '%s' "$SERVER_URL" | tr '[:upper:]' '[:lower:]')"
    if [[ "$_cp_scheme" == http:* && -n "$_cp_scheme" ]]; then
        _cp_host="$(server_host "$SERVER_URL")"
        if [[ -z "$_cp_host" ]]; then
            warn "Could not parse a host out of --server-url '${SERVER_URL}'; expected http://host:9090."
        elif is_loopback_host "$_cp_host"; then
            warn "Plain http:// is accepted only because ${_cp_host} is loopback: a control plane on another host serves gRPC over TLS by default."
        else
            warn "Plain http:// to ${_cp_host} fails against a default control plane: the gRPC port serves TLS, so the agent only reports an opaque h2 protocol error."
            warn "  Symptom: journalctl -u pingwaf shows 'Connection to control plane failed' with FRAME_SIZE_ERROR."
            warn "  Fix:     reinstall with --server-url https://${_cp_host}:9090 --server-ca-cert <control-plane.pem> (add --server-tls-name <SAN> when the certificate does not name ${_cp_host})."
            warn "  Verify:  openssl s_client -connect ${_cp_host}:9090 -servername ${_cp_host} -alpn h2"
        fi
    fi
    if [[ -n "$SERVER_CA_CERT" && ! -r "$SERVER_CA_CERT" ]]; then
        fatal "--server-ca-cert ${SERVER_CA_CERT} is not readable. Download the control plane certificate (Settings -> Control plane HTTPS) and point this option at the PEM file."
    fi
    if [[ -n "$SERVER_TLS_NAME" && -z "$SERVER_CA_CERT" ]]; then
        warn "--server-tls-name only changes certificate verification; the control plane's CA still has to be trusted (use --server-ca-cert)."
    fi
    if [[ -n "$SERVER_TLS_NAME" && "$_cp_scheme" == http:* ]]; then
        warn "--server-tls-name has no effect while --server-url is http:// (TLS is not in use)."
    fi
fi

# ─── Platform Detection ───────────────────────────────────────────────────────
detect_os() {
    local os
    os="$(uname -s)"
    case "$os" in
        Linux*)  echo "linux" ;;
        Darwin*) echo "darwin" ;;
        *)       fatal "Unsupported operating system: $os" ;;
    esac
}

detect_arch() {
    local arch
    arch="$(uname -m)"
    case "$arch" in
        x86_64|amd64)  echo "amd64" ;;
        aarch64|arm64) echo "arm64" ;;
        *)             fatal "Unsupported architecture: $arch" ;;
    esac
}

OS="$(detect_os)"
ARCH="$(detect_arch)"
PLATFORM="${OS}/${ARCH}"

info "Detected platform: ${PLATFORM}"

if [[ "$DRY_RUN" == "true" ]]; then
    print_plan_summary
    info "Dry run: nothing was downloaded, installed or written."
    exit 0
fi

# Secrets used by the generated configuration. The administrator password is
# printed exactly once at the end of a successful install; the instance key is
# written to a 0600 environment file the unit reads (never printed).
if [[ -z "$ADMIN_PASSWORD" ]]; then
    ADMIN_PASSWORD="$(random_password)"
    ADMIN_PASSWORD_GENERATED=true
fi
if [[ -z "$PINGWAF_SECRET_KEY" ]]; then
    PINGWAF_SECRET_KEY="$(random_hex 32)"
fi

if [[ "$OS" == "darwin" ]]; then
    fatal "Prebuilt binaries are published for Linux only. On macOS, build from source (see README.md)."
fi

if ! echo "$SUPPORTED_PLATFORMS" | grep -q "$PLATFORM"; then
    fatal "Platform $PLATFORM is not supported. Supported: $SUPPORTED_PLATFORMS"
fi

# ─── Version Resolution ───────────────────────────────────────────────────────
get_latest_version() {
    local url="https://api.github.com/repos/${REPO}/releases/latest"
    if has curl; then
        curl -fsSL "$url" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/'
    elif has wget; then
        wget -qO- "$url" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/'
    else
        fatal "Neither curl nor wget found. Please install one of them."
    fi
}

if [[ -z "$VERSION" ]]; then
    info "Fetching latest release version..."
    VERSION="$(get_latest_version)"
    if [[ -z "$VERSION" ]]; then
        fatal "Failed to determine latest version. Use --version to specify manually."
    fi
fi

# Strip leading 'v' if present
VERSION="${VERSION#v}"
info "Installing PingWAF v${VERSION}"

# ─── Download ─────────────────────────────────────────────────────────────────
# Releases ship both .tar.xz (primary, smaller) and .tar.gz (compatibility
# fallback). Prefer xz; skip straight to .tar.gz when xz(1) is not installed,
# and fall back on a failed download (e.g. releases that predate .tar.xz).

# Verifies one downloaded asset against the published sidecar with
# `sha256sum -c` (or `shasum -a 256 -c` where coreutils is absent).
verify_checksum() {
    local file="$1" sidecar="$2" dir base
    dir="$(dirname "$file")"
    base="$(basename "$sidecar")"
    if has sha256sum; then
        ( cd "$dir" && sha256sum -c "$base" ) >/dev/null 2>&1
    elif has shasum; then
        ( cd "$dir" && shasum -a 256 -c "$base" ) >/dev/null 2>&1
    else
        fatal "Neither sha256sum nor shasum is available; cannot verify the release asset."
    fi
}

# Rejects an archive that would write outside the temp directory (absolute
# paths, `..` components) or that is not the expected single release directory.
verify_archive_members() {
    local archive="$1" list="${TMPDIR}/members.txt" prefix="${ASSET_PREFIX}-${OS}-${ARCH}/"
    case "$archive" in
        *.tar.xz) tar -tJf "$archive" > "$list" ;;
        *.tar.gz) tar -tzf "$archive" > "$list" ;;
        *) fatal "Unexpected archive type: ${archive}" ;;
    esac
    if [[ ! -s "$list" ]]; then
        fatal "The archive is empty: $(basename "$archive")"
    fi
    if grep -Eq '(^/|(^|/)\.\.(/|$))' "$list"; then
        fatal "The archive contains an unsafe path (absolute or '..'); refusing to extract it."
    fi
    if grep -v "^${prefix}" "$list" | grep -q .; then
        fatal "The archive contains entries outside '${prefix}'; refusing to extract it."
    fi
}

fetch_asset() {
    local ext url checksum_url expected downloaded prefix
    # Agent mode prefers the agent-only archive. Releases before it existed
    # only shipped the full binary, so a pinned old version falls back to that
    # (and says so): `--mode agent --version <old>` keeps working.
    local prefixes=("$ASSET_PREFIX")
    if [[ "$MODE" == "agent" && "$ASSET_PREFIX" != "pingwaf" ]]; then
        prefixes+=("pingwaf")
    fi
    for prefix in "${prefixes[@]}"; do
        for ext in tar.xz tar.gz; do
            if [[ "$ext" == "tar.xz" ]] && ! has xz; then
                info "xz(1) not found — falling back to the .tar.gz asset."
                continue
            fi
            ASSET_NAME="${prefix}-${OS}-${ARCH}.${ext}"
            url="https://github.com/${REPO}/releases/download/v${VERSION}/${ASSET_NAME}"
            checksum_url="${url}.sha256"
            info "Downloading ${ASSET_NAME}..."
            info "URL: ${url}"
            downloaded=false
            if has curl; then
                if curl -fsSL --proto '=https' "$url" -o "${TMPDIR}/${ASSET_NAME}" \
                    && curl -fsSL --proto '=https' "$checksum_url" -o "${TMPDIR}/${ASSET_NAME}.sha256"; then
                    downloaded=true
                fi
            elif has wget; then
                if wget -q "$url" -O "${TMPDIR}/${ASSET_NAME}" \
                    && wget -q "$checksum_url" -O "${TMPDIR}/${ASSET_NAME}.sha256"; then
                    downloaded=true
                fi
            else
                fatal "Neither curl nor wget found. Please install one of them."
            fi
            if [[ "$downloaded" != "true" ]]; then
                warn "Download failed for ${ASSET_NAME} (asset or its .sha256) — trying the next format."
                continue
            fi
            # Every release ships a .sha256 sidecar; a missing, malformed or
            # mismatching one is fatal. Nothing is extracted or executed before
            # this check passes.
            expected="$(awk 'NR == 1 { print $1 }' "${TMPDIR}/${ASSET_NAME}.sha256")"
            if [[ ! "$expected" =~ ^[0-9a-fA-F]{64}$ ]]; then
                fatal "The .sha256 sidecar for ${ASSET_NAME} is missing or malformed; refusing to install."
            fi
            if ! verify_checksum "${TMPDIR}/${ASSET_NAME}" "${TMPDIR}/${ASSET_NAME}.sha256"; then
                fatal "Checksum mismatch for ${ASSET_NAME} (published: ${expected}). Refusing to install."
            fi
            if [[ "$prefix" != "$ASSET_PREFIX" ]]; then
                warn "No agent-only build in v${VERSION}; falling back to the full binary (larger footprint)."
                ASSET_PREFIX="$prefix"
                ARCHIVE_BINARY="$prefix"
            fi
            if [[ "$MODE" == "agent" && "$ASSET_PREFIX" == "pingwaf-agent" ]]; then
                AGENT_ONLY_BUILD=true
            fi
            success "Verified the sha256 of ${ASSET_NAME}."
            return 0
        done
    done
    fatal "Download failed. Check that version v${VERSION} exists and has a ${ASSET_PREFIX}-${OS}-${ARCH} asset."
}

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

ASSET_NAME=""
fetch_asset

# ─── Extract & Install Binary ─────────────────────────────────────────────────
# The checksum has already been verified; still refuse archives whose member
# paths could escape the temp directory before tar touches the filesystem.
verify_archive_members "${TMPDIR}/${ASSET_NAME}"

info "Extracting..."
case "$ASSET_NAME" in
    *.tar.xz) tar -xJf "${TMPDIR}/${ASSET_NAME}" -C "$TMPDIR" ;;
    *.tar.gz) tar -xzf "${TMPDIR}/${ASSET_NAME}" -C "$TMPDIR" ;;
    *)        fatal "Unexpected asset name: ${ASSET_NAME}" ;;
esac

# The release archive contains exactly one binary with this name. There is no
# "first executable" fallback: an unexpected archive layout is an error, not an
# invitation to install an arbitrary file.
BINARY_PATH="$(find "${TMPDIR}/${ASSET_PREFIX}-${OS}-${ARCH}" -name "$ARCHIVE_BINARY" -type f | head -n1)"
if [[ -z "$BINARY_PATH" ]]; then
    fatal "Binary '${ARCHIVE_BINARY}' not found in the verified archive."
fi

chmod +x "$BINARY_PATH"

# The release binaries are linked against the glibc of the runner that built
# them (Ubuntu 24.04 today), so an older distro gets as far as systemd before
# failing with a versioned-symbol error. Catch that here instead, while the
# message can still name the cause.
if ! "$BINARY_PATH" --version >/dev/null 2>&1; then
    smoke_error="$("$BINARY_PATH" --version 2>&1 | head -n1 || true)"
    fatal "The downloaded binary cannot run on this system: ${smoke_error}
       Release assets are built on Ubuntu 24.04 and need glibc 2.39 or newer.
       On older distributions (CentOS/RHEL, Debian 11, Ubuntu 20.04, Alpine,
       ...) build from source instead: https://github.com/${REPO}/blob/main/docs/quick-start.md#path-2-build-from-source"
fi
# The agent archive must actually contain the agent-only build. The version
# banner marks it with `edge`; a wrongly packaged (or swapped) artifact would
# otherwise be installed silently and bring the control plane back.
if [[ "$AGENT_ONLY_BUILD" == "true" ]]; then
    # stdout only: the allocator's startup notice goes to stderr and would
    # otherwise be the first line captured here.
    agent_version="$("$BINARY_PATH" --version 2>/dev/null | head -n1 || true)"
    if [[ "$agent_version" != *"edge"* ]]; then
        fatal "The agent-only archive does not contain an agent-only binary: ${agent_version}"
    fi
fi

info "Installing binary to ${INSTALL_DIR}/${BINARY_NAME}..."
# The binary must be root-owned: it is executed by the systemd unit (as the
# unprivileged pingwaf user) and may be run by an operator. `mv` preserves the
# installing user's ownership, so the owner/mode are normalised afterwards.
if [[ -w "$INSTALL_DIR" ]]; then
    mv "$BINARY_PATH" "${INSTALL_DIR}/${BINARY_NAME}"
    if [[ "$(id -u)" -eq 0 ]]; then
        chown root:root "${INSTALL_DIR}/${BINARY_NAME}" 2>/dev/null || true
    else
        warn "Installed as the current user; run: sudo chown root:root ${INSTALL_DIR}/${BINARY_NAME} && sudo chmod 0755 ${INSTALL_DIR}/${BINARY_NAME}"
    fi
    chmod 0755 "${INSTALL_DIR}/${BINARY_NAME}"
elif has sudo; then
    sudo mv "$BINARY_PATH" "${INSTALL_DIR}/${BINARY_NAME}"
    sudo chown root:root "${INSTALL_DIR}/${BINARY_NAME}"
    sudo chmod 0755 "${INSTALL_DIR}/${BINARY_NAME}"
else
    fatal "No write permission to ${INSTALL_DIR} and sudo not available."
fi

# Installs the tunnel connector: pairs it with the control plane and registers
# it as its own unit. Nothing here touches PostgreSQL, the dashboard or the
# data-plane binary an edge already runs.
install_tunnel_connector() {
    local cmd="" config_file="${CONFIG_DIR}/tunnel.toml"
    local ca_dest="${CONFIG_DIR}/tunnel-ca.pem"
    if [[ -w "/" ]]; then
        cmd=""
    elif has sudo; then
        cmd="sudo"
    else
        fatal "Cannot write ${CONFIG_DIR} without sudo."
    fi
    $cmd mkdir -p "$CONFIG_DIR" "${DATA_DIR}/tunnel"
    if [[ "$OS" == "linux" ]] && ! id -u pingwaf >/dev/null 2>&1; then
        $cmd useradd -r -m -d "$DATA_DIR" -s /usr/sbin/nologin pingwaf 2>/dev/null || true
    fi
    if [[ "$OS" == "linux" ]]; then
        $cmd chown -R pingwaf:pingwaf "${DATA_DIR}/tunnel" 2>/dev/null || true
        $cmd chmod 0750 "${DATA_DIR}/tunnel" 2>/dev/null || true
    fi

    # The control-plane CA, if one was supplied, is copied to a durable path:
    # the pairing command references it and the unit must keep working after
    # the operator's download directory is gone.
    local ca_args=()
    if [[ -n "$SERVER_CA_CERT" ]]; then
        $cmd cp "$SERVER_CA_CERT" "$ca_dest" || fatal "Could not copy ${SERVER_CA_CERT} to ${ca_dest}."
        $cmd chmod 0644 "$ca_dest" 2>/dev/null || true
        ca_args=(--server-ca-cert "$ca_dest")
    fi
    local tls_args=()
    if [[ -n "$SERVER_TLS_NAME" ]]; then
        tls_args=(--server-tls-name "$SERVER_TLS_NAME")
    fi

    info "Pairing the tunnel connector with ${SERVER_URL}..."
    # The token is on the command line because it is a one-time secret the
    # operator just pasted; it is redeemed immediately and never stored (the
    # long-lived credential it returns goes into the 0600 config file).
    local token_args=(--token "$TUNNEL_TOKEN")
    if [[ -n "$TUNNEL_TOKEN_FILE" ]]; then
        token_args=(--token-file "$TUNNEL_TOKEN_FILE")
    fi
    local proxy_args=()
    if [[ -n "$SOCKS5_PROXY" ]]; then
        proxy_args=(--socks5-proxy "$SOCKS5_PROXY")
        [[ -n "$SOCKS5_USERNAME" ]] && proxy_args+=(--socks5-username "$SOCKS5_USERNAME")
        # The password is not echoed: it is already on this command line only
        # because the operator passed it, and it is written to the 0600 config.
        [[ -n "$SOCKS5_PASSWORD" ]] && proxy_args+=(--socks5-password "$SOCKS5_PASSWORD")
    fi
    if ! $cmd "${INSTALL_DIR}/${BINARY_NAME}" pair \
        --config "$config_file" \
        ${token_args[@]+"${token_args[@]}"} \
        ${proxy_args[@]+"${proxy_args[@]}"} \
        --server-url "$SERVER_URL" \
        ${ca_args[@]+"${ca_args[@]}"} \
        ${tls_args[@]+"${tls_args[@]}"}; then
        fatal "Pairing failed. Check --server-url/--server-ca-cert, and mint a fresh token in Edge Agents -> Tunnels."
    fi
    $cmd chown root:pingwaf "$config_file" 2>/dev/null || true
    $cmd chmod 0640 "$config_file" 2>/dev/null || true
    success "Paired; configuration written to ${config_file}"

    if [[ "$SKIP_SYSTEMD" == "true" ]]; then
        info "Skipping the systemd unit (--no-systemd)."
    elif [[ "$OS" != "linux" ]]; then
        info "Not Linux — run it with: ${INSTALL_DIR}/${BINARY_NAME} run --config ${config_file}"
    elif ! has systemctl; then
        warn "systemctl not found — run it with: ${INSTALL_DIR}/${BINARY_NAME} run --config ${config_file}"
    else
        local service_file="/etc/systemd/system/${SERVICE_NAME}.service"
        info "Creating systemd service ${SERVICE_NAME}..."
        (umask 077; $cmd tee "$service_file" >/dev/null <<EOF
[Unit]
Description=PingWAF tunnel connector
Documentation=https://github.com/${REPO}
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=pingwaf
Group=pingwaf
ExecStart=${INSTALL_DIR}/${BINARY_NAME} run --config ${config_file}
# The connector retries network and control-plane failures internally, so it
# only exits when it was revoked or told to stop. `on-failure` keeps a revoked
# connector stopped instead of restart-looping against a dead credential.
Restart=on-failure
RestartSec=5
LimitNOFILE=65536
# The connector only dials out and needs no privilege of its own.
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=${DATA_DIR}

[Install]
WantedBy=multi-user.target
EOF
)
        $cmd systemctl daemon-reload
        # `restart` rather than `enable --now`: a re-run pairs a NEW credential,
        # and an already-running unit would keep using the old one (the config
        # file is only read at startup) until someone restarted it by hand.
        if $cmd systemctl enable "$SERVICE_NAME" >/dev/null 2>&1 \
            && $cmd systemctl restart "$SERVICE_NAME"; then
            success "Service ${SERVICE_NAME} is running"
        else
            warn "Could not start ${SERVICE_NAME}; check: journalctl -u ${SERVICE_NAME} -n 50"
        fi
    fi

    echo ""
    echo "${BOLD}Tunnel connector installed${NO_COLOR}"
    echo "  Binary:   ${INSTALL_DIR}/${BINARY_NAME}"
    echo "  Config:   ${config_file}"
    echo "  Control:  ${SERVER_URL}"
    echo "  Socks5:   ${SOCKS5_PROXY:-none}"
    echo "  Status:   ${INSTALL_DIR}/${BINARY_NAME} status --config ${config_file}"
    if [[ "$OS" == "linux" && "$SKIP_SYSTEMD" != "true" ]]; then
        echo "  Logs:     journalctl -u ${SERVICE_NAME} -f"
    fi
    echo ""
    if grep -q '^\[\[ingress\]\]' "$config_file" 2>/dev/null; then
        echo "  Ingress endpoints were advertised by the control plane."
    else
        echo "  ${YELLOW}! No ingress endpoint was advertised.${NO_COLOR}"
        echo "    On the edge that should terminate this tunnel: set"
        echo "    agent.tunnel_addr (e.g. 0.0.0.0:9443) and firewall the port, then"
        echo "    fill in that node's Tunnel ingress address in the console"
        echo "    (Agents -> [node] -> Tunnel ingress) and restart the connector:"
        echo "      sudo systemctl restart ${SERVICE_NAME}"
    fi
    echo ""
    echo "  Next: bind an origin pool to this tunnel in the site's origin pool"
    echo "  settings; until then it advertises no targets."
    echo ""
}

success "Binary installed: ${INSTALL_DIR}/${BINARY_NAME}"

if [[ "$MODE" == "tunnel" ]]; then
    install_tunnel_connector
    exit 0
fi

# ─── Create Directories ───────────────────────────────────────────────────────
create_dirs() {
    local cmd=""
    if [[ -w "/" ]]; then
        cmd=""
    elif has sudo; then
        cmd="sudo"
    else
        warn "Cannot create system directories without sudo."
        return
    fi

    $cmd mkdir -p "$CONFIG_DIR" "$DATA_DIR" "${DATA_DIR}/cache" "${DATA_DIR}/certs"

    # Create pingwaf user if on Linux
    if [[ "$OS" == "linux" ]]; then
        if ! id -u pingwaf >/dev/null 2>&1; then
            $cmd useradd -r -m -d "$DATA_DIR" -s /usr/sbin/nologin pingwaf 2>/dev/null || \
            $cmd adduser -r -d "$DATA_DIR" -s /sbin/nologin pingwaf 2>/dev/null || true
        fi
        # /etc/pingwaf is configuration, not application state: root owns it
        # and only the service group may read it (the unit runs as pingwaf and
        # needs the 0640 config, but must not be able to rewrite its own
        # secrets). /var/lib/pingwaf is the agent's writable state.
        $cmd chown root:pingwaf "$CONFIG_DIR" 2>/dev/null || true
        $cmd chmod 0750 "$CONFIG_DIR" 2>/dev/null || true
        $cmd chown -R pingwaf:pingwaf "$DATA_DIR" 2>/dev/null || true
        $cmd chmod 0750 "$DATA_DIR" 2>/dev/null || true
        $cmd chmod 0750 "${DATA_DIR}/cache" "${DATA_DIR}/certs" 2>/dev/null || true
    fi
}

if [[ "$OS" == "linux" ]]; then
    info "Creating directories..."
    create_dirs
    success "Created ${CONFIG_DIR} and ${DATA_DIR}"
fi

# Copies the control-plane CA the operator pointed at into /etc/pingwaf. The
# generated unit and [agent] table must reference a durable path: a CA kept in
# /tmp or a release download directory disappears on the next reboot, and the
# agent then fails every gRPC handshake with an opaque certificate error.
install_agent_ca() {
    [[ "$MODE" == "agent" ]] || return 0
    [[ -n "$SERVER_CA_CERT" ]] || return 0

    local dest="${CONFIG_DIR}/grpc-ca.pem"
    if [[ "$SERVER_CA_CERT" == "$dest" ]]; then
        return 0
    fi
    if [[ "$(readlink -f "$SERVER_CA_CERT" 2>/dev/null || printf '%s' "$SERVER_CA_CERT")" == \
          "$(readlink -f "$dest" 2>/dev/null || printf '%s' "$dest")" ]]; then
        SERVER_CA_CERT="$dest"
        return 0
    fi

    local cmd=""
    [[ -w "$CONFIG_DIR" ]] || cmd="sudo"
    $cmd cp "$SERVER_CA_CERT" "$dest" || \
        fatal "Could not copy the control-plane CA to ${dest}."
    $cmd chmod 0644 "$dest" 2>/dev/null || true
    $cmd chown root:pingwaf "$dest" 2>/dev/null || true
    SERVER_CA_CERT="$dest"
    success "Control-plane CA installed at ${dest}"
}

install_agent_ca

# ─── Write Default Configuration ─────────────────────────────────────────────
write_config() {
    local config_file="${CONFIG_DIR}/pingwaf.toml"
    if [[ -f "$config_file" ]]; then
        warn "Config file already exists at ${config_file}, leaving it untouched."
        if grep -q '^admin_password = "pingwaf123"' "$config_file" 2>/dev/null; then
            warn "WARNING: ${config_file} still uses the public default administrator password."
            warn "Change it now: pingwaf user reset-password --email ${ADMIN_EMAIL} (or via the dashboard)."
        fi
        warn "Values from this run (control-plane URL, API key, admin account, TLS names,"
        warn "database) were not applied. In agent mode this file is authoritative:"
        warn "edit ${config_file} and run: sudo systemctl restart ${SERVICE_NAME}"
        if [[ "$MODE" == "agent" ]]; then
            local key_state="unset"
            if [[ -n "$API_KEY" ]]; then key_state="set"; fi
            warn "Requested agent settings that were NOT applied: server_url=${SERVER_URL:-unset} api_key=${key_state} server_ca_cert=${SERVER_CA_CERT:-unset} server_tls_name=${SERVER_TLS_NAME:-unset}"
            warn "Put them under [agent] in ${config_file} instead."
        fi
        return
    fi

    local cmd=""
    [[ -w "$CONFIG_DIR" ]] || cmd="sudo"

    # Keys are the command-line flags with dashes replaced by underscores, and
    # the file only fills in what the environment and the command line leave
    # alone (see docs/deployment.md). Settings without a flag — registration,
    # CORS, Elasticsearch, passkeys — stay in the environment.
    local agent_server_url="# agent mode only; all-in-one talks to its own loopback gRPC"
    local agent_api_key="# api_key = \"pwk_...\""
    local agent_ca_cert="# server_ca_cert = \"/etc/pingwaf/grpc-ca.pem\""
    local agent_tls_name="# server_tls_name = \"control-plane\""
    if [[ "$MODE" == "agent" ]]; then
        agent_server_url="server_url = \"$(toml_string "$SERVER_URL")\""
        # The node's credentials live in this file only. The unit used to carry
        # them as well, and the environment silently beat the file, so an
        # operator editing the file saw no effect.
        if [[ -n "$API_KEY" ]]; then
            agent_api_key="api_key = \"$(toml_string "$API_KEY")\""
        fi
        if [[ -n "$SERVER_CA_CERT" ]]; then
            agent_ca_cert="server_ca_cert = \"$(toml_string "$SERVER_CA_CERT")\""
        fi
        if [[ -n "$SERVER_TLS_NAME" ]]; then
            agent_tls_name="server_tls_name = \"$(toml_string "$SERVER_TLS_NAME")\""
        fi
    fi

    # The console listens on every interface. A loopback bind made a fresh
    # install look unreachable from the operator's browser — nothing answered on
    # the host's address, so the install itself appeared to have failed. The
    # exposure is acceptable because the installer generates a random
    # administrator password and a fresh database answers every write with 403
    # `password_change_required` until it is changed; restricting 9080 to the
    # admin network (or fronting it with a reverse proxy) is still recommended.
    #
    # gRPC stays on loopback: only remote agents need it, and they are the
    # deployment that can actually firewall it (`--listen-public` opens it).
    local admin_addr="0.0.0.0:9080" grpc_addr="127.0.0.1:9090"
    if [[ "$LISTEN_PUBLIC" == "true" ]]; then
        grpc_addr="0.0.0.0:9090"
    fi

    local tls_sans_line="# tls_sans = [\"waf.example.com\"]"
    if [[ -n "$TLS_SANS" ]]; then
        local sans_entry sans_joined=""
        local IFS=','
        for sans_entry in $TLS_SANS; do
            [[ -n "$sans_entry" ]] || continue
            [[ -n "$sans_joined" ]] && sans_joined+=", "
            sans_joined+="\"$(toml_string "$sans_entry")\""
        done
        unset IFS
        tls_sans_line="tls_sans = [${sans_joined}]"
    fi

    # `umask 077` for the write itself: tee creates the file 0644 under the
    # default umask, and it holds the admin password and the JWT secret.
    (umask 077; $cmd tee "$config_file" >/dev/null <<EOF
# PingWAF Configuration
# Generated by install.sh on $(date -u +"%Y-%m-%dT%H:%M:%SZ")
# Mode: ${MODE}
#
# This file is read when PINGWAF_CONFIG points at it (the systemd unit sets
# that). In agent mode this file is the source of truth: it wins over
# PINGWAF_* environment variables, while command-line flags still win. Server
# and all-in-one keep the environment as an override. Unknown keys are reported
# at startup and ignored.

[server]
db_url = "$(toml_string "$DB_URL")"
# The console listens on every interface so a fresh install is reachable from
# the operator's browser; restrict 9080 to your admin network or put it behind
# a reverse proxy. gRPC (agent connections) stays on loopback unless
# --listen-public was given, which only remote agents need.
admin_addr = "${admin_addr}"
grpc_addr = "${grpc_addr}"
# gRPC control plane TLS: "tls" (default) serves agents over TLS, reusing the
# dashboard certificate — self-signed on first boot unless grpc_tls_cert/key
# are configured. "off" serves plaintext and is logged as degraded.
# grpc_tls_mode = "tls"
# Dashboard + REST API over HTTPS (passkeys require a secure origin).
# A self-signed certificate is generated on first boot; upload a real one in
# Settings -> Control plane HTTPS. Add the hostname you use to reach the
# dashboard so the generated certificate covers it:
${tls_sans_line}
tls_enabled = true
jwt_secret = "$(head -c 32 /dev/urandom | base64 | tr -dc 'a-zA-Z0-9' | head -c 32)"
admin_email = "$(toml_string "$ADMIN_EMAIL")"
admin_password = "$(toml_string "$ADMIN_PASSWORD")"
serve_frontend = true

# Used by the all-in-one mode and by agents; a server ignores this section.
[agent]
${agent_server_url}
${agent_api_key}
${agent_ca_cert}
${agent_tls_name}
cache_dir = "${DATA_DIR}/cache"
heartbeat_interval_secs = 30
log_batch_size = 100
log_flush_interval_secs = 5
max_body_log_size = 8192
fail_open = true
metrics_ship_interval_secs = 30

# Everything else comes from the environment, e.g.
#   PINGWAF_ALLOW_REGISTRATION=false     PINGWAF_CORS_ORIGINS=https://ui.example.com
#   PINGWAF_JWT_EXPIRATION_HOURS=12      PINGWAF_DB_MAX_CONNECTIONS=20
#   PINGWAF_METRIC_RETENTION_DAYS=30     PINGWAF_WEBHOOK_URL=https://hooks.example.com
#   PINGWAF_PASSKEY_ENABLED=true         PINGWAF_ES_ENABLED=true
EOF
    )

    # Not behind `if [[ -n "$cmd" ]]`: a root install has no sudo wrapper, and
    # skipping the branch left the file with the seeded admin password and the
    # JWT secret readable by every local user (0644, owned by root). Root owns
    # the file; the pingwaf group may read it (the unit runs as User=pingwaf)
    # but cannot rewrite its own secrets.
    if ! $cmd chown root:pingwaf "$config_file" 2>/dev/null; then
        warn "Could not hand ${config_file} to root:pingwaf — does the group exist?"
    fi
    if ! $cmd chmod 0640 "$config_file" 2>/dev/null; then
        warn "Could not chmod 0640 ${config_file}; it may stay readable by other local users."
    fi

    success "Configuration written to ${config_file}"
}

# Writes the instance secret key (env-only setting) to a 0600 environment file
# that the systemd unit reads. An existing key is kept: rotating it silently
# would make every previously sealed notification/AI/backup credential
# unreadable, and there is no automatic re-encryption.
write_secret_env_file() {
    local env_file="${CONFIG_DIR}/pingwaf.env" cmd=""
    [[ -w "$CONFIG_DIR" ]] || cmd="sudo"

    if [[ -f "$env_file" ]] && grep -q '^PINGWAF_SECRET_KEY=' "$env_file" 2>/dev/null; then
        info "Keeping the existing ${env_file} (rotating it would break sealed credentials)."
        return 0
    fi

    (umask 077; $cmd tee "$env_file" >/dev/null <<EOF
# PingWAF environment secrets — mode 0600, read by the systemd unit.
#
# PINGWAF_SECRET_KEY seals notification channel / AI provider / backup target
# credentials at rest. Treat this file like the database itself: rotating the
# key makes previously sealed credentials unreadable (there is no automatic
# re-encryption), so back it up and do not regenerate it on upgrade.
PINGWAF_SECRET_KEY=${PINGWAF_SECRET_KEY}
# The console must not accept sign-ups unless the operator asks for them; the
# unit reads this file, and an explicit `false` beats the built-in default.
PINGWAF_ALLOW_REGISTRATION=false
EOF
    )
    # systemd reads EnvironmentFile as root before dropping to User=pingwaf, so
    # the instance key stays root-only: the service process never needs to read
    # this file itself.
    if ! $cmd chown root:root "$env_file" 2>/dev/null; then
        warn "Could not hand ${env_file} to root:root."
    fi
    if ! $cmd chmod 0600 "$env_file" 2>/dev/null; then
        warn "Could not chmod 0600 ${env_file}; it may stay readable by other local users."
    fi
    success "Instance secret key written to ${env_file} (0600)"
}

if [[ "$OS" == "linux" ]]; then
    write_config
    write_secret_env_file
fi

# ─── Systemd Service ──────────────────────────────────────────────────────────
# An upgrade run (`--no-systemd`) replaces the binary but leaves the unit alone,
# so a unit written before CAP_NET_BIND_SERVICE was granted keeps failing to
# bind 80/443. Point the operator at the drop-in instead of pretending the
# upgrade fixed it. `setcap` is not an alternative here: the unit sets
# NoNewPrivileges=true, which makes the kernel ignore file capabilities.
check_unit_capabilities() {
    local unit_file="/etc/systemd/system/${SERVICE_NAME}.service"
    local dropin_dir="/etc/systemd/system/${SERVICE_NAME}.service.d"
    local granted=false conf

    if [[ -f "$unit_file" ]]; then
        if [[ -r "$unit_file" ]]; then
            if grep -q '^AmbientCapabilities=.*CAP_NET_BIND_SERVICE' "$unit_file"; then
                granted=true
            fi
        elif has sudo; then
            # The unit is written 0600 (it can carry the agent API key), so a
            # non-root upgrade run has to inspect it through sudo.
            if sudo grep -q '^AmbientCapabilities=.*CAP_NET_BIND_SERVICE' "$unit_file" 2>/dev/null; then
                granted=true
            fi
        else
            # Cannot read the unit: do not claim the capability is missing.
            return 0
        fi
    fi
    # `systemctl edit` writes a drop-in, which the main unit file never shows.
    if [[ -d "$dropin_dir" ]]; then
        for conf in "$dropin_dir"/*.conf; do
            [[ -r "$conf" ]] || continue
            if grep -q '^AmbientCapabilities=.*CAP_NET_BIND_SERVICE' "$conf"; then
                granted=true
            fi
        done
    fi
    [[ -f "$unit_file" ]] || return 0
    [[ "$granted" == "true" ]] && return 0
    warn "The existing unit does not grant CAP_NET_BIND_SERVICE: binding 80/443 will fail with \"Permission denied\" (bind() failed on 0.0.0.0:443)."
    echo "  Add it with: sudo systemctl edit ${SERVICE_NAME}"
    echo "    [Service]"
    echo "    AmbientCapabilities=CAP_NET_BIND_SERVICE"
    echo "    CapabilityBoundingSet=CAP_NET_BIND_SERVICE"
}

install_systemd() {
    if [[ "$SKIP_SYSTEMD" == "true" ]]; then
        info "Skipping systemd service installation (--no-systemd)."
        check_unit_capabilities
        return
    fi

    if [[ "$OS" != "linux" ]]; then
        info "macOS detected — skipping systemd. Use launchd or run directly."
        return
    fi

    if ! has systemctl; then
        warn "systemctl not found — skipping service installation."
        return
    fi

    # Agent installs used to live in pingwaf.service next to /usr/local/bin/
    # pingwaf. Both units would now bind the same ports and fight over the
    # config file, so retire the old one before writing the new one. Only an
    # agent unit is touched: a server/all-in-one install keeps pingwaf.service.
    if [[ "$MODE" == "agent" ]]; then
        local legacy_unit="/etc/systemd/system/pingwaf.service"
        if [[ -f "$legacy_unit" ]] && grep -qE '^ExecStart=.*/pingwaf agent( |$)' "$legacy_unit"; then
            info "Retiring the legacy agent unit pingwaf.service (now ${SERVICE_NAME}.service)..."
            local cmd=""
            [[ -w "/etc/systemd/system" ]] || cmd="sudo"
            $cmd systemctl stop pingwaf >/dev/null 2>&1 || true
            $cmd systemctl disable pingwaf >/dev/null 2>&1 || true
            $cmd rm -f "$legacy_unit"
            $cmd systemctl daemon-reload >/dev/null 2>&1 || true
            success "Removed pingwaf.service; the agent now runs as ${SERVICE_NAME}.service."
        fi
    fi

    local service_file="/etc/systemd/system/${SERVICE_NAME}.service"
    local exec_start="${INSTALL_DIR}/${BINARY_NAME} ${MODE}"

    # Settings come from the generated file first, then these environment
    # variables, then the unit's command line. There is no SIGHUP reload: a
    # changed unit or config file takes effect with `systemctl restart`.
    local unit_after="After=network.target postgresql.service"
    local unit_wants="Wants=postgresql.service"
    # Every value is quoted and escaped: an `Environment=` line ends at the
    # newline, so an unescaped key would inject further unit directives.
    local agent_env
    agent_env="$(systemd_env PINGWAF_CONFIG "${CONFIG_DIR}/pingwaf.toml")"
    if [[ "$MODE" == "agent" ]]; then
        unit_after="After=network-online.target"
        unit_wants="Wants=network-online.target"
        # In agent mode the config file is the source of truth: it wins over
        # PINGWAF_* variables. The unit therefore carries no second copy — a
        # duplicate was inert at best, and when it disagreed with the file
        # `systemctl cat` lied about the URL the agent dials. An explicit
        # --server-url/--api-key/--server-ca-cert/--server-tls-name updates
        # the file instead (backed up first), and the unit keeps only
        # PINGWAF_CONFIG.
        if [[ "$CONFIG_PRESENT" == "true" ]]; then
            local cfg="${CONFIG_DIR}/pingwaf.toml"
            local cfg_cmd=""
            [[ -w "$cfg" ]] || cfg_cmd="sudo"
            local updates=0
            [[ "$AGENT_SERVER_URL_GIVEN" == "true" ]] && updates=$((updates + 1))
            [[ "$AGENT_API_KEY_GIVEN" == "true" ]] && updates=$((updates + 1))
            [[ "$AGENT_SERVER_CA_GIVEN" == "true" ]] && updates=$((updates + 1))
            [[ "$AGENT_TLS_NAME_GIVEN" == "true" ]] && updates=$((updates + 1))
            local backup=""
            if [[ "$updates" -gt 0 ]]; then
                backup="${cfg}.bak-$(date -u +%Y%m%dT%H%M%SZ)"
                $cfg_cmd cp -p "$cfg" "$backup" 2>/dev/null || backup=""
            fi
            local cfg_changed=false
            if [[ "$AGENT_SERVER_URL_GIVEN" == "true" ]] \
                && agent_config_set "$cfg" server_url "$SERVER_URL"; then
                cfg_changed=true
                info "Updated [agent].server_url in ${cfg} to ${SERVER_URL}."
            fi
            if [[ "$AGENT_API_KEY_GIVEN" == "true" ]] \
                && agent_config_set "$cfg" api_key "$API_KEY"; then
                cfg_changed=true
                info "Updated [agent].api_key in ${cfg}."
            fi
            if [[ "$AGENT_SERVER_CA_GIVEN" == "true" ]] \
                && agent_config_set "$cfg" server_ca_cert "$SERVER_CA_CERT"; then
                cfg_changed=true
                info "Updated [agent].server_ca_cert in ${cfg} to ${SERVER_CA_CERT}."
            fi
            if [[ "$AGENT_TLS_NAME_GIVEN" == "true" ]] \
                && agent_config_set "$cfg" server_tls_name "$SERVER_TLS_NAME"; then
                cfg_changed=true
                info "Updated [agent].server_tls_name in ${cfg} to ${SERVER_TLS_NAME}."
            fi
            # The cache directory is the installer's, not the operator's: keep
            # it defined so an old file without it still works.
            agent_config_set "$cfg" cache_dir "${DATA_DIR}/cache" && cfg_changed=true
            if [[ "$cfg_changed" == "true" ]]; then
                $cfg_cmd chown root:pingwaf "$cfg" 2>/dev/null || true
                $cfg_cmd chmod 0640 "$cfg" 2>/dev/null || true
                [[ -n "$backup" ]] && info "Previous ${cfg} saved as ${backup}."
            fi
            # A file written before this installer may predate a key and no
            # flag supplied it: say so instead of leaving a silent gap.
            if [[ -z "$(agent_config_value "$cfg" api_key)" ]]; then
                warn "${cfg} has no [agent].api_key and none was given: the agent cannot register until one is added."
            fi
        fi
    else
        # The dashboard is served over HTTPS with a self-signed certificate
        # until a real one is uploaded. Add the hostname it is reached under so
        # the generated certificate covers it:
        # Environment=PINGWAF_TLS_SANS=waf.example.com
        agent_env="${agent_env}
$(systemd_env PINGWAF_TLS_ENABLED "true")"
    fi

    # mimalloc parses these in `_mi_options_init`, before the first arena is
    # created; setting them from `allocator_tune::apply()` is too late on a
    # small host (the default 1 GiB arena cannot be shrunk). The values mirror
    # the Rust buckets, see `allocator_env_lines`.
    local allocator_env
    allocator_env="$(allocator_env_lines)"

    info "Creating systemd service..."

    local cmd=""
    [[ -w "/etc/systemd/system" ]] || cmd="sudo"

    # The unit can carry the agent enrollment key (upgrade path), so it is
    # written private (0600) and only readable by root.
    (umask 077; $cmd tee "$service_file" >/dev/null <<EOF
[Unit]
Description=PingWAF - High Performance Web Application Firewall
Documentation=https://github.com/${REPO}
${unit_after}
${unit_wants}

[Service]
Type=simple
User=pingwaf
Group=pingwaf
ExecStart=${exec_start}
Restart=always
RestartSec=5
LimitNOFILE=65536
${agent_env}
# mimalloc's first arena is created before allocator_tune::apply(); without
# these a small host keeps the 1 GiB default even though the startup log
# prints the adaptive value.
${allocator_env}
# Log level; the default keeps sqlx's per-statement log out of the way:
Environment=RUST_LOG=info,sqlx=warn
# Instance secrets (PINGWAF_SECRET_KEY) live in a 0600 file; the leading dash
# keeps the unit working when the file does not exist (e.g. an agent node).
EnvironmentFile=-${CONFIG_DIR}/pingwaf.env

# The data plane binds 80/443 while running as the unprivileged pingwaf user.
# Ambient capabilities are the only form that survives NoNewPrivileges=true;
# the bounding set is narrowed to the same bit so nothing else can be regained.
AmbientCapabilities=CAP_NET_BIND_SERVICE
CapabilityBoundingSet=CAP_NET_BIND_SERVICE

# Security hardening
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=${DATA_DIR} ${CONFIG_DIR}
PrivateTmp=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictSUIDSGID=true
LockPersonality=true
UMask=0077

[Install]
WantedBy=multi-user.target
EOF
    )
    if ! $cmd chmod 600 "$service_file" 2>/dev/null; then
        warn "Could not chmod 600 ${service_file}; it may be readable by other local users."
    fi

    $cmd systemctl daemon-reload
    success "Systemd service installed: ${SERVICE_NAME}.service"

    # An agent only contributes once it is running, so start it here — this is
    # what makes the dashboard's one-line enrolment command enough on its own.
    # Server modes stay manual: they need PostgreSQL ready and an attended
    # first boot.
    if [[ "$MODE" == "agent" ]]; then
        info "Starting the agent service..."
        # `enable --now` does not restart an already-running unit, so an
        # upgrade would install the new binary and keep the old process (and
        # the old config) alive until the next manual restart. Always restart.
        if $cmd systemctl enable "$SERVICE_NAME" >/dev/null 2>&1 && \
           $cmd systemctl restart "$SERVICE_NAME"; then
            success "Service enabled and restarted: ${SERVICE_NAME}"
        else
            warn "The service did not start — inspect: journalctl -u ${SERVICE_NAME} -e"
            warn "After fixing, start it with: sudo systemctl enable --now ${SERVICE_NAME}"
            return 1
        fi

        # An active unit is not a working node: a wrong key, URL or CA leaves
        # the agent retrying forever while the installer still prints success.
        # Wait for the registration line before claiming the install worked.
        info "Waiting for the agent to register..."
        local start_epoch registered=false
        start_epoch=$(( $(date +%s) - 2 ))
        for _ in $(seq 1 60); do
            if $cmd journalctl -u "$SERVICE_NAME" -b --no-pager \
                --since "@${start_epoch}" 2>/dev/null \
                | grep -q "Registered with control plane"; then
                registered=true
                break
            fi
            sleep 1
        done
        if [[ "$registered" == "true" ]]; then
            success "Agent registered with the control plane."
        else
            warn "The agent service is active but did not register within 60s."
            warn "Check the control-plane URL, API key and CA:"
            warn "  journalctl -u ${SERVICE_NAME} -e"
            return 1
        fi
        return
    fi

    info "To start PingWAF:"
    echo "  sudo systemctl enable --now ${SERVICE_NAME}"
    echo "  sudo systemctl status ${SERVICE_NAME}"
}

if ! install_systemd; then
    error "Installation stopped before the node was verified."
    exit 1
fi

# The package name moves with the distribution: Ubuntu 24.04 ships
# postgresql-16, 26.04 ships 18, Debian tracks its own numbering. Printing the
# one name that only worked on the machine the message was written on sends the
# operator into "Unable to locate package".
postgres_pkg_hint() {
    local pkg=""
    if has apt-get; then
        pkg="$(apt-cache search --names-only '^postgresql-[0-9]+$' 2>/dev/null \
            | awk '{print $1}' | sort -V | tail -n1)"
        echo "sudo apt install ${pkg:-postgresql}"
    elif has dnf; then
        echo "sudo dnf install postgresql-server"
    elif has yum; then
        echo "sudo yum install postgresql-server"
    elif has apk; then
        echo "sudo apk add postgresql"
    else
        echo "(install PostgreSQL from your distribution's package manager)"
    fi
}

# ─── PostgreSQL Check ─────────────────────────────────────────────────────────
# Sets DB_READY so the caller can refuse to report a successful install on a
# machine that cannot reach a database: exit 0 with an inactive service is
# exactly what automation reads as "ready".
DB_READY=false
check_postgres() {
    info "Checking PostgreSQL availability..."

    if has psql; then
        if postgres_reachable "$DB_URL"; then
            success "PostgreSQL connection successful."
            DB_READY=true
        else
            # Never echo the connection string: it carries the DB password.
            warn "psql found but cannot connect to: $(mask_db_url "$DB_URL")"
            echo ""
            echo "  To set up PostgreSQL:"
            echo "    1. Install: $(postgres_pkg_hint)"
            echo "    2. Create user & database:"
            echo "       sudo -u postgres createuser pingwaf"
            echo "       sudo -u postgres createdb -O pingwaf pingwaf"
            echo "       sudo -u postgres psql -c \"ALTER USER pingwaf PASSWORD 'pingwaf';\""
            echo ""
        fi
    else
        warn "PostgreSQL client (psql) not found."
        echo ""
        echo "  PingWAF requires PostgreSQL 14+. Install it with:"
        echo ""
        if [[ "$OS" == "linux" ]]; then
            echo "    $(postgres_pkg_hint)"
        else
            echo "    brew install postgresql"
        fi
        echo ""
        echo "  Then create the database:"
        echo "    sudo -u postgres createuser pingwaf"
        echo "    sudo -u postgres createdb -O pingwaf pingwaf"
        echo "    sudo -u postgres psql -c \"ALTER USER pingwaf PASSWORD 'pingwaf';\""
        echo ""
    fi
}

if [[ "$MODE" == "agent" ]]; then
    info "Agent mode: skipping the PostgreSQL check, nodes keep no database."
    DB_READY=true
else
    check_postgres
    if [[ "$DB_READY" != "true" ]]; then
        echo ""
        warn "Installation files are in place, but PingWAF cannot start without a database."
        warn "Fix PostgreSQL as shown above, then: sudo systemctl enable --now ${SERVICE_NAME}"
        echo ""
        exit 1
    fi
fi

# ─── Firewall Hints ───────────────────────────────────────────────────────────
print_firewall_hints() {
    if [[ "$OS" != "linux" ]]; then return; fi

    echo ""
    if [[ "$MODE" == "agent" ]]; then
        info "Firewall ports:"
        echo "    80/tcp   — HTTP traffic (proxied sites)"
        echo "    443/tcp  — HTTPS traffic (proxied sites)"
        echo "  Outbound to the control plane (${SERVER_URL}) must be allowed."
        echo "  80/443 are bound as User=pingwaf through the unit's CAP_NET_BIND_SERVICE — no root needed."
        echo ""
        if has ufw; then
            echo "  With UFW:"
            echo "    sudo ufw allow 80/tcp"
            echo "    sudo ufw allow 443/tcp"
        fi
        return
    fi

    info "Firewall ports:"
    echo "    80/tcp   — HTTP traffic (proxied sites)"
    echo "    443/tcp  — HTTPS traffic (proxied sites)"
    echo "    9080/tcp — Admin dashboard & REST API (HTTPS) — open by default; restrict it to the admin network"
    echo "    9090/tcp — gRPC control plane (agent connections) — loopback unless --listen-public"
    echo ""
    echo "  The console binds 0.0.0.0 so a fresh install is reachable. Its"
    echo "  administrator password is generated at install time and a fresh"
    echo "  database refuses every write until it is changed — but restrict 9080"
    echo "  to your admin network (or front it with a reverse proxy), and change"
    echo "  the password at the first sign-in."
    echo ""
    if has ufw; then
        echo "  With UFW (business ports):"
        echo "    sudo ufw allow 80/tcp"
        echo "    sudo ufw allow 443/tcp"
        echo "  For the management ports, restrict the source instead of opening them:"
        echo "    sudo ufw allow from <admin-cidr> to any port 9080 proto tcp"
        echo "    sudo ufw allow from <agent-cidr> to any port 9090 proto tcp"
    fi
}

print_firewall_hints

# ─── Success ──────────────────────────────────────────────────────────────────
echo ""
echo "${BOLD}${GREEN}═══════════════════════════════════════════════════════════════${NO_COLOR}"
echo "${BOLD}${GREEN}  PingWAF v${VERSION} installed successfully!${NO_COLOR}"
echo "${BOLD}${GREEN}═══════════════════════════════════════════════════════════════${NO_COLOR}"
echo ""
echo "  Binary:    ${INSTALL_DIR}/${BINARY_NAME}"
if [[ "$AGENT_ONLY_BUILD" == "true" ]]; then
    echo "  Build:     agent-only (data plane; no control plane, REST API or dashboard)"
fi
if [[ "$OS" == "linux" ]]; then
    echo "  Config:    ${CONFIG_DIR}/pingwaf.toml"
    echo "  Secrets:   ${CONFIG_DIR}/pingwaf.env (0600, holds PINGWAF_SECRET_KEY)"
    echo "  Data:      ${DATA_DIR}/"
    echo "  Service:   systemctl {start|stop|restart|status} ${SERVICE_NAME}"
fi

if [[ "$MODE" == "agent" ]]; then
    echo ""
    echo "  Next steps:"
    echo "    1. Confirm the node appears under Nodes in the dashboard"
    if [[ "$OS" == "linux" ]]; then
        echo "    2. If it does not: systemctl status ${SERVICE_NAME} && journalctl -u ${SERVICE_NAME} -e"
    else
        echo "    2. If it does not, run the agent in the foreground to see the error"
    fi
    echo "    3. Point another control plane at it by editing ${CONFIG_DIR}/pingwaf.toml"
    echo ""
else
    echo ""
    echo "  Quick start:"
    echo "    ${BINARY_NAME} ${MODE} --db-url \"${DB_URL}\""
    echo ""
    echo "  Dashboard: https://<this-host>:9080  (self-signed certificate)"
    echo "  Listen:    admin 0.0.0.0:9080 — restrict it to your admin network"
    if [[ "$LISTEN_PUBLIC" == "true" ]]; then
        echo "             gRPC 0.0.0.0:9090 (remote agents enabled)"
        echo "  ${YELLOW}⚠ Both ports are open: change the administrator password and firewall 9080/9090 to the admin and agent networks.${NO_COLOR}"
    else
        echo "             gRPC 127.0.0.1:9090 — add --listen-public for remote agents"
        echo "  ${YELLOW}⚠ The console is reachable from the network: change the administrator password at the first sign-in and firewall 9080 to your admin network.${NO_COLOR}"
    fi
    if [[ "$CONFIG_PRESENT" == "true" ]]; then
        echo "  Administrator: unchanged — ${CONFIG_DIR}/pingwaf.toml was left as it was."
    else
        echo "  Administrator:"
        echo "    Email:    ${ADMIN_EMAIL}"
        if [[ "$ADMIN_PASSWORD_GENERATED" == "true" ]]; then
            echo "    Password: ${ADMIN_PASSWORD}"
            echo "              (generated — shown once; store it now and change it after sign-in)"
        else
            echo "    Password: (the value you supplied, stored in ${CONFIG_DIR}/pingwaf.toml)"
        fi
    fi
    echo ""
    if [[ "$OS" == "linux" && "$SKIP_SYSTEMD" != "true" ]]; then
        echo "  Ports 80/443 are bound by User=pingwaf through the unit's"
        echo "  CAP_NET_BIND_SERVICE capability. Verify with:"
        echo "    systemctl show ${SERVICE_NAME} -p AmbientCapabilities"
        echo ""
    fi
    if [[ "$ADMIN_PASSWORD_GENERATED" == "true" ]]; then
        echo "  ${YELLOW}⚠ Save the generated administrator password now: it is not shown again.${NO_COLOR}"
    fi
    echo "  ${YELLOW}⚠ Back up ${CONFIG_DIR}/pingwaf.toml and ${CONFIG_DIR}/pingwaf.env.${NO_COLOR}"
    echo "  ${YELLOW}   Do NOT regenerate the JWT secret or PINGWAF_SECRET_KEY: previously sealed${NO_COLOR}"
    echo "  ${YELLOW}   credentials become unreadable and sessions are invalidated. Change the${NO_COLOR}"
    echo "  ${YELLOW}   administrator password from the dashboard (Settings → Account) instead.${NO_COLOR}"
    echo ""
    echo "  Next steps:"
    echo "    1. Ensure PostgreSQL is running and accessible"
    echo "    2. Edit ${CONFIG_DIR}/pingwaf.toml, then: sudo systemctl restart ${SERVICE_NAME}"
    echo "    3. Start the service: sudo systemctl enable --now ${SERVICE_NAME}"
    echo "    4. Sign in and CHANGE the administrator password: a fresh database"
    echo "       answers every write with 403 password_change_required until it is"
    echo "       changed (console: Settings -> Account; API: PUT /api/v1/auth/password)"
    echo "    5. Open the dashboard and add your first site"
    echo ""
    echo "  Upgrade / rollback / uninstall:"
    echo "    Upgrade:   re-run this installer with --version <new>; the config and the"
    echo "               secrets in ${CONFIG_DIR} are kept. Then: sudo systemctl restart ${SERVICE_NAME}"
    echo "    Rollback:  re-run with --version <previous> (older releases stay on GitHub),"
    echo "               then: sudo systemctl restart ${SERVICE_NAME}"
    echo "    Backup:    sudo tar -czf pingwaf-config-\$(date +%F).tgz -C / etc/pingwaf"
    echo "               sudo chmod 600 pingwaf-config-*.tgz   # it holds the admin password and keys"
    echo "    Uninstall: sudo systemctl disable --now ${SERVICE_NAME}"
    echo "               sudo rm -f /etc/systemd/system/${SERVICE_NAME}.service"
    echo "               sudo rm -f ${INSTALL_DIR}/${BINARY_NAME}"
    echo "               keep (or back up) ${CONFIG_DIR} and ${DATA_DIR}; remove the account with:"
    echo "               sudo userdel pingwaf"
    echo "    Never delete ${DATA_DIR} or drop the database before taking a backup."
    echo ""
fi
