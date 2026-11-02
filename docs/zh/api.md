# PingWAF REST API 参考（关键章节中文镜像）

> **本文是英文 [`docs/api.md`](../api.md) 的同步镜像，只覆盖关键章节，不是全文翻译。**
> 两处不一致时，**以英文版为准**。章节标题保留英文原文，便于对照；小节内的
> 「详见」链接指向英文原文对应锚点。
>
> 注意：文中「证书链接幂等」「`cpu_source`」「`site_hosts` 告警」以及响应替换的
> `br`/`zstd` 支持属于 **Unreleased**（见 [`CHANGELOG.md`](../../CHANGELOG.md)）
> 行为；英文版现已同步这些变更，本镜像与之保持一致。
>
> 所有端点的基址：`https://<host>:9080/api/v1`。

## Certificates（证书）

### Certificate `relation`

每个证书响应都带 `relation` 字段，说明该行数据从何而来：

| 取值 | 含义 |
|-------|---------|
| `owner` | 该站点拥有证书行（`site_certificates.site_id` 是本站点）。 |
| `linked` | 证书由另一站点拥有，通过 link 行共享给本站点。 |
| `available` | 仅由 [GET /sites/{site_id}/certificates/available](../api.md#get-sitessite_idcertificatesavailable) 返回：证书属于调用方可读的另一站点，且尚未与本站点建立链接。 |

`site_certificates.site_id` 始终是**属主**——它决定配置版本的作用域、备份/恢复
分层与 ACME 记账；link 只是允许另一站点使用该证书的授权。
详见：[Certificate `relation`](../api.md#certificate-relation)。

### Certificate sharing

一张证书可以服务多个站点：一张泛域名或多 SAN 证书，多个边缘节点。属主记录在
`site_certificates.site_id`，可以为 **null**——不带站点上传的证书在挂载之前处于
[未绑定](#unbound-certificates)状态；`site_certificate_links` 表记录哪些站点可以
使用它。详见：[Certificate sharing](../api.md#certificate-sharing)。

### POST /sites/{site_id}/certificate-links

把其他站点的证书共享给本站点。要求对本站点有**写权限**，且对属主站点有**读权限**
（共享等于授予使用他人证书的权利）。

**请求：**
```json
{ "certificate_id": "uuid" }
```

**响应（201）：** 证书对象，`relation: "linked"`，带完整装饰
（`bound_sites`/`bound_count`/`warnings`，口径见下文
[Unbound certificates](#unbound-certificates)），并**立即推送该站点的 agent
配置**——共享的 PEM 需要进入该站点的数据面配置。

**幂等语义（Unreleased，旧版本重复绑定返回 409）：** 同一证书已链接到本站点时
再调用，返回与
[`POST /certificates/{id}/bindings`](../api.md#post-certificatescert_idbindings)
相同的 **200**，响应体是同一份带装饰的证书对象。目标状态已达成即视为成功，
重试不再报冲突。

**错误：**

| 状态 | 场景 |
|--------|------|
| `404` | 证书不存在（`certificate {id} not found`），或调用方无法读取属主站点。 |
| `409` | 证书已由本站点**拥有**（`certificate already belongs to this site`）——属主是真实的冲突，站点对自己拥有的证书不需要也无法再建立共享授权。 |

详见：[POST /sites/{site_id}/certificate-links](../api.md#post-sitessite_idcertificate-links)。

### DELETE /sites/{site_id}/certificate-links/{cert_id}

移除一条证书链接。要求对该站点有写权限。

**响应：** `204`。

**错误：**

| 状态 | 场景 |
|--------|------|
| `409` | 证书是该站点当前的 TLS 证书（`certificate is the site's current TLS certificate; clear or replace it first`）——直接解除链接会静默丢掉 TLS，需先在 [PUT /sites/{site_id}/ssl-settings](../api.md#put-sitessite_idssl-settings) 中清除（`clear_certificate: true`）或更换选择。 |
| `404` | 该站点没有这条链接（`certificate link {cert_id} not found`）。 |

详见：[DELETE /sites/{site_id}/certificate-links/{cert_id}](../api.md#delete-sitessite_idcertificate-linkscert_id)。

### Certificate `warnings`

每个返回证书的响应（创建/更新响应，以及列表/详情读取）都可能携带 `warnings`
数组：

```json
"warnings": ["certificate domain api.other.com is not served by this site"]
```

条目列出证书的**属主站点**不服务的主机名（来自 `domain`，逗号分隔列表）：即既不
匹配站点 `domain` 也不匹配 `alternate_domains` 的主机名（通配符双向都只覆盖一层
标签，不区分大小写）。

**仅提示，不阻断任何操作。** 命名了站点不服务的主机名的证书仍然会保存，仍然返回
`200`/`201`：先上传证书再补域名、或站点经由其他边缘服务该域名，都是合法工作流。
空数组表示「已检查，无异常」。
详见：[Certificate `warnings`](../api.md#certificate-warnings)。

### Unbound certificates

每个证书响应都带 `site_id`（可空）、`bound_sites` 与 `bound_count`：

```json
{
  "id": "uuid",
  "site_id": null,
  "bound_sites": [{"id": "uuid", "domain": "a.example.com"}],
  "bound_count": 1,
  "domain": "example.com"
}
```

**`bound_count` 口径：** `bound_sites` 列出通过**共享绑定**（`site_certificate_links`
行）可以使用该证书的每个站点（id + domain，控制台无需二次请求即可渲染）；
`bound_count` 就是该列表的长度。两者**只**由链接行推导——**属主站点不计入**，
属主单独由 `site_id` 表达。站点 A 拥有、共享给 B 和 C 的证书报告
`site_id: A`、`bound_sites: [B, C]`、`bound_count: 2`。

不带站点上传的证书是**未绑定**状态：`site_id` 为 `null`、`bound_sites` 为空、
`bound_count` 为 `0`——字段始终存在，因此「已检查、无绑定」与「字段缺失」可区分；
其 `status` 跟随自身有效期，因为没有边缘节点服务它，也就没有边缘上报来修正状态。

未绑定行的限制：

- **不能用 ACME。** 签发与续期在边缘运行，由承载证书的站点配置驱动。不带站点的
  `POST /certificates` 对 ACME 请求返回 `400`；
  `POST /certificates/{cert_id}/renew` 在绑定之前返回 `400`。
- **不能用 `activate: true`。** 开启 HTTPS 是站点级设置，创建调用返回 `400`。
  先上传、绑定，再按站点开启。
- **删除是直接的 `204`**——没有站点在用它，见
  [DELETE /certificates/{cert_id}](#delete-certificatescert_id)。

详见：[Unbound certificates](../api.md#unbound-certificates)。

### POST /certificates

**不带站点**上传证书（仅管理员）。请求体与手动上传相同——`domain` 与 PEM 对必填，
省略 `site_id`：

```json
{
  "domain": "example.com",
  "cert_pem": "-----BEGIN CERTIFICATE-----\n...",
  "key_pem": "-----BEGIN PRIVATE KEY-----\n..."
}
```

**响应（201）：** 保存后的未绑定证书——`site_id: null`、空 `warnings`
数组（覆盖率是对站点主机名度量的，尚未绑定）。之后用
[`POST /certificates/{cert_id}/bindings`](../api.md#post-certificatescert_idbindings)
挂载站点。未绑定行也可以更新（错误的域名或轮换的 PEM 可以在投入使用前修正）；
仅跳过 ACME 泛域名保护，因为没有站点主机名集合可比较。
详见：[POST /certificates](../api.md#post-certificates)。

### POST /certificates/{cert_id}/bindings

授予一个站点使用该证书的权利（`{"site_id": "uuid"}`）——这正是把 PEM 放入该站点
数据面配置的动作。该授权与站点侧
[certificate link](../api.md#post-sitessite_idcertificate-links) 创建的是同一行
`site_certificate_links`；提供证书优先的形式，是为了让「先上传、后绑定」的工作流
可用，也让多 SAN 证书能在一处绑定多个站点。

**响应（200）：** 重新计算绑定后的证书。幂等：重复绑定同一站点返回相同的 `200`，
不报冲突。

**错误：**

| 状态 | 场景 |
|--------|------|
| `400` | 证书不覆盖站点的主机名：`certificate {id} (issued for {domain}) does not cover site hostname(s) …` 列出证书无法校验的**站点**主机名——绑定一张覆盖不了的证书会让站点陷入无从察觉的 TLS 错误。证书覆盖了站点之外的主机名*不是*错误（与上传路径相同的提示性 warning），多 SAN 证书因此可以绑定到它覆盖的每个站点。 |
| `404` | 证书或站点不存在、调用方对该站点无写权限，或——对他人拥有的证书——对属主站点无读权限。 |
| `409` | 该站点已拥有该证书（`certificate already belongs to this site`）。 |

详见：[POST /certificates/{cert_id}/bindings](../api.md#post-certificatescert_idbindings)。

### DELETE /certificates/{cert_id}/bindings/{site_id}

移除一个站点的授权；证书本身保留。该站点回退到自身 SSL 设置提供的内容——当它
只依赖这条链接时就是「没有」——变更会立即推送到它的 agent。

**响应：** `204`。

**错误：**

| 状态 | 场景 |
|--------|------|
| `409` | 证书是该站点当前的 TLS 选择（`certificate is the site's current TLS certificate; clear or replace it first`）——先清除或更换选择。 |
| `404` | 该站点没有这条绑定（`certificate {id} is not bound to site {site_id}`）。 |

详见：[DELETE /certificates/{cert_id}/bindings/{site_id}](../api.md#delete-certificatescert_idbindingssite_id)。

### DELETE /certificates/{cert_id}

跨站点删除证书。两个阻塞条件，按此顺序，均为 `409`：

1. 仍有站点在 SSL 设置中**选择**它
   （`certificate is selected by site(s) …; clear or replace it there first`）；
2. 仍有**绑定**把它授予站点
   （`certificate is bound to site(s) …; unbind it there first`）——该站点的
   数据面配置携带这份 PEM，删除会让它的 TLS 下线。

未绑定证书（无属主、无绑定）直接以 `204` 删除。
详见：[DELETE /certificates/{cert_id}](../api.md#delete-certificatescert_id)。

### GET /certificates/{cert_id}/usage

一个证书由谁拥有、共享与使用——控制台的绑定、解绑、删除对话框在展示操作之前
会先取这个答案，让 `409` 得到解释（或被预防），而不是点击之后才出现。仅管理员。

**响应（200）：**
```json
{
  "owner": { "id": "uuid", "domain": "a.example.com" },
  "linked_sites": [
    { "id": "uuid", "domain": "b.example.com", "selected": true }
  ],
  "serving_sites": [ { "id": "uuid", "domain": "b.example.com" } ]
}
```

| 字段 | 含义 |
|-------|---------|
| `owner` | 拥有证书行的站点（`site_certificates.site_id`），证书[未绑定](#unbound-certificates)时为 `null`。把证书绑定到该站点会得到 `409 certificate already belongs to this site`。 |
| `linked_sites` | 与证书**共享**的每个站点——即 [DELETE /certificates/{cert_id}/bindings/{site_id}](../api.md#delete-certificatescert_idbindingssite_id) 移除的 `site_certificate_links` 授权行。当站点的当前 TLS 选择（`site_ssl.certificate_id`）是该证书时 `selected` 为 `true`——这正是解绑该站点返回 `409` 的条件。 |
| `serving_sites` | SSL 设置当前选择了该证书的站点（`site_ssl.certificate_id`）——第一个[删除](../api.md#delete-certificatescert_id)阻塞条件。由 `site_ssl` 推导，**不是**链接表：站点可以在没有任何授权行的情况下选择自己的证书，因此站点可能出现在这里而不在 `linked_sites` 里。 |

两个站点列表均按域名排序（id 兜底），相同请求产出相同输出。无论涉及多少站点，
答案固定由四次查询组装。未知证书 id 返回 `404`。

**建议操作顺序**（控制台引导的同一序列）：先在每个 serving 站点的 SSL 页清除或
更换选择（[PUT /sites/{site_id}/ssl-settings](../api.md#put-sitessite_idssl-settings)
带 `clear_certificate: true` 或另一 `certificate_id`），解绑剩余授权，再删除证书
——先切换/清除，再解绑，最后删除。
[GET /certificates/{cert_id}/usage](../api.md#get-certificatescert_idusage) 会在
开始前告诉你每个站点处于哪个桶里。
详见：[GET /certificates/{cert_id}/usage](../api.md#get-certificatescert_idusage)。

## Site Upstream Pools（上游池）

### GET /sites/{site_id}/upstream-pools

列出站点源站池，默认池在前。每个池：

```json
{
  "id": "uuid",
  "site_id": "uuid",
  "name": "default",
  "lb_algorithm": "round_robin",
  "sni": null,
  "verify_cert": null,
  "address_family": "auto",
  "is_default": true,
  "created_at": "2024-01-01T00:00:00Z",
  "upstreams": []
}
```

`address_family` —— agent 解析该池源站域名时允许的地址族：

| 取值 | 含义 |
|---|---|
| `auto`（默认） | 优先 IPv4，仅当主机没有 A 记录时回退 IPv6；IP 字面量源站忽略此设置 |
| `ipv4` | 固定 IPv4 |
| `ipv6` | 固定 IPv6 |

它是生成 agent 配置中旧布尔开关 `ipv4_only` 的三值替代（旧开关在配置中仍被接受
并由构建器保持同步；两者同时存在时 `address_family` 胜出）。池 API 本身只接受
`address_family`。创建与更新共用同一套三值校验——空值或未知值返回 `400`，而不是
当作「未设置」。

`upstreams` 是内联的成员数组：每个池的源站节点直接嵌入池对象，控制台渲染池成员
无需对每个池发起第二次请求。它也是删除前排查需要排空成员的方式——池内仍有源站
节点时，删除被 `409` 拒绝。

`sni: "$host"` 的补充：SNI 跟随每个请求的客户端 `Host`（去端口、小写；路由的
[upstream_host 覆盖](../api.md#upstream-host-override-upstream_host)优先生效）。
无法作为 TLS 名称的 `Host`（IP 字面量、缺失或格式非法）解析为**空 SNI**；不属于
站点域名集合（`domain` + `alternate_domains`）的 `Host` 仍会使用，但数据面会记录
warn 日志（见 [CHANGELOG Unreleased](../../CHANGELOG.md) 的 `site_hosts` 条目）。
**透明上游的行为变更：** 透明池（`discovery = "transparent"`——无静态节点，直接
转发到请求自身的目标）现在与静态池走同一 SNI 解析路径；此前它会把原始 `Host`
原样作为 SNI 发送（IP 字面量照发、大小写不变）。变更后 IP 字面量 `Host` 解析为
**空 SNI**，域名发送前会转小写：若客户端**以 IP 访问**经 TLS 透明池的源站，而源站
按 SNI 选择证书且没有默认证书，原先能完成的握手现在会失败——此类源站应改用域名访问。
详见：[GET /sites/{site_id}/upstream-pools](../api.md#get-sitessite_idupstream-pools)。

## System（系统观测）

### GET /system/nodes

一次请求的机队视图：控制面进程自身的计数器，加上每个已注册 agent 一行。是唯一
一次调用回答「整个机队负载如何」的端点。

**响应（200）：**

```json
{
  "control": {
    "process_cpu_percent": 12.5,
    "host_cpu_percent": 8.1,
    "cores": 4,
    "rss_bytes": 120258048,
    "rss_peak_bytes": 154140467,
    "requests": 980,
    "avg_elapsed_ms": 0.4084
  },
  "agents": [
    {
      "id": "uuid",
      "name": "edge-01",
      "status": "online",
      "cpu_percent": 23.4,
      "cpu_source": "probe",
      "memory_bytes": 1073741824,
      "connections": 128,
      "rps": null,
      "sites": 1,
      "last_seen": "2024-01-01T12:00:00Z"
    }
  ],
  "boots": [
    {
      "booted_at": "2026-10-22T12:31:05Z",
      "reason": "unclean",
      "version": "0.29.3",
      "hostname": "waf-1",
      "previous_uptime_seconds": 12
    }
  ]
}
```

（数值仅为示例。）

**`control`** —— 服务本请求的进程：

| 字段 | 含义 |
|-------|---------|
| `process_cpu_percent` | 本进程占用的 CPU，单核百分比（可超过 100） |
| `host_cpu_percent` | 整机 CPU 使用率，百分比 |
| `cores` | 机器核心数 |
| `rss_bytes` / `rss_peak_bytes` | 进程当前 / 启动以来峰值 RSS，字节（OS 不暴露峰值时为 `0`） |
| `requests` | 性能分析归因到 `control` 模块的请求数（自启动累计） |
| `avg_elapsed_ms` | 归因到 control 的请求平均耗时，毫秒 |

`control` 的 CPU 数据与 `GET /system/metrics` 同源（探针采样，约 5 s 间隔），
**不是** agent 心跳计数器——心跳携带的是自启动以来的平均值，口径不同。

**`agents[]`** —— 每个已注册 agent 一行，按 `name` 排序（轮询时表格不重排）：

| 字段 | 含义 |
|-------|---------|
| `id` | Agent id |
| `name` | 显示名——清单按节点保留一个身份，即其 `hostname` |
| `status` | `online` / `offline` / `degraded`，最近一次上报 |
| `cpu_percent` | 节点 CPU 负载，百分比；`null` = 尚无读数 |
| `cpu_source` | `cpu_percent` 的来源（Unreleased 新增）：`probe` = agent 主机探针的 5 秒区间差分样本，跨 agent 版本可比；`heartbeat` = 无探针样本时回退到心跳值（注意：0.28.3 之前的心跳值是自启动以来的平均，口径不同）；`none` = 两个来源都没有读数 |
| `memory_bytes` | 节点内存使用，字节；`null` = 尚无样本 |
| `connections` | 边缘进程当前处理的连接数；`null` = 从未上报（如指标上报被禁用） |
| `rps` | 边缘每秒处理请求数；**当前所有节点均为 `null`**——心跳携带计数器但控制面不保留每 agent 副本，字段暂无数据源，保留在契约中因为它是最常被问到的数字 |
| `sites` | 该 agent 服务的站点数：绑定 agent 为 `1`；全局（未绑定）agent 为其 API key 属主的全部站点——与配置推送相同的可见性 |
| `last_seen` | 最近一次心跳，服务器时间；`null` = 从未收到 |

控制面尚不知道的字段以 `null` 而非 `0` 表达：从未心跳过的节点报「0 %」会被误读
为空闲节点而不是未知节点。

**`boots[]`** —— 最近的控制面启动记录，新的在前（至多五条）。两行之间的间隔
就是上一个进程存活的时间。

| 字段 | 含义 |
|-------|---------|
| `booted_at` | 进程启动时间，服务器时间 |
| `reason` | `fresh`（无在案的前次启动）、`after_stop`（前一进程存活至少 60 s，符合计划内停止）、`unclean`（存活更短——前进程几乎肯定是被杀死而未正常关闭） |
| `version` | 该次启动时控制面的版本 |
| `hostname` | 启动所在机器 |
| `previous_uptime_seconds` | 前一进程存活秒数；首次启动为 `null`，两次启动间时钟回拨时亦为 `null` |

**读取须知（行为未变的既知口径）：**

- 心跳的 `cpu_usage_percent` 是**自启动以来的平均**；此处的 `cpu_percent` 优先取
  agent 最新的探针样本（约 5 s 区间测量），两次轮询之间可能有剧烈波动——新增的
  `cpu_source` 字段明确标出来源，不再需要猜。
- `agents.memory_bytes` 有一次语义漂移：**首次心跳之前**携带主机*总*内存（注册
  清单值），之后是探针样本中的*使用*内存。尚未修复——对首次轮询值据此解读。

```bash
curl -k -H "Authorization: Bearer $TOKEN" \
  "https://localhost:9080/api/v1/system/nodes"
```

详见：[GET /system/nodes](../api.md#get-systemnodes)。

## Sites：response_replace（响应体替换）

### Response body replacement (`response_replace`)

`response_replace`（JSON 对象或 `null`）重写站点响应体中的字符串。省略该键保留
已存储的策略不变；`null` 清除；`rules: []` 的策略同样视为已清除。文档结构：

```json
{
  "path": "^/status",
  "status_codes": [200],
  "rules": [
    { "pattern": "Google", "replacement": "PingWAF", "is_regex": false }
  ]
}
```

- `path` —— 请求路径必须匹配的正则；空表示所有路径。
- `status_codes` —— 替换适用的状态码；空表示全部。每项须在 100–599。
- `rules` —— 按顺序应用，至多 20 条。`pattern`（1–512 字节）默认按字面量匹配，
  `is_regex: true` 时按正则编译，`replacement` 支持 `$1` / `$name` 捕获引用；
  `replacement` 为 1–2048 字节。两侧都**不能包含单引号**（数据面规则语法以 `'`
  界定 pattern 与 replacement），且都不能为空。

违反约束在写入时以 `400` 拒绝——否则数据面构建不了的规则会拖垮整个站点的 agent
配置。策略最终成为生成 agent 配置中的站点级 `sub_filter` 插件实例。

**压缩编码支持（Unreleased 扩展）：** 插件理解 `gzip` / `x-gzip`、`deflate`
（zlib 封装；raw deflate 作为回退）以及 **`br`（brotli）** 与 **`zstd`**：解压 →
应用替换 → 用同一算法重压缩，保留 `Content-Encoding`。语义一致的限制：

- 解压后明文上限 **8 MiB**，超限按原始压缩字节透传；
- 解压失败、重压缩失败或**无任何命中**时，输出原始压缩字节（无命中时与源字节
  逐位一致，ETag 与完整性摘要仍然有效）；头部调整在响应被插件接管时统一进行
  （见下一条），因此即使正文回退为原始字节，头部也已按下方规则处理；
- 头部处理一致：删除 `Content-Length`、响应改为 chunked、强 `ETag` 弱化为
  `W/`、删除 `Content-MD5` 与 `Digest`、添加 `Vary: Accept-Encoding`；
- 多值编码（如 `gzip, br`）仍按不支持透传，不做替换。

详见：[Response body replacement (`response_replace`)](../api.md#response-body-replacement-response_replace)。
