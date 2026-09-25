# PingWAF Deployment Guide

## Prerequisites

| Requirement | Minimum | Recommended |
|-------------|---------|-------------|
| OS | Linux (amd64/arm64), macOS | Ubuntu 22.04+, Debian 12+ |
| PostgreSQL | 14+ | 16 |
| RAM | 512 MB | 2 GB+ |
| Disk | 1 GB | 10 GB+ (logs, cache) |
| Ports | 80, 443, 9080, 9090 | — |

## Quick Start (Docker Compose)

The fastest way to get PingWAF running:

```bash
# Clone the repository
git clone https://github.com/shuaiZend/PingWAF.git
cd PingWAF

# Set secrets (recommended)
export JWT_SECRET=$(openssl rand -hex 32)
export ADMIN_PASSWORD=$(openssl rand -hex 16)
export POSTGRES_PASSWORD=$(openssl rand -hex 16)

# Start all services
docker compose up -d

# Verify
curl http://localhost:9080/healthz
```

Dashboard: `http://localhost:9080`  
Default credentials: `admin@pingwaf.local` / value of `$ADMIN_PASSWORD`

### Environment Variables (docker-compose)

| Variable | Default | Description |
|----------|---------|-------------|
| `POSTGRES_PASSWORD` | `pingwaf` | PostgreSQL password |
| `JWT_SECRET` | `change-me-in-production-use-32-chars` | JWT signing secret (≥16 chars) |
| `ADMIN_EMAIL` | `admin@pingwaf.local` | Initial admin email |
| `ADMIN_PASSWORD` | `pingwaf123` | Initial admin password |
| `ALLOW_REGISTRATION` | `false` | Allow new user signups |
| `HEARTBEAT_INTERVAL` | `15` | Agent heartbeat interval (seconds) |
| `RUST_LOG` | `info` | Log level filter |

## Manual Installation (Binary + systemd)

### 1. Install the Binary

```bash
# Automatic (downloads latest release)
curl -fsSL https://raw.githubusercontent.com/shuaiZend/PingWAF/main/install.sh | bash

# Or specify version and mode
./install.sh --version 0.14.3 --mode all-in-one
```

### 2. Set Up PostgreSQL

```bash
# Install PostgreSQL
sudo apt install postgresql-16

# Create user and database
sudo -u postgres createuser pingwaf
sudo -u postgres createdb -O pingwaf pingwaf
sudo -u postgres psql -c "ALTER USER pingwaf PASSWORD 'your-secure-password';"
```

### 3. Configure

Edit `/etc/pingwaf/pingwaf.toml`:

```toml
[server]
db_url = "postgres://pingwaf:your-secure-password@localhost:5432/pingwaf"
http_addr = "0.0.0.0:9080"
grpc_addr = "0.0.0.0:9090"
jwt_secret = "generate-a-random-32-char-secret"
admin_email = "admin@yourdomain.com"
admin_password = "strong-password-here"
allow_registration = false

[agent]
server_url = "http://127.0.0.1:9090"
cache_dir = "/var/lib/pingwaf/cache"
heartbeat_interval_secs = 30
fail_open = true
```

### 4. Start the Service

```bash
sudo systemctl enable --now pingwaf
sudo systemctl status pingwaf

# View logs
sudo journalctl -u pingwaf -f
```

## Configuration Reference

### Server Configuration (`[server]`)

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `db_url` | string | `postgres://pingwaf:pingwaf@localhost:5432/pingwaf` | PostgreSQL DSN |
| `http_addr` | string | `0.0.0.0:9080` | REST API + dashboard address |
| `grpc_addr` | string | `0.0.0.0:9090` | gRPC control plane address |
| `jwt_secret` | string | — | JWT signing secret (≥16 chars, required) |
| `jwt_expiration_hours` | int | `12` | Access token lifetime |
| `refresh_token_expiration_hours` | int | `720` | Refresh token lifetime |
| `admin_email` | string | `admin@pingwaf.local` | Seeded admin email |
| `admin_password` | string | `pingwaf123` | Seeded admin password |
| `allow_registration` | bool | `true` | Allow new user signups |
| `db_max_connections` | int | `20` | Connection pool max |
| `db_min_connections` | int | `1` | Connection pool min |
| `heartbeat_interval_seconds` | int | `15` | Agent heartbeat interval |
| `log_batch_size` | int | `500` | Log entries per batch insert |
| `cors_origins` | string[] | `[]` | Allowed CORS origins (empty = all) |

