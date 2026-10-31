#!/bin/bash
# ─────────────────────────────────────────────────────────────────────────────
# PingWAF Installation Script
# ─────────────────────────────────────────────────────────────────────────────
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/shuaiZend/PingWAF/main/install.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/shuaiZend/PingWAF/main/install.sh | bash -s -- --mode agent --server-url https://... --api-key pwk_...
#   ./install.sh                     # interactive prompts (needs a terminal)
#   ./install.sh [--version X.Y.Z] [--mode all-in-one|server|agent]
#
# Options:
#   --version         Install a specific version (default: latest)
#   --mode            Operating mode: all-in-one, server, agent (default: all-in-one)
#   --server-url      Control plane gRPC URL (agent mode)
#   --api-key         Agent enrollment token (agent mode)
#   --admin-email     Seed administrator e-mail (server / all-in-one)
#   --admin-password  Seed administrator password (server / all-in-one)
#   --tls-sans        Comma separated names of the dashboard certificate
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

# ─── Constants ────────────────────────────────────────────────────────────────
REPO="shuaiZend/PingWAF"
BINARY_NAME="pingwaf"
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
API_KEY=""
ADMIN_EMAIL=""
ADMIN_PASSWORD=""
TLS_SANS=""
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
  --mode MODE              Operating mode: all-in-one, server, agent
  --server-url URL         Control plane gRPC URL (agent mode)
  --api-key KEY            Agent enrollment token (agent mode)
  --db-url URL             PostgreSQL connection string
  --admin-email EMAIL      Seed administrator e-mail (server / all-in-one)
  --admin-password PASS    Seed administrator password (server / all-in-one)
  --tls-sans NAMES         Comma separated dashboard certificate names
  --no-systemd             Skip systemd service file creation
  --interactive            Ask the questions even when options were given
  --non-interactive, --yes Never prompt; use the given values and defaults
  --dry-run                Print the resolved plan, then exit without changes
  --help                   Show this help message

Environment Variables:
  PINGWAF_VERSION       Same as --version
  PINGWAF_MODE          Same as --mode
  PINGWAF_SERVER_URL    Same as --server-url
  PINGWAF_API_KEY       Same as --api-key
  PINGWAF_DB_URL        Same as --db-url
  PINGWAF_ADMIN_EMAIL   Same as --admin-email
  PINGWAF_ADMIN_PASSWORD Same as --admin-password
  PINGWAF_TLS_SANS      Same as --tls-sans

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
  Config: /etc/pingwaf/pingwaf.toml   (edit it, then: systemctl restart pingwaf)
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
        --api-key)        require_value "$1" "${2:-}"; API_KEY="$2"; PARAMS_GIVEN=true; shift 2 ;;
        --admin-email)    require_value "$1" "${2:-}"; ADMIN_EMAIL="$2"; PARAMS_GIVEN=true; shift 2 ;;
        --admin-password) require_value "$1" "${2:-}"; ADMIN_PASSWORD="$2"; PARAMS_GIVEN=true; shift 2 ;;
        --tls-sans)       require_value "$1" "${2:-}"; TLS_SANS="$2"; PARAMS_GIVEN=true; shift 2 ;;
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
API_KEY="${API_KEY:-${PINGWAF_API_KEY:-}}"
ADMIN_EMAIL="${ADMIN_EMAIL:-${PINGWAF_ADMIN_EMAIL:-}}"
ADMIN_PASSWORD="${ADMIN_PASSWORD:-${PINGWAF_ADMIN_PASSWORD:-}}"
TLS_SANS="${TLS_SANS:-${PINGWAF_TLS_SANS:-}}"

# `--version latest` is the documented spelling for "no pin"; the interactive
# prompt already normalises it, the flag path has to do the same or the download
# URL becomes .../download/vlatest/...
if [[ "$VERSION" == "latest" ]]; then VERSION=""; fi

if [[ -n "$VERSION$MODE$DB_URL$SERVER_URL$API_KEY$ADMIN_EMAIL$ADMIN_PASSWORD$TLS_SANS" ]]; then
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
        all-in-one|server|agent) ;;
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

systemd_env() { # $1 variable name, $2 value -> Environment="NAME=value"
    local value="${2//\\/\\\\}"
    value="${value//\"/\\\"}"
    # Unit files expand % specifiers, so a literal percent must be doubled.
    value="${value//%/%%}"
    printf 'Environment="%s=%s"' "$1" "$value"
}

