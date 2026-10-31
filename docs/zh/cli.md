# PingWAF CLI — 管理员手册

`pingwaf` 二进制不只是服务进程：它内置一组**直连 PostgreSQL**（而非走 REST API）的运维子命令。本手册覆盖运行模式、公共参数与全部运维命令，并给出控制台无能为力时的处置手册。

## 何时使用 CLI

Web 控制台是 `:9080` 控制面的 HTTP 客户端。当控制面本身出问题时，控制台就派不上用场了——而这恰恰是 CLI 仍然可用的场景：

- **控制面宕机**（崩溃循环、迁移失败、配置错误），需要检查或修复状态。
- **API IP 白名单把运维人员锁在外面**——你能访问 `:9080` 的所有来源地址都被拒绝。
- **忘记管理员密码**，无法登录（或登记的邮箱不对）。
- **需要回滚一次配置变更**，而控制台回滚无法触达。

运行 `pingwaf <command> --help` 可查看任一子命令的精确参数。直接执行 `pingwaf --help` 返回的是共享二进制的 `pingap` 兼容选项，而不是这套子命令树。

## 运行模式

三个子命令会启动常驻进程，它们正是 systemd 单元 `ExecStart` 所调用的内容：

| 模式 | 命令 | 角色 |
| --- | --- | --- |
| 控制面 | `pingwaf server` | REST API + gRPC 控制面 + 内嵌控制台。不代理流量。 |
| 数据面 | `pingwaf agent` | 连接远端控制面，执行其规则，在 :80/:443 代理流量。 |
| All-in-one | `pingwaf all-in-one` | 上述两者同进程运行（Agent 经回环连接本地 Server）。 |

也可以用 `PINGWAF_MODE` 环境变量代替子命令选择模式；`pingwaf --version`（长横幅）/ `pingwaf -V`（纯版本号）打印版本后退出。systemd 安装的
`ExecStart=/usr/local/bin/pingwaf <mode>`（模式取决于 `install.sh` 收到的参数），并在单元中设置 `PINGWAF_CONFIG=/etc/pingwaf/pingwaf.toml`；`systemctl restart pingwaf` 应用任何配置变更，`journalctl -u pingwaf -e` 查看服务日志。

## 公共参数

`server` 与 `all-in-one` 接受以下参数（`agent` 有自己的一组，见表后说明）。每个参数都同时读取对应的 `PINGWAF_*` 环境变量。

| 参数 | 环境变量 | 默认值 | 用途 |
| --- | --- | --- | --- |
| `--db-url` | `PINGWAF_DB_URL` | `postgres://pingwaf:pingwaf@localhost:5432/pingwaf` | PostgreSQL 连接串 |
| `--admin-addr` | `PINGWAF_ADMIN_ADDR` | `0.0.0.0:9080` | HTTP 管理/API 监听地址 |
| `--grpc-addr` | `PINGWAF_GRPC_ADDR` | `0.0.0.0:9090` | gRPC 监听（server/all-in-one）或连接（agent）地址 |
| `--grpc-tls-cert` | `PINGWAF_GRPC_TLS_CERT` | — | gRPC 控制面的 PEM 证书链；Agent 此后经 `https://` 连接 |
| `--grpc-tls-key` | `PINGWAF_GRPC_TLS_KEY` | — | `--grpc-tls-cert` 的 PEM 私钥 |
| `--grpc-tls-mode` | `PINGWAF_GRPC_TLS_MODE` | `tls` | `tls` 使用已配置的证书，缺省时使用自签控制面证书；`off` 提供明文（降级） |
| `--tls-enabled` | `PINGWAF_TLS_ENABLED` | `true` | 控制台与 REST API 以 TLS 提供服务；首次启动生成自签名证书。置于 TLS 终止代理之后时设 `false` |
| `--tls-sans` | `PINGWAF_TLS_SANS` | *（空）* | 生成的自签名证书的 SAN 列表（DNS 名与 IP，逗号分隔） |
| `--public-host` | `PINGWAF_PUBLIC_HOST` | *（未设置）* | 控制台对外访问的主机名（可带端口）；写死到 HTTP→HTTPS 跳转目标中，取代客户端的 `Host` 头 |
| `--config` | `PINGWAF_CONFIG` | — | TOML 配置文件；其 `[server]` 与 `[agent]` 表持有相同设置 |

