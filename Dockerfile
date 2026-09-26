# PingWAF Multi-stage Dockerfile (optimized)
# ─────────────────────────────────────────────────────────────────────────────
# Stage 1: Build the React frontend (npm cache mount)
# Stage 2: Build the Rust binary (warm-up layer for dependency precompilation)
# Stage 3: Minimal runtime image (behavior contract unchanged)

# ─── Stage 1: Frontend ───────────────────────────────────────────────────────
FROM node:22-alpine AS frontend-builder

WORKDIR /app/web

# 1a. 先拷 manifest，最大化层缓存命中（源码变化不会让此层失效）
COPY web/package.json web/package-lock.json ./

# 1b. npm ci 挂载 npm 缓存目录，消除重复下载（改进：原无 cache mount，每次全量下载）
RUN --mount=type=cache,target=/root/.npm \
    npm ci --ignore-scripts

# 1c. 拷源码并构建（源码变化只让此层失效，1b 层仍命中）
COPY web/ ./
RUN npm run build

# ─── Stage 2: Rust Builder ───────────────────────────────────────────────────
# 改进：版本从 1.98.0 升至 1.98.1，与 release.yml 的 dtolnay/rust-toolchain@1.98.1 对齐
FROM rust:1.98.1-bookworm AS builder

# SYNC: keep this list in lockstep with ci.yml / release.yml
RUN apt-get update && apt-get install -y --no-install-recommends \
    protobuf-compiler \
    libprotobuf-dev \
    cmake \
    libclang-dev \
    pkg-config \
    libssl-dev \
    nasm \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# 2b.【关键修复】预热前必须拷入：
#     - build.rs（根 Cargo.toml 声明 build = "build.rs"，缺失会导致预热 cargo build 立即失败）
#     - pingwaf-proto/proto/（pingwaf-proto/build.rs 需要 control_plane.proto 生成 gRPC stub）
#     - 根 Cargo.toml/lock + 全部 24 个成员的 Cargo.toml
COPY Cargo.toml Cargo.lock build.rs ./
COPY pingwaf-proto/proto/ pingwaf-proto/proto/

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

# 2c. 创建 dummy 源文件，让 cargo 能通过 manifest 解析并编译第三方依赖
#     benches/bench.rs 是根 Cargo.toml 的 [[bench]] target，manifest 解析期必须存在
RUN mkdir -p src && echo "fn main() {}" > src/main.rs \
    && mkdir -p benches && echo "fn main() {}" > benches/bench.rs \
    && for dir in pingap-core pingap-util pingap-config pingap-cache \
       pingap-certificate pingap-discovery pingap-health pingap-location \
       pingap-logger pingap-plugin pingap-proxy pingap-upstream pingap-acme \
       pingap-performance pingap-otel pingap-sentry pingap-pyroscope \
       pingap-imageoptim pingap-webhook pingwaf-server pingwaf-agent \
       pingwaf-waf pingwaf-challenge; do \
       mkdir -p "$dir/src" && echo "" > "$dir/src/lib.rs"; \
    done \
    && mkdir -p pingwaf-proto/src && echo "" > pingwaf-proto/src/lib.rs

# 2d. 依赖预热构建（修复：build.rs 与 proto/ 已前置 COPY，预热现在真正生效）
#     编译产物留在镜像层中，供 CI type=gha/type=registry 缓存后端正确导出和恢复
#     保留 `|| true` 因 dummy 源可能触发个别 crate 的编译错误
RUN cargo build --release --features full || true; \
    ls target/release/deps/*.rlib >/dev/null 2>&1 \
      || { echo "ERROR: dependency warm-up produced no rlib — check build log above" >&2; exit 1; }

# 2e. 拷真实源码（任何源码变化让此层及以下失效，但 2d 的缓存层仍命中）
COPY src/ src/
COPY benches/ benches/
# src/plugin/admin.rs embeds dist/ via rust-embed (resolved against the root
# crate's manifest dir /app), so the folder must exist at compile time.
COPY dist/ dist/
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

# 2f. 前端产物（供 pingwaf-server/src/frontend.rs 的 rust-embed #[folder = "../web/dist/"]）
COPY --from=frontend-builder /app/web/dist/ web/dist/

# 2g. 最终构建：touch 使 workspace crate 的 mtime 指纹失效（排除 target/ 避免污染生成代码），
#     触发 25 个 workspace crate 重编，而第三方 .rlib 从预热层继承、不受影响
#     （Docker 层叠加：target/ 存在于 2d 预热层的文件系统中，后续 COPY 源码层不会覆盖它）
RUN find . -path ./target -prune -o -name '*.rs' -print0 | xargs -0 --no-run-if-empty touch \
    && cargo build --release --bin pingwaf --features full

# ─── Stage 3: Runtime（行为契约保持不变）─────────────────────────────────────
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