validate_install_values() {
    reject_control_chars "mode" "$MODE"
    reject_control_chars "--version" "$VERSION"
    reject_control_chars "--db-url" "$DB_URL"
    reject_control_chars "--server-url" "$SERVER_URL"
    reject_control_chars "--api-key" "$API_KEY"
    reject_control_chars "--admin-email" "$ADMIN_EMAIL"
    reject_control_chars "--admin-password" "$ADMIN_PASSWORD"
    reject_control_chars "--tls-sans" "$TLS_SANS"
}

prompt_admin_password() {
    local first second
    for _ in 1 2 3; do
        prompt_line "Administrator password (empty keeps the default, changed at first sign-in)" "" true
        first="$REPLY"
        if [[ -z "$first" ]]; then
            ADMIN_PASSWORD="pingwaf123"
            warn "Keeping the documented default password; the console forces a change at first sign-in."
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

print_plan_summary() {
    local systemd_state="no"
    local password_state="default (pingwaf123)"
    local api_key_state="unset"

    if [[ "$SKIP_SYSTEMD" != "true" && "${OS:-}" == "linux" ]]; then
        systemd_state="yes (User=pingwaf, CAP_NET_BIND_SERVICE for 80/443)"
    fi
    if [[ -n "$ADMIN_PASSWORD" && "$ADMIN_PASSWORD" != "pingwaf123" ]]; then
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
    echo "  config:      ${CONFIG_DIR}/pingwaf.toml"
    echo "  data:        ${DATA_DIR}"
    echo "  systemd:     ${systemd_state}"
    if [[ "${MODE:-all-in-one}" == "agent" ]]; then
        echo "  control:     ${SERVER_URL}"
        echo "  api key:     ${api_key_state}"
    else
        echo "  database:    $(mask_db_url "${DB_URL:-postgres://pingwaf:pingwaf@localhost:5432/pingwaf}")"
        echo "  admin:       ${ADMIN_EMAIL:-admin@pingwaf.local} (password: ${password_state})"
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
    echo ""
    prompt_until "Mode [1-3 or name]" "all-in-one" false validate_mode_input
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
ADMIN_PASSWORD="${ADMIN_PASSWORD:-pingwaf123}"
TLS_SANS="${TLS_SANS// /}"

# Validate mode
case "$MODE" in
    all-in-one|server|agent) ;;
    *) fatal "Invalid mode: $MODE (must be: all-in-one, server, agent)" ;;
esac

validate_install_values

# Agents talk to the control plane, never to PostgreSQL.
if [[ "$MODE" == "agent" ]]; then
    SERVER_URL="${SERVER_URL:-http://127.0.0.1:9090}"
    if [[ -z "$API_KEY" ]]; then
        warn "No --api-key given: start the agent with PINGWAF_API_KEY set, or it cannot register."
    fi
    if [[ "$SERVER_URL" == http:* ]]; then
        warn "The control plane serves gRPC over TLS by default; a plain http:// server-url only works when the control plane sets grpc_tls_mode = \"off\". For TLS use https:// and configure agent.server_ca_cert with the control plane's certificate."
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
fetch_asset() {
    local ext url
    for ext in tar.xz tar.gz; do
        if [[ "$ext" == "tar.xz" ]] && ! has xz; then
            info "xz(1) not found — falling back to the .tar.gz asset."
            continue
        fi
        ASSET_NAME="pingwaf-${OS}-${ARCH}.${ext}"
        url="https://github.com/${REPO}/releases/download/v${VERSION}/${ASSET_NAME}"
        info "Downloading ${ASSET_NAME}..."
        info "URL: ${url}"
        if has curl; then
            if curl -fsSL "$url" -o "${TMPDIR}/${ASSET_NAME}"; then
                return 0
            fi
        elif has wget; then
            if wget -q "$url" -O "${TMPDIR}/${ASSET_NAME}"; then
                return 0
            fi
        else
            fatal "Neither curl nor wget found. Please install one of them."
        fi
        warn "Download failed for ${ASSET_NAME} — trying the next format."
    done
    fatal "Download failed. Check that version v${VERSION} exists and has a pingwaf-${OS}-${ARCH} asset."
}

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

ASSET_NAME=""
fetch_asset

# ─── Extract & Install Binary ─────────────────────────────────────────────────
info "Extracting..."
case "$ASSET_NAME" in
    *.tar.xz) tar -xJf "${TMPDIR}/${ASSET_NAME}" -C "$TMPDIR" ;;
    *.tar.gz) tar -xzf "${TMPDIR}/${ASSET_NAME}" -C "$TMPDIR" ;;
    *)        fatal "Unexpected asset name: ${ASSET_NAME}" ;;