仅 server 侧参数（`server` / `all-in-one`）：`--jwt-secret`（`PINGWAF_JWT_SECRET`，默认 `change-me-in-production`，须 ≥ 16 字符）、`--admin-email`（`PINGWAF_ADMIN_EMAIL`，默认 `admin@pingwaf.local`）、`--admin-password`（`PINGWAF_ADMIN_PASSWORD`，默认 `pingwaf123`）与 `--serve-frontend`（`PINGWAF_SERVE_FRONTEND`，默认 `true`）。

仅 agent 侧参数（`agent` / `all-in-one` 的 agent 半边）：`--server-url`（`PINGWAF_SERVER_URL`，默认 `http://localhost:9090`）、`--server-ca-cert`（`PINGWAF_SERVER_CA_CERT`）、`--api-key`（`PINGWAF_API_KEY`）、`--cache-dir`（`PINGWAF_CACHE_DIR`，默认 `./data/cache`）、`--fail-open`（`PINGWAF_FAIL_OPEN`，默认 `true`）、`--heartbeat-interval-secs`（`PINGWAF_HEARTBEAT_INTERVAL`，默认 `30`）、`--metrics-ship-interval`（`PINGWAF_METRICS_SHIP_INTERVAL`，默认 `30`），以及 `--log-batch-size`、`--log-flush-interval-secs`、`--max-body-log-size`（仅命令行）。

### 配置优先级

配置依次从 TOML 文件、`PINGWAF_*` 环境变量、命令行读取——后者覆盖前者。同一参数在命令行重复出现时，命令行的值覆盖配置文件注入的值，因此 `pingwaf all-in-one --config /etc/pingwaf/pingwaf.toml --tls-enabled false` 总能关闭 TLS，与文件内容无关。

## 运维子命令

所有运维子命令都接受 `--db-url`（环境变量 `PINGWAF_DB_URL`，默认 `postgres://pingwaf:pingwaf@localhost:5432/pingwaf`），直连 PostgreSQL，完成工作后退出。**正在运行**的控制面会在几秒内察觉这些写入，但 CLI 与控制台不同，它不会向 Agent 推送任何内容——见下文 `config rollback` 的注意事项。

### `pingwaf user` — 控制台账号

| 命令 | 用途 |
| --- | --- |
| `pingwaf user list` | 列出控制台账号。 |
| `pingwaf user add-admin --email <addr> [--name <name>] [--password <pw>]` | 创建管理员账号。省略 `--password` 时会生成随机密码并**只打印一次**。 |
| `pingwaf user reset-password --email <addr> [--password <pw>]` | 设置新密码。省略密码时生成并打印一次。重置会递增账号的 token 版本号，吊销该账号的全部现存会话——被泄露的会话不会在重置后继续有效。 |

### `pingwaf mode` — 观察模式

| 命令 | 用途 |
| --- | --- |
| `pingwaf mode status` | 打印当前观察模式状态。 |
| `pingwaf mode observe` | 开启观察模式：所有检测照常运行，但一律不拦截。 |
| `pingwaf mode enforce` | 关闭观察模式：防护重新生效。 |

在线上流量调优规则时用 `observe`；误报处理完后再 `enforce` 切回。`mode status` 可以确认当前处于哪一种，不必依赖记忆。

### `pingwaf security` — 控制面自身防护

| 命令 | 用途 |
| --- | --- |
| `pingwaf security status` | 打印 `:9080` 防护设置与生效的白名单。 |
| `pingwaf security allowlist on\|off [--force]` | 开关 API IP 白名单——控制台不可达时的紧急开关。 |

```bash
pingwaf security allowlist on           # 开启；白名单为空时会被拒绝
pingwaf security allowlist off          # 关闭
pingwaf security allowlist on --force   # 白名单为空也开启，见下方警告
```