### Agent Configuration (`[agent]`)

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `server_url` | string | `http://127.0.0.1:9090` | Control plane gRPC URL |
| `api_key` | string | `""` | Agent authentication key |
| `cache_dir` | string | `./data/cache` | Local rule cache directory |
| `heartbeat_interval_secs` | int | `30` | Heartbeat frequency |
| `log_batch_size` | int | `100` | Log entries before flush |
| `log_flush_interval_secs` | int | `5` | Max time between flushes |
| `max_body_log_size` | int | `8192` | Max request body bytes to log |
| `fail_open` | bool | `true` | Allow traffic when disconnected |
| `reconnect_initial_delay_ms` | int | `1000` | Initial reconnect backoff |
| `reconnect_max_delay_ms` | int | `60000` | Maximum reconnect backoff |

### Environment Variables

All config values can be set via environment variables with the `PINGWAF_` prefix:

| Variable | Config Key |
|----------|-----------|
| `PINGWAF_DB_URL` | `server.db_url` |
| `PINGWAF_HTTP_ADDR` | `server.http_addr` |
| `PINGWAF_ADMIN_ADDR` | `server.http_addr` (alias) |
| `PINGWAF_GRPC_ADDR` | `server.grpc_addr` |
| `PINGWAF_JWT_SECRET` | `server.jwt_secret` |
| `PINGWAF_ADMIN_EMAIL` | `server.admin_email` |
| `PINGWAF_ADMIN_PASSWORD` | `server.admin_password` |
| `PINGWAF_ALLOW_REGISTRATION` | `server.allow_registration` |
| `PINGWAF_HEARTBEAT_INTERVAL` | `server.heartbeat_interval_seconds` |
| `PINGWAF_DB_MAX_CONNECTIONS` | `server.db_max_connections` |
| `PINGWAF_CORS_ORIGINS` | `server.cors_origins` (comma-separated) |
| `PINGWAF_MODE` | CLI mode (`all-in-one`, `server`, `agent`) |
| `PINGWAF_SERVER_URL` | `agent.server_url` |
| `PINGWAF_API_KEY` | `agent.api_key` |
| `PINGWAF_CACHE_DIR` | `agent.cache_dir` |
| `PINGWAF_ES_ENABLED` | Elasticsearch shipper toggle |
| `PINGWAF_ES_URLS` | Elasticsearch URLs (comma-separated) |
| `PINGWAF_ES_INDEX_PREFIX` | ES index prefix |
| `PINGWAF_ES_USERNAME` | ES basic auth username |
| `PINGWAF_ES_PASSWORD` | ES basic auth password |
| `PINGWAF_ES_API_KEY` | ES API key |
| `RUST_LOG` | Tracing log level filter |

## Deployment Modes

### All-in-One Mode

Runs both control plane and data plane in a single process. Best for single-server setups.

```bash
pingwaf all-in-one --db-url "postgres://..." --admin-addr 0.0.0.0:9080
```

### Distributed Mode

Separate control plane server and multiple edge agents.

**Control Plane (server):**
```bash
pingwaf server --db-url "postgres://..." --admin-addr 0.0.0.0:9080 --grpc-addr 0.0.0.0:9090
```

**Edge Agent:**
```bash
pingwaf agent --server-url "http://control-plane:9090" --api-key "your-agent-key"
```

Generate an agent API key from the dashboard: **Settings → API Keys → Create Key**.

