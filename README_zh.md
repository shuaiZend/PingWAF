<div align="center">

# 🛡️ PingWAF

**基于 [`pingap`](https://github.com/vicanso/pingap) 与 Cloudflare [`Pingora`](https://github.com/cloudflare/pingora) 构建的分布式、集中控制的高性能 Web 应用防火墙（WAF）。**

语义级攻击检测 · Cloudflare 风格规则 · CC 与 Bot 防护 · TLS 自动签发 · 内嵌多语言控制台

[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.96%2B-orange.svg)](https://www.rust-lang.org/)
[![CI](https://github.com/shuaiZend/PingWAF/actions/workflows/ci.yml/badge.svg)](https://github.com/shuaiZend/PingWAF/actions/workflows/ci.yml)
[![Docker](https://img.shields.io/badge/docker-compose%20ready-2496ED?logo=docker&logoColor=white)](./docker-compose.yml)

**[English](./README.md) | [简体中文](./README_zh.md) | [Español](./README_es.md) | [Français](./README_fr.md)**

[快速开始](#-快速开始) · [架构概览](#-架构概览) · [核心特性](#-核心特性) · [文档导航](#-文档导航) · [参与贡献](./CONTRIBUTING.md)

</div>

---

## 📖 PingWAF 是什么？

**PingWAF** 是一款高性能的开源 **Web 应用防火墙（WAF）**，把 Cloudflare 级别的边缘安全能力带到你自己掌控的基础设施中。它构建于 [`pingap`](https://github.com/vicanso/pingap)——一款由 Cloudflare [`Pingora`](https://github.com/cloudflare/pingora) 网络框架驱动的生产级反向代理——之上，并在其上叠加了**分布式、集中控制**的安全层。

单一的**控制面（Control Plane）**负责定义站点、规则与策略；一个或多个**数据面（Data Plane）Agent** 在边缘执行它们。规则、日志与指标通过持久化的 **gRPC 双向流**在两者之间流转，因此在控制台修改策略后，几秒内即可下发到所有 Agent——无需重载、无需停机。

- 🎯 **语义级检测**——通过 `libinjection` + Aho-Corasick 签名匹配、异常评分以及 Cloudflare 风格表达式规则，覆盖 SQLi / XSS / RCE / 路径穿越 / 命令注入。
- 🕸️ **为分布式而生**——既可单进程运行全部能力（`all-in-one`），也可将数据面横向扩展为多个独立的边缘 Agent（`server` + `agent`）。
- 🧭 **开箱即用，默认关闭**——每一项防护能力默认关闭，按站点单独开启，让你对流量拥有完全的掌控。
- ⚡ **纯 Rust 打造**——内存安全、异步 I/O，单个自包含二进制文件，控制台已内嵌其中。

> PingWAF 是一个独立项目，与 Cloudflare 及 `pingap` 维护者均无关联，也未获其背书。详见[致谢](#-致谢)。

---

## 🏗️ 架构概览

<img src="./asset/readme-architecture.webp" alt="PingWAF 架构：客户端经 HTTP/HTTPS 接入边缘 Agent；每个 Agent 通过 gRPC 双向流与控制面同步规则与遥测；只有干净流量继续到源站" width="1000">

控制面与数据面通过 `ControlPlane` gRPC 服务通信（定义于 [`control_plane.proto`](./pingwaf-proto/proto/control_plane.proto)），共包含 7 个 RPC：

| RPC | 类型 | 用途 |
| --- | --- | --- |
| `RegisterAgent` | 一元调用 | Agent 加入集群，获取 ID 与心跳间隔 |
| `Heartbeat` | 双向流 | 上行存活/统计/每站点证书状态，下行实时命令 |
| `SyncRules` | 服务端流 | 策略变更时将规则包推送到 Agent |
| `ShipLogs` | 客户端流 | 批量请求/攻击日志流式上报控制面 |
| `ShipCertEvents` | 客户端流 | Agent 捕获的 ACME 签发事件流式上报控制面 |
| `ShipMetrics` | 客户端流 | 批量流量指标流式上报控制面 |
| `GetSiteConfig` | 一元调用 | Agent 拉取单个站点的完整配置 |

**同一个二进制，两种部署形态：**

- **All-in-One（一体化）**——控制面 + 数据面运行于单进程。适合单节点、小型部署与本地评估。
- **Distributed（分布式）**——一个独立的 `server`（控制面）+ 多个边缘 `agent` 进程。适合集群、多地域部署与集中管理。

---

## ✨ 核心特性

### 🛡️ 安全检测
- **语义级 WAF 引擎**，覆盖 SQL 注入、XSS、RCE、路径穿越与命令注入——由 `libinjection` 启发式算法与 Aho-Corasick 多模式签名匹配驱动。
- **四阶段流水线**：`normalize`（请求归一化）→ `signatures`（签名 / libinjection）→ `expression`（规则表达式）→ `anomaly score`（加权异常评分）。
- **Cloudflare 风格表达式规则**——可编写形如 `http.request.uri.path contains "/admin" and ip.src in {1.2.3.0/24}` 的条件。
- **异常评分**，四种裁决结果：`Pass`（放行）、`Monitor`（观察）、`Block`（拦截）、`Challenge`（挑战）。
- **托管规则集**——可按站点开关的精选签名。

### 🤖 CC 与 Bot 防护
- **CC 防护 / 5 秒盾**——JavaScript 挑战、PoW（工作量证明）与交互式挑战，配合 HMAC 签名 clearance cookie。
- **Bot 防护**——验证 Bot 白名单直接放行、已知浏览器 UA 放行，其余流量按配置动作处置；TLS 指纹与行为分析在路线图中。

### 🚦 访问控制
- **IP 访问规则**——`block` / `allow` / `challenge` / `js_challenge`，支持 CIDR 网段、CSV 批量导入与命名规则。
- **全局 IP 分组**——可订阅的 CIDR 列表（纯文本、JSON 或 Cloudflare IP 段 API），按周期自动同步到所有站点，并记录同步错误。
- **Geo 地域限制**——按国家与 ASN（自治系统号）放行或拦截，支持"拦截未知国家"策略。
- **多维限流**——按 IP、主机、路径、ASN、国家以及请求头/Cookie/查询参数等维度计数限流。

### 🌊 流量管理
- **源站池与路由**——将上游节点组成源站池，支持负载均衡（轮询、最小连接、随机或按 IP/URL/路径/头部/Cookie/查询参数一致性哈希），并按前缀、精确匹配或正则路由请求；路由可限定到某个 IP 分组，从而让同一路径对办公网或合作方地址走不同的源站池。
- **边缘缓存**——按站点磁盘配额与 LRU 驱逐，支持 stale-while-revalidate 与浏览器 TTL 控制。
- **请求 / 响应改写**——头部、路径、查询参数、状态码与响应体查找替换。
- **自定义错误页**，基于 Tera 模板渲染；对 WAF 拦截、站点暂停等边缘生成的响应同样生效。
- **暂停站点**——在控制台一键暂停后，所有请求返回 `503` 维护页，证书照常握手、流量照常入日志。

### 🔐 TLS 与证书
- **ACME / Let's Encrypt 自动签发与续期**，直接在边缘完成（HTTP-01 与 DNS-01）。
- **多 DNS 提供商**支持 DNS-01（阿里云、Cloudflare、腾讯云 DNSPod、华为云、手动）。
- **每站点证书状态**（`valid` / `即将过期` / `expired`）通过心跳上报控制面，并提供可检索的 ACME 事件日志。
- **HSTS 与强制 HTTPS** 在边缘生效，`max-age` 默认半年（15552000 秒）。
- **双向 TLS（mTLS）**——控制面内生成或导入 CA、签发客户端证书，吊销即时在边缘生效；证书被吊销或签发给其他站点时一律拒绝。
- **HTTPS 回源证书校验是显式开关**——「系统设置 → 回源」提供全局默认值，单个回源池可覆盖。默认接受任意回源证书，自签名与内网 CA 环境升级后不受影响；源站持有受信任证书时应显式开启校验。

### 📊 可观测性
- **全量请求日志写入 Elasticsearch**，支持响应体截断与 WAL 缓冲以保证可靠性。
- **请求与响应原文采集**——PostgreSQL 中保存头部、请求/响应体前缀、真实大小与截断标记，可在控制台一键复制为 `curl` 命令复现请求。头部按原样保存（含 Cookie），以便安全复现场景。
- **Analytics 分析**面板，呈现流量与攻击趋势。
- Agent 健康、流量统计与缓存命中/驱逐指标通过心跳上报控制面。

### 🎛️ 管理控制台
- **JWT + bcrypt 认证**与**多租户**隔离。
- **Passkey 通行密钥登录**（WebAuthn）——在设置页绑定平台认证器或安全密钥后即可免密码登录。
- **内嵌多语言控制台**（English / 简体中文 / 日本語），通过 `rust-embed` 编译进二进制。
- **所有功能默认关闭**——按站点显式开启防护。
- **配置版本管理**——控制台可筛选列出全部配置快照，将任一版本与其前一版本逐表对比，并在二次确认后回滚。

---

## 📊 实测效果

<img src="./asset/readme-effectiveness.webp" alt="blazehttp 语料上的拦截率对比：PingWAF 全量口径 93.8%（有效口径 96.6%，误报 0.06%）对比 ModSecurity PL4 94.61%、SafeLine 76.17%、ModSecurity PL1 69.74%、Cloudflare 10.70%" width="1000">

基于公开的 [blazehttp](https://github.com/chaitin/blazehttp) 样本库（33 877 条原始 HTTP 报文：恶意 658 / 正常 33 219），对 release 构建逐字节回放实测。拦截数按 WAF 判决面统计——403 拦截页与 503 质询页；"正常流量受影响"同样计入两者，质询不会被藏进"拦截"数字里。

在反向代理侧 WAF 真正能够判决的请求范围内，**Strict + 请求体检测的拦截率为 96.6%（616/638）**，正常流量受影响 **0.06%（19/33 219）**，准确率 **99.82%**。658 条恶意样本中有 20 条从未到达任何检测面，未计入该口径：19 条是不带 `Content-Length` / `Transfer-Encoding` 的 `POST`/`PUT`（按 RFC 7230 请求体为空），1 条把载荷藏在 URL fragment 里，HTTP 解析器在构造请求行前就已剥离。任何反代侧 WAF 对这 20 条都只能放行——把它们算作漏报会低估检出能力。

| 档位 | 攻击拦截率（全量口径） | 正常流量受影响 | p95 |
| --- | --- | --- | --- |
| Normal（默认，不检测请求体） | 60.2% | 0.01% | 1 ms |
| Normal + 请求体检测 | 71.9% | 0.04% | 1 ms |
| Strict | 76.0% | 0.01% | 1 ms |
| Strict + 请求体检测 | **93.8%** | **0.06%** | 2 ms |

全量口径（93.8%，617/658）保留用于与其他测量直接对照；有效口径见上。

### 跨厂商对比

| 产品 / 模式 | 恶意样本 | 拦截率 | 正常样本 | 误报率 | 准确率 |
|---|---|---|---|---|---|
| **PingWAF 0.32.4 · Strict + body** | 658 | **93.8%**（617） | 33 219 | **0.06%**（19） | **99.82%** |
| **PingWAF 0.32.4 · Normal** | 658 | **60.2%**（396） | 33 219 | **0.01%**（4） | **99.21%** |
| SafeLine free · strict | 575 | 76.17%（438） | 33 094 | 0.22%（73） | 99.38% |
| SafeLine free · balanced | 575 | 71.65%（412） | 33 094 | 0.07%（23） | 99.45% |
| ModSecurity PL4 | 575 | 94.61%（544） | 33 094 | 52.46%（17 362） | 48.34% |
| ModSecurity PL1 | 575 | 69.74%（401） | 33 094 | 17.58%（5 819） | 82.20% |
| Cloudflare free | 570 | 10.70%（61） | 32 780 | 0.07%（23） | 98.40% |

PingWAF 两行为本项目自行回放（有效口径 96.6%，见上），其余各行引自 blazehttp 项目 README，未在本地重跑。两边样本集接近但不相同（33 877 vs 33 669；恶意样本 658 vs 570–575），且第三方延迟含公网往返，而 PingWAF 的 p95 是环回判决延迟——只比检出与误报，不比毫秒。

每个数字都可复现：套件定义、回放工具与原始指标在 [`tools/waf-bench/`](./tools/waf-bench)。方法论见**[测试方案](./docs/zh/waf-test-plan.md)**，结论、分类表、与 Cloudflare / 雷池 / ModSecurity 的横向对比（引自 blazehttp 评测）与已知限制见**[测试效果报告](./docs/zh/waf-test-report.md)**。

## 🧱 技术栈

| 层次 | 技术 |
| --- | --- |
| 数据面 / 代理 | Rust · [`Pingora`](https://github.com/cloudflare/pingora) · [`pingap`](https://github.com/vicanso/pingap) |
| 控制面 API | [`Axum`](https://github.com/tokio-rs/axum)（REST）· [`tonic`](https://github.com/hyperium/tonic)（gRPC） |
| 持久化 | [`SeaORM`](https://www.sea-ql.org/SeaORM/) · PostgreSQL 14+（推荐 16） |
| 日志存储 | Elasticsearch（可选） |
| 控制台 | React 19 · Vite · Tailwind CSS v4（通过 `rust-embed` 内嵌） |
| WAF 引擎 | `libinjection` · Aho-Corasick · 表达式求值器 |

**核心 crate：**

| Crate | 职责 |
| --- | --- |
| [`pingwaf-proto`](./pingwaf-proto) | 控制面 gRPC 协议定义（单一来源：`control_plane.proto`） |
| [`pingwaf-server`](./pingwaf-server) | 控制面：Axum REST + tonic gRPC + SeaORM/PostgreSQL + ES 日志 + 内嵌前端 + Agent 健康监控 |
| [`pingwaf-agent`](./pingwaf-agent) | 数据面 Agent：连接控制面、规则缓存 + 磁盘持久化、回传日志/指标、接收命令 |
| [`pingwaf-waf`](./pingwaf-waf) | 检测引擎：归一化 → 签名 → 表达式 → 异常评分 |
| [`pingwaf-challenge`](./pingwaf-challenge) | 动态挑战子系统：JS 5 秒盾、交互式挑战、PoW、浏览器指纹、HMAC 签名 clearance cookie |
| `pingwaf-profiler` / `pingwaf-system`（无 README） | 进程内 CPU/内存归因与快照，经 `/api/v1/system/profile` 与 `/api/v1/system/metrics` 提供 |

---

## 🚀 快速开始

两条典型路径：**Docker Compose**（一条命令，`all-in-one` 模式 + PostgreSQL；预编译二进制仅提供 Linux amd64/arm64——见 [Releases 页面](https://github.com/shuaiZend/PingWAF/releases)），或**源码构建**（Rust 1.96+、Node.js 26.12.1+、`protoc`）。

```bash
# 路径 A —— Docker Compose（推荐；无需克隆仓库、无需 root）
curl -fsSL https://raw.githubusercontent.com/shuaiZend/PingWAF/main/deploy/compose/install.sh | bash
# 或克隆本仓库后执行：
docker compose up -d

# 路径 B —— 源码构建
git clone https://github.com/shuaiZend/PingWAF.git && cd PingWAF
cd web && npm ci && npm run build && cd ..
cargo build --release --bin pingwaf --features full
./target/release/pingwaf all-in-one --db-url "postgres://pingwaf:pingwaf@localhost:5432/pingwaf"
```

随后打开控制台 **https://localhost:9080**（HTTPS，首次启动自动生成自签名证书），使用 `admin@pingwaf.local` / `pingwaf123` 登录——**生产环境务必修改默认密码与 `PINGWAF_JWT_SECRET`**。

👉 完整安装向导、交互式安装器、TLS/端口参数说明见**[官网安装页](https://www.pingwaf.com/zh/install/)**；分步上手见**[官网快速上手页](https://www.pingwaf.com/zh/quickstart/)**；仓库内完整流程（数据库准备、首个站点、分布式 Agent、systemd）见 **[docs/quick-start.md](./docs/quick-start.md)**。

---

## 🧭 运行模式

PingWAF 是单一二进制（`pingwaf`），与 `pingap` 共享入口。它通过 CLI 子命令**或** `PINGWAF_MODE` 环境变量选择运行模式。

| 模式 | 命令 | 角色 |
| --- | --- | --- |
| **控制面** | `pingwaf server` | REST API + gRPC 服务 + 控制台 + PostgreSQL。不代理流量。 |
| **数据面** | `pingwaf agent` | 连接远端控制面，执行规则，在 :80/:443 代理流量。 |
| **All-in-One** | `pingwaf all-in-one` | 单进程同时运行上述两者（Agent 通过本地回环连接 Server）。 |

```bash
# 等价于 `pingwaf all-in-one`
PINGWAF_MODE=all-in-one ./pingwaf
```

---

## ⚙️ 配置说明

PingWAF 通过 **TOML 配置文件**、**`PINGWAF_*` 环境变量**与 **CLI 参数**进行配置，优先级依次升高（后者覆盖前者）。

```bash
pingwaf all-in-one --config /etc/pingwaf/pingwaf.toml

PINGWAF_CONFIG=/etc/pingwaf/pingwaf.toml pingwaf all-in-one   # 等价写法
```

> ℹ️ 仓库根目录的 [`pingwaf.toml`](./pingwaf.toml) 是可用的示例配置——用
> `--config` 指定后，其中 `[server]` / `[agent]` 两张表即会生效（启动时会打印
> `pingwaf: loaded N setting(s)`）。键名即 CLI 参数名（下划线形式，如
> `admin_addr`、`max_body_log_size`）；无法识别的键会被提示并忽略。

### 关键环境变量

| 变量 | 默认值 | 说明 |
| --- | --- | --- |
| `PINGWAF_CONFIG` | — | TOML 配置文件路径 |
| `PINGWAF_MODE` | — | `server`、`agent` 或 `all-in-one` |
| `PINGWAF_DB_URL` | `postgres://pingwaf:pingwaf@localhost:5432/pingwaf` | PostgreSQL DSN |
| `PINGWAF_ADMIN_ADDR` | `0.0.0.0:9080` | REST API + 控制台监听地址 |
| `PINGWAF_GRPC_ADDR` | `0.0.0.0:9090` | gRPC 控制面监听地址 |
| `PINGWAF_JWT_SECRET` | `change-me-in-production` | JWT 签名密钥（**≥ 16 字符**，生产必改） |
| `PINGWAF_ADMIN_EMAIL` | `admin@pingwaf.local` | 初始管理员邮箱 |
| `PINGWAF_ADMIN_PASSWORD` | `pingwaf123` | 初始管理员密码（**生产必改**） |
| `PINGWAF_ALLOW_REGISTRATION` | `true` | `POST /api/v1/auth/register` 是否接受注册（`docker-compose.yml` 已将其设为 `false`） |
| `PINGWAF_HEARTBEAT_INTERVAL` | `15` | 控制面在注册时下发给 Agent 的心跳间隔（秒）。Agent 侧的 `--heartbeat-interval-secs` 读取同一变量但回退值为 `30`，且仅在控制面未下发间隔时生效 |
| `PINGWAF_TLS_ENABLED` | `true` | REST API 与控制台是否以 HTTPS 提供服务（替换前为自签名证书）；置于 TLS 终止代理之后时设为 `false` |
| `PINGWAF_TLS_SANS` | 主机名、`localhost`、`127.0.0.1` | 生成的自签名证书所覆盖的 SAN 列表（逗号分隔） |
| `PINGWAF_SERVER_URL` | `http://localhost:9090` | *（agent）* 控制面 gRPC 地址 |
| `PINGWAF_API_KEY` | *（空）* | *（agent）* API Key；为空则通过本地回环自动注册 |
| `PINGWAF_CACHE_DIR` | `./data/cache` | *（agent）* 本地规则缓存目录 |
| `PINGWAF_ES_ENABLED` | `false` | 是否启用 Elasticsearch 日志投递 |
| `PINGWAF_ES_URLS` | *（空）* | 逗号分隔的 Elasticsearch 地址 |

### 默认端口

| 端口 | 用途 |
| --- | --- |
| `9080` | REST API + 内嵌控制台（HTTPS；健康检查 `GET /healthz`，HTTP 亦可访问） |
| `9090` | gRPC 控制面（Agent 连接此端口） |
| `80` / `443` | 代理流量（创建首个站点后绑定；`443` 在签发或上传证书后才会开启——此前仅 `80` 端口提供 HTTP 服务） |

👉 完整配置参考：**[docs/deployment.md](./docs/deployment.md)** 与 **[docs/api.md](./docs/api.md)**。

---

## 📚 文档导航

| 文档 | 内容 |
| --- | --- |
| [docs/quick-start.md](./docs/quick-start.md) | 从零开始，保护你的第一个站点 |
| [docs/deployment.md](./docs/deployment.md) | Docker、二进制 + systemd、分布式拓扑 |
| [docs/cli.md](./docs/cli.md) | 管理员二进制 CLI——运行模式、运维子命令、故障处置手册（[中文版](./docs/zh/cli.md)） |
| [docs/user-guide.md](./docs/user-guide.md) | 控制台使用、站点、规则与策略 |
| [docs/zh/http-lifecycle.md](./docs/zh/http-lifecycle.md) | 请求生命周期：各项功能在流水线中的执行阶段（WAF 检查顺序、缓存、错误页）与调试方法 |
| [docs/zh/waf-test-plan.md](./docs/zh/waf-test-plan.md) | WAF 测试方案——分层模型、blazehttp 用例选取、口径与验收门槛 |
| [docs/zh/waf-test-report.md](./docs/zh/waf-test-report.md) | WAF 实测效果报告（0.32.4）——拦截率、误报、归因与限制 |
| [docs/waf-benchmark-report.md](./docs/waf-benchmark-report.md) | 逐轮基准数据附录（P0 → P14 迭代史） |
| [docs/api.md](./docs/api.md) | REST API 参考（`https://<host>:9080/api/v1`） |
| [docs/README.md](./docs/README.md) | 完整文档索引 |
| [CONTRIBUTING.md](./CONTRIBUTING.md) | 如何参与贡献 |
| [SECURITY.md](./SECURITY.md) | 漏洞披露政策 |
| [CODE_OF_CONDUCT.md](./CODE_OF_CONDUCT.md) | 社区行为准则 |

---

## 🤝 参与贡献

欢迎贡献代码！在提交 Pull Request 前，请先阅读 **[CONTRIBUTING.md](./CONTRIBUTING.md)**；如需负责任地报告安全漏洞，请见 **[SECURITY.md](./SECURITY.md)**。

---

## 🙏 致谢

PingWAF 建立在优秀的开源项目之上：

- **[pingap](https://github.com/vicanso/pingap)**（作者 Tree Xie）——PingWAF 数据面所依托的反向代理基础（路由、插件、ACME、缓存、热重载）。
- **[Pingora](https://github.com/cloudflare/pingora)**（Cloudflare）——驱动 `pingap` 的异步网络框架。
- **[libinjection](https://github.com/client9/libinjection)**——WAF 引擎所使用的 SQLi/XSS 检测启发式算法。

PingWAF 是在上述项目之上的衍生作品，新增了 WAF 控制面、数据面 Agent 与检测引擎。它与上游依赖一样，采用相同的 **[Apache License 2.0](./LICENSE)** 分发，并保留了 `pingap`/`Pingora` 的原始版权声明。PingWAF 与 Cloudflare 及 `pingap` 项目**均无关联，也未获其背书**。

---

## 📄 License

PingWAF 基于 **[Apache License 2.0](./LICENSE)** 发布。