> ⚠️ **`allowlist on --force` 是个陷阱。** `--force` 允许在白名单为空时开启它，而空白名单会**拒绝所有 API 调用方**——控制台、管理面板以及对 `:9080` 的所有脚本调用全部被锁死，只能回到本 CLI（或直接改数据库）才能解开。请先不带 `--force` 开启白名单，把自己的地址加进去之后再依赖它。

### `pingwaf config` — 配置版本历史

```bash
pingwaf config history [--site <uuid|global>] [--limit <n>]   # 最新在前，默认 20
pingwaf config show <version> [--json]                        # 元数据 + 快照摘要
pingwaf config rollback <version> [--yes]                     # 除非 --yes，否则询问确认
```

- `history` 按最新在前列出已记录的版本。`--site` 过滤到单个站点（其 UUID）或部署级全局设置（`global`）；`--limit` 限制打印条数（默认 20）。
- `show` 打印某个版本的元数据与快照摘要；`--json` 打印完整快照 JSON。
- `rollback` 恢复某个版本，并把这次恢复记录为一条归属 `cli` 的新版本——因此可以「回滚一次回滚」，历史保持线性。除非给出 `--yes`，否则会请求确认。

> ⚠️ **CLI 回滚不会通知正在运行的控制面。** 它直接写 PostgreSQL 的数据行；Agent 要到下一次全量同步或重启时才会取到恢复后的配置。如果变更必须立刻到达全部 Agent，请使用控制台回滚（`POST /config-versions/{id}/rollback`，见 [docs/api.md](../api.md)）——它会立即推送到 Agent。

## 典型故障处置

### 忘记管理员密码

```bash
pingwaf user reset-password --email admin@pingwaf.local
# → 生成密码并只打印一次
pingwaf user reset-password --email admin@pingwaf.local --password 'S3cret-New-Pass'
```

重置会吊销该账号的全部既有会话；用新密码登录，若密码是打印出来的，请随后在控制台修改。

### 被 API 白名单锁死

```bash
pingwaf security status          # 确认是白名单在拦截
pingwaf security allowlist off   # 关闭它，登录，修好名单后再开启
```

如果白名单是在为空时用 `--force` 开启的，`allowlist off` 是唯一解法——除非直接改数据库。

### 临时停止拦截（观察模式）

```bash
pingwaf mode status
pingwaf mode observe    # 全部检测照跑，一律不拦截
pingwaf mode enforce    # 恢复执行
```

### 回滚一次错误的配置变更

```bash
pingwaf config history --limit 10          # 找到要恢复的版本
pingwaf config show 42                     # 恢复前先检查
pingwaf config rollback 42                 # 会询问；--yes 跳过确认
sudo systemctl restart pingwaf             # 让运行中的进程立刻生效
```

注意 CLI 回滚要等 Agent 下一次全量同步或重启才会生效；重启服务（或在控制台可达时使用控制台回滚）可以让它立即下发。

### 服务无法启动或日志异常

```bash
journalctl -u pingwaf -e        # 该单元最近的日志
systemctl status pingwaf
```

### 绑定 80/443 报 "Permission denied"

单元以非特权用户 `pingwaf` 运行；只有单元持有 `CAP_NET_BIND_SERVICE` 时才能绑定特权端口。检查方法：

```bash
systemctl show pingwaf -p AmbientCapabilities
# AmbientCapabilities=CAP_NET_BIND_SERVICE
```

如果该行为空（旧版 `install.sh` 装出的单元），按安装脚本升级时打印的 `systemctl edit pingwaf` drop-in 片段添加——见[部署指南的升级章节](../deployment.md#upgrade-procedure)。

## 与 Web 控制台的差异

| 方面 | CLI | Web 控制台 |
| --- | --- | --- |
| 变更状态的方式 | 直写 PostgreSQL | 经 `:9080` 的 REST API，受白名单约束 |
| 控制面宕机时 | 仍然可用 | 不可用 |
| Agent | 不推送任何内容；Agent 稍后同步 | 立即推送（如控制台回滚） |
| 认证 | 数据库凭证（`--db-url`） | 控制台会话 / JWT |
| 适用范围 | 单实例运维 | 全集群管理 |

两者互补：集群日常管理走控制台，把本 CLI 留给控制台恰好坏掉的时刻。