## SSL/TLS Configuration

### Automatic (ACME / Let's Encrypt)

Configure through the dashboard:
1. Add a site with your domain
2. Go to **SSL** tab
3. Select "Let's Encrypt" and choose challenge type:
   - **HTTP-01**: Requires port 80 accessible
   - **DNS-01**: Supports Cloudflare, AliDNS, Huawei DNS, Tencent DNS

Certificates are automatically renewed before expiry.

### Manual Certificates

Upload PEM-encoded certificate and key through the dashboard or API:

```bash
curl -X POST http://localhost:9080/api/v1/sites/{site_id}/certificates \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "my-cert",
    "certificate": "-----BEGIN CERTIFICATE-----\n...",
    "private_key": "-----BEGIN PRIVATE KEY-----\n...",
    "domains": ["example.com", "*.example.com"]
  }'
```

## Firewall & Port Requirements

| Port | Protocol | Purpose | Required |
|------|----------|---------|----------|
| 80 | TCP | HTTP traffic / ACME HTTP-01 | Yes (for proxied sites) |
| 443 | TCP | HTTPS traffic | Yes (for proxied sites) |
| 9080 | TCP | Admin dashboard + REST API | Yes |
| 9090 | TCP | gRPC control plane (agents) | Distributed mode |
| 5432 | TCP | PostgreSQL | Internal only |

```bash
# UFW example
sudo ufw allow 80/tcp
sudo ufw allow 443/tcp
sudo ufw allow 9080/tcp
sudo ufw allow 9090/tcp  # Only if agents connect remotely
```

## Backup & Recovery

### Database Backup

```bash
# Full backup
pg_dump -U pingwaf -d pingwaf -F c -f pingwaf_backup_$(date +%Y%m%d).dump

# Restore
pg_restore -U pingwaf -d pingwaf --clean pingwaf_backup_20240101.dump
```

### Configuration Backup

```bash
tar -czf pingwaf_config_backup.tar.gz /etc/pingwaf/ /var/lib/pingwaf/certs/
```

### Automated Backups (cron)

```cron
# Daily at 2:00 AM
0 2 * * * pg_dump -U pingwaf -d pingwaf -F c -f /backups/pingwaf_$(date +\%Y\%m\%d).dump
# Remove backups older than 30 days
0 3 * * * find /backups -name "pingwaf_*.dump" -mtime +30 -delete
```

## Upgrade Procedure

### Docker

```bash
docker compose pull
docker compose up -d
```

### Binary

```bash
# Download and install new version
./install.sh --version NEW_VERSION --no-systemd

# Restart service
sudo systemctl restart pingwaf
```

Database migrations run automatically on startup.

### Rolling Upgrade (Distributed)

1. Upgrade the control plane server first
2. Verify agents reconnect successfully
3. Upgrade agents one at a time

## Troubleshooting

### Service won't start

```bash
# Check logs
sudo journalctl -u pingwaf --no-pager -n 50

# Common issues:
# - PostgreSQL not running: systemctl status postgresql
# - Port already in use: ss -tlnp | grep -E '9080|9090'
# - Invalid config: pingwaf all-in-one --db-url "..." (run interactively)
```

### Database connection failed

```bash
# Test connection
psql "postgres://pingwaf:password@localhost:5432/pingwaf" -c "SELECT 1"

# Check pg_hba.conf allows connections
# Ensure the user has CREATEDB privilege for migrations
```

### Agent not connecting

1. Verify the control plane gRPC port (9090) is accessible
2. Check the API key is valid
3. Review agent logs: `RUST_LOG=debug pingwaf agent --server-url ...`
4. Ensure clocks are synchronized (NTP)

### High memory usage

- Reduce `db_max_connections`
- Lower `log_batch_size`
- Check if cache disk quota is configured

### Health check endpoint

```bash
curl http://localhost:9080/healthz
# {"status":"ok","database":"up"}
```
