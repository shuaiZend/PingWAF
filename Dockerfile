# PingWAF Multi-stage Dockerfile
# ─────────────────────────────────────────────────────────────────────────────
# Stage 1: Build the React frontend
# Stage 2: Build the Rust binary (with protoc for gRPC code generation)
# Stage 3: Minimal runtime image

# ─── Stage 1: Frontend ───────────────────────────────────────────────────────
FROM node:22-alpine AS frontend-builder

WORKDIR /app/web
COPY web/package*.json ./
RUN npm ci --ignore-scripts
COPY web/ ./
RUN npm run build

# ─── Stage 2: Rust Builder ───────────────────────────────────────────────────
FROM rust:1.98.0-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    protobuf-compiler \
    cmake \
    libclang-dev \
    pkg-config \
    libssl-dev \
    nasm \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy manifests first for dependency caching
COPY Cargo.toml Cargo.lock ./

# Copy all workspace member Cargo.toml files for dependency resolution
COPY pingap-core/Cargo.toml pingap-core/
COPY pingap-util/Cargo.toml pingap-util/
COPY pingap-config/Cargo.toml pingap-config/
COPY pingap-cache/Cargo.toml pingap-cache/
COPY pingap-certificate/Cargo.toml pingap-certificate/
COPY pingap-discovery/Cargo.toml pingap-discovery/
COPY pingap-health/Cargo.toml pingap-health/
COPY pingap-location/Cargo.toml pingap-location/
COPY pingap-logger/Cargo.toml pingap-logger/
COPY pingap-plugin/Cargo.toml pingap-plugin/
COPY pingap-proxy/Cargo.toml pingap-proxy/
COPY pingap-upstream/Cargo.toml pingap-upstream/
COPY pingap-acme/Cargo.toml pingap-acme/
COPY pingap-performance/Cargo.toml pingap-performance/
COPY pingap-otel/Cargo.toml pingap-otel/
COPY pingap-sentry/Cargo.toml pingap-sentry/
COPY pingap-pyroscope/Cargo.toml pingap-pyroscope/
COPY pingap-imageoptim/Cargo.toml pingap-imageoptim/
COPY pingap-webhook/Cargo.toml pingap-webhook/
COPY pingwaf-proto/Cargo.toml pingwaf-proto/
COPY pingwaf-server/Cargo.toml pingwaf-server/
COPY pingwaf-agent/Cargo.toml pingwaf-agent/
COPY pingwaf-waf/Cargo.toml pingwaf-waf/
COPY pingwaf-challenge/Cargo.toml pingwaf-challenge/

# Create dummy source files so cargo can resolve dependencies
RUN mkdir -p src && echo "fn main() {}" > src/main.rs \
    && for dir in pingap-core pingap-util pingap-config pingap-cache \
       pingap-certificate pingap-discovery pingap-health pingap-location \
       pingap-logger pingap-plugin pingap-proxy pingap-upstream pingap-acme \
       pingap-performance pingap-otel pingap-sentry pingap-pyroscope \
       pingap-imageoptim pingap-webhook pingwaf-server pingwaf-agent \
       pingwaf-waf pingwaf-challenge; do \
       mkdir -p "$dir/src" && echo "" > "$dir/src/lib.rs"; \
    done \
    && mkdir -p pingwaf-proto/src && echo "" > pingwaf-proto/src/lib.rs

# Pre-build dependencies (this layer is cached unless Cargo.toml/lock changes)
RUN cargo build --release --features full 2>/dev/null || true

# Copy actual source code
COPY build.rs ./
COPY src/ src/
COPY pingap-core/ pingap-core/
COPY pingap-util/ pingap-util/
COPY pingap-config/ pingap-config/
COPY pingap-cache/ pingap-cache/
COPY pingap-certificate/ pingap-certificate/
COPY pingap-discovery/ pingap-discovery/
COPY pingap-health/ pingap-health/
COPY pingap-location/ pingap-location/
COPY pingap-logger/ pingap-logger/
COPY pingap-plugin/ pingap-plugin/
COPY pingap-proxy/ pingap-proxy/
COPY pingap-upstream/ pingap-upstream/
COPY pingap-acme/ pingap-acme/
COPY pingap-performance/ pingap-performance/
COPY pingap-otel/ pingap-otel/
COPY pingap-sentry/ pingap-sentry/
COPY pingap-pyroscope/ pingap-pyroscope/
COPY pingap-imageoptim/ pingap-imageoptim/
COPY pingap-webhook/ pingap-webhook/
COPY pingwaf-proto/ pingwaf-proto/
COPY pingwaf-server/ pingwaf-server/
COPY pingwaf-agent/ pingwaf-agent/
COPY pingwaf-waf/ pingwaf-waf/
COPY pingwaf-challenge/ pingwaf-challenge/

# Copy frontend dist for rust-embed (pingwaf-server expects web/dist/)
COPY --from=frontend-builder /app/web/dist/ web/dist/

# Force rebuild of all crates (touch to invalidate the dummy cache)
RUN find . -name "*.rs" -exec touch {} + \
    && cargo build --release --bin pingwaf --features full

# ─── Stage 3: Runtime ────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -r -m -s /usr/sbin/nologin pingwaf \
    && mkdir -p /etc/pingwaf /var/lib/pingwaf \
    && chown -R pingwaf:pingwaf /var/lib/pingwaf /etc/pingwaf

COPY --from=builder /app/target/release/pingwaf /usr/local/bin/pingwaf
COPY pingwaf.toml /etc/pingwaf/pingwaf.toml

EXPOSE 80 443 9080 9090

VOLUME ["/var/lib/pingwaf"]

USER pingwaf

HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD curl -sf http://localhost:9080/healthz || exit 1

ENTRYPOINT ["pingwaf"]
CMD ["all-in-one", "--db-url", "postgres://pingwaf:pingwaf@localhost:5432/pingwaf"]