esac

# Find the binary
BINARY_PATH="$(find "$TMPDIR" -name "$BINARY_NAME" -type f | head -n1)"
if [[ -z "$BINARY_PATH" ]]; then
    # Fallback: first executable file
    BINARY_PATH="$(find "$TMPDIR" -type f -perm +111 | head -n1)"
fi
if [[ -z "$BINARY_PATH" ]]; then
    fatal "Binary not found in archive."
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

info "Installing binary to ${INSTALL_DIR}/${BINARY_NAME}..."
if [[ -w "$INSTALL_DIR" ]]; then
    mv "$BINARY_PATH" "${INSTALL_DIR}/${BINARY_NAME}"
elif has sudo; then
    sudo mv "$BINARY_PATH" "${INSTALL_DIR}/${BINARY_NAME}"
else
    fatal "No write permission to ${INSTALL_DIR} and sudo not available."
fi

success "Binary installed: ${INSTALL_DIR}/${BINARY_NAME}"

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
        $cmd chown -R pingwaf:pingwaf "$CONFIG_DIR" "$DATA_DIR" 2>/dev/null || true
    fi
}

if [[ "$OS" == "linux" ]]; then
    info "Creating directories..."
    create_dirs
    success "Created ${CONFIG_DIR} and ${DATA_DIR}"
fi

# ─── Write Default Configuration ─────────────────────────────────────────────
write_config() {
    local config_file="${CONFIG_DIR}/pingwaf.toml"
    if [[ -f "$config_file" ]]; then
        warn "Config file already exists at ${config_file}, leaving it untouched."
        warn "Values from this run (admin account, TLS names, database) were not applied;"
        warn "edit ${config_file} and run: sudo systemctl restart ${SERVICE_NAME}"
        return
    fi

    local cmd=""
    [[ -w "$CONFIG_DIR" ]] || cmd="sudo"

    # Keys are the command-line flags with dashes replaced by underscores, and
    # the file only fills in what the environment and the command line leave
    # alone (see docs/deployment.md). Settings without a flag — registration,
    # CORS, Elasticsearch, passkeys — stay in the environment.
    local agent_server_url="# agent mode only; all-in-one talks to its own loopback gRPC"
    if [[ "$MODE" == "agent" ]]; then
        agent_server_url="server_url = \"$(toml_string "$SERVER_URL")\""
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
# that). Precedence: this file, then PINGWAF_* environment variables, then
# command-line flags. Unknown keys are reported at startup and ignored.

[server]
db_url = "$(toml_string "$DB_URL")"
admin_addr = "0.0.0.0:9080"
grpc_addr = "0.0.0.0:9090"
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
    # JWT secret readable by every local user (0644, owned by root). It must be
    # pingwaf-owned as well, because the unit runs as User=pingwaf.
    if ! $cmd chown pingwaf:pingwaf "$config_file" 2>/dev/null; then
        warn "Could not hand ${config_file} to pingwaf:pingwaf — does the user exist?"
    fi
    if ! $cmd chmod 640 "$config_file" 2>/dev/null; then
        warn "Could not chmod 640 ${config_file}; it may stay readable by other local users."
    fi

    success "Configuration written to ${config_file}"
}

if [[ "$OS" == "linux" ]]; then
    write_config
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

    if [[ -r "$unit_file" ]] && grep -q '^AmbientCapabilities=.*CAP_NET_BIND_SERVICE' "$unit_file"; then
        granted=true
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
        agent_env="${agent_env}
$(systemd_env PINGWAF_SERVER_URL "$SERVER_URL")
$(systemd_env PINGWAF_API_KEY "$API_KEY")
$(systemd_env PINGWAF_CACHE_DIR "${DATA_DIR}/cache")"
    else
        # The dashboard is served over HTTPS with a self-signed certificate
        # until a real one is uploaded. Add the hostname it is reached under so
        # the generated certificate covers it:
        # Environment=PINGWAF_TLS_SANS=waf.example.com
        agent_env="${agent_env}
$(systemd_env PINGWAF_TLS_ENABLED "true")"
    fi

    info "Creating systemd service..."

    local cmd=""
    [[ -w "/etc/systemd/system" ]] || cmd="sudo"

    $cmd tee "$service_file" >/dev/null <<EOF
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
# Log level; the default keeps sqlx's per-statement log out of the way:
Environment=RUST_LOG=info,sqlx=warn

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

[Install]
WantedBy=multi-user.target
EOF

    $cmd systemctl daemon-reload
    success "Systemd service installed: ${SERVICE_NAME}.service"

    # An agent only contributes once it is running, so start it here — this is
    # what makes the dashboard's one-line enrolment command enough on its own.
    # Server modes stay manual: they need PostgreSQL ready and an attended
    # first boot.
    if [[ "$MODE" == "agent" ]]; then
        info "Starting the agent service..."
        if $cmd systemctl enable --now "$SERVICE_NAME"; then
            success "Service enabled and started: ${SERVICE_NAME}"
        else
            warn "The service did not start — inspect: journalctl -u ${SERVICE_NAME} -e"
            warn "After fixing, start it with: sudo systemctl enable --now ${SERVICE_NAME}"
        fi
        return
    fi

    info "To start PingWAF:"
    echo "  sudo systemctl enable --now ${SERVICE_NAME}"
    echo "  sudo systemctl status ${SERVICE_NAME}"
}

install_systemd

# ─── PostgreSQL Check ─────────────────────────────────────────────────────────
check_postgres() {
    info "Checking PostgreSQL availability..."

    if has psql; then
        if psql "$DB_URL" -c "SELECT 1" >/dev/null 2>&1; then
            success "PostgreSQL connection successful."
        else
            warn "psql found but cannot connect to: ${DB_URL}"
            echo ""
            echo "  To set up PostgreSQL:"
            echo "    1. Install: sudo apt install postgresql-16"
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
            if has apt-get; then
                echo "    sudo apt install postgresql-16"
            elif has dnf; then
                echo "    sudo dnf install postgresql-server"
            elif has yum; then
                echo "    sudo yum install postgresql-server"
            else
                echo "    (install PostgreSQL from your distribution's package manager)"
            fi
        else
            echo "    brew install postgresql@16"
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
else
    check_postgres
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

    info "Firewall ports to open:"
    echo "    80/tcp   — HTTP traffic (proxied sites)"
    echo "    443/tcp  — HTTPS traffic (proxied sites)"
    echo "    9080/tcp — Admin dashboard & REST API (HTTPS)"
    echo "    9090/tcp — gRPC control plane (agent connections)"
    echo ""
    if has ufw; then
        echo "  With UFW:"
        echo "    sudo ufw allow 80/tcp"
        echo "    sudo ufw allow 443/tcp"
        echo "    sudo ufw allow 9080/tcp"
        echo "    sudo ufw allow 9090/tcp"
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
if [[ "$OS" == "linux" ]]; then
    echo "  Config:    ${CONFIG_DIR}/pingwaf.toml"
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
    echo "  Dashboard: https://localhost:9080  (self-signed certificate)"
    if [[ "$CONFIG_PRESENT" == "true" ]]; then
        echo "  Administrator: unchanged — ${CONFIG_DIR}/pingwaf.toml was left as it was."
    else
        echo "  Administrator:"
        echo "    Email:    ${ADMIN_EMAIL}"
        if [[ "$ADMIN_PASSWORD" == "pingwaf123" ]]; then
            echo "    Password: pingwaf123 (the default — the console forces a change at first sign-in)"
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
    echo "  ${YELLOW}⚠ Change the default admin password and JWT secret in production!${NO_COLOR}"
    echo ""
    echo "  Next steps:"
    echo "    1. Ensure PostgreSQL is running and accessible"
    echo "    2. Edit ${CONFIG_DIR}/pingwaf.toml, then: sudo systemctl restart ${SERVICE_NAME}"
    echo "    3. Start the service: sudo systemctl enable --now ${SERVICE_NAME}"
    echo "    4. Open the dashboard and add your first site"
    echo ""
fi
