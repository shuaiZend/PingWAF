# PingWAF 用户指南（关键章节中文镜像）

> **本文是英文 [`docs/user-guide.md`](../user-guide.md) 的同步镜像，只覆盖关键
> 章节，不是全文翻译。** 两处不一致时，**以英文版为准**。章节标题保留英文原文，
> 便于对照；小节内的「详见」链接指向英文原文或 API 参考对应锚点。
>
> 注意：「证书链接幂等」「`cpu_source`」以及响应替换的 `br`/`zstd` 支持
> 属于 **Unreleased**（见 [`CHANGELOG.md`](../../CHANGELOG.md)）行为；英文版
> 现已同步这些变更，本镜像与之保持一致。

## SSL Certificate Management（SSL 证书管理）

### Upload without a site, bind later

管理员可以**不带站点**上传证书——适用于 PEM 已在手、同一张证书（泛域名或多主机
名 SAN 证书）要服务多个站点的场景。证书落地为**未绑定**状态：没有属主站点、
没有边缘节点在用，`status` 跟随自身有效期。之后：

1. **上传** —— `POST /api/v1/certificates`，带 `domain`、`cert_pem` 与
   `key_pem`（不带 `site_id`）。
2. **逐个绑定站点** —— `POST /api/v1/certificates/{id}/bindings`，带
   `{"site_id": "…"}`。证书不覆盖站点主机名时绑定失败（`400`，列出未覆盖的
   **站点**主机名；证书多出的 SAN 无妨）；站点已拥有该证书时 `409`；幂等——
   重复绑定不改变任何状态。
3. **解绑** —— `DELETE /api/v1/certificates/{id}/bindings/{site_id}`；证书仍是
   该站点当前 TLS 选择时被 `409` 拒绝。

每个证书响应以 `bound_sites` / `bound_count` 展示结果。未绑定证书不能用 ACME
（签发与续期在边缘运行，由承载证书的站点驱动）；仍有绑定的证书删除被 `409`
拒绝，无绑定后立即可删。站点侧的替代入口仍然可用：从站点 SSL 设置的选择器里
发起链接，见 [Sharing certificates between sites](#sharing-certificates-between-sites)。
详见：[Upload without a site, bind later](../user-guide.md#upload-without-a-site-bind-later)。

### Certificate lifecycle

证书在四个状态间流转，每个状态决定你可以对它做什么：

```
              带 site_id 上传              不带 site_id 上传
                     │                            │
                     ▼                            ▼
            ┌─────────────────┐          ┌─────────────────┐
            │  Owner（属主）  │          │  Unbound（未绑定）│
            └─────────────────┘          └─────────────────┘
                     │                            │
        绑定其他站点  │ 共享                       │ 绑定第一个
                     ▼                            │ 站点
            ┌─────────────────┐                    │
            │  Shared（共享）  │ ───────────────────┘
            └─────────────────┘
                     │
   某站点（属主或    │ 站点 SSL 页选择
   已链接）选择其    ▼ 其为 TLS 证书
   为 TLS      ┌─────────────────┐
               │   In use（使用中）│
               └─────────────────┘
```

- **Unbound（未绑定）** —— 不带站点上传（`site_id: null`，`bound_count: 0`）。
  没有边缘在用；ACME 与 `activate` 被拒（`400`），因为二者都是站点级操作。
  任何时候都可删除（`204`），或绑定站点投入使用。
- **Owner（属主）** —— 证书创建时所在的站点。这正是把证书绑定*回*该站点被拒
  （`409`）的原因：它已经拥有它。属主同时驱动续期与 ACME 记账。
- **Shared（共享）** —— 通过证书绑定共享给其他站点（`bound_count` 只统计这些
  **共享站点，不含属主**）。
- **In use（使用中）** —— 至少一个站点已把该证书选为 TLS 证书。这可以是属主
  自己的选择，因此证书可以没有任何共享绑定而处于使用中。

**推荐操作顺序**（控制台引导的同一序列）：

1. **上传** —— 带站点（`Sites → [site] → Certificates → Add`）或不带站点
   （见 [Upload without a site, bind later](#upload-without-a-site-bind-later)）。
2. **绑定** —— 从证书的绑定对话框。自己的站点显示 **owner** 徽标：不可勾选、
   没有解绑按钮。其他站点要么已绑定（可解绑，附带「站点将回退到自身证书或无
   HTTPS」的确认提示），要么可绑定——绑定会校验证书覆盖站点主机名，未覆盖的
   以 `400` 列出。一次绑定多个站点时逐个提交：成功保留，失败以
   `site — reason` 列出，全部成功才关闭对话框。
3. **解绑（409 处理）** —— 证书仍是该站点当前 TLS 选择时被 `409` 拒绝。先去
   该站点的 SSL 页，切换到另一张证书或清除选择，再回来解绑。解绑一个只通过
   链接使用该证书的站点，会让它回退到自身证书材料——或完全没有 HTTPS，这正是
   确认提示存在的原因。
4. **删除** —— 仍有站点选择该证书**或**仍有绑定授予它时被 `409` 拒绝。删除
   对话框先查询证书的 usage：若有站点在使用或已链接，列出受影响站点，按钮变为
   **Go handle**，带你到第一个阻塞站点的 SSL 页。usage 查询本身不可用时，删除
   被保守拒绝而不是放行。没有任何站点选择、没有绑定后，删除立即生效（`204`）。

详见：[Certificate lifecycle](../user-guide.md#certificate-lifecycle)；
`409` 的服务端语义见
[API 参考的证书章节](../api.md#certificates)（含
[GET /certificates/{cert_id}/usage](../api.md#get-certificatescert_idusage)）。

### Sharing certificates between sites

一张泛域名或多主机名证书可以服务多个站点。证书保持唯一**属主**（创建时所在的
站点）；其他站点*链接*到它来使用：

在站点的 **SSL Settings** 页管理（证书选择器）：「链接证书」选项列出可用的
证书，已链接的证书在同一选择器中带共享标记显示。

- **链接** 列出你可读站点拥有、本站点既不拥有也未使用的证书。站点已拥有时失败
  于 `certificate already belongs to this site`。
- **重复链接是幂等的**：同一证书已链接时再次调用返回 **200** 与完整装饰的证书
  对象（首次创建为 `201`），不再报 `409`；仅当站点**拥有**该证书时仍是 `409`。
- **选择** 已链接的证书与自有证书完全一致——SSL 设置选择器同时显示两者，共享的
  带标签。
- **解除链接** 在证书是该站点当前 TLS 证书时被拒
  （`certificate is the site's current TLS certificate; clear or replace it
  first`）——否则站点会静默失去 HTTPS。先清除证书选择（或换一张）。
- **回退服务**：未选择证书、也无内联 PEM、ACME 订单或自签名证书的站点，会自动
  服务覆盖其主机名最多的已链接证书——一张都不覆盖的会被跳过并告警，而不是被
  服务。注入的证书不参与续期：服务非自有证书的站点 ACME 保持关闭。

**属主模型的意义：** 续期、ACME 记账与配置备份都挂在属主站点上，证书只续期一次，
所有已链接站点自动拿到新材料。**谨慎共享**：被链接站点会服务相同的主机名，只把
证书链接给这些主机名的合法承载者。
详见：[Sharing certificates between sites](../user-guide.md#sharing-certificates-between-sites)。

## Origin Pools & Routes（源站池与路由）

### Routes

路由按路径把请求分派到不同池；未匹配流量走默认池。

| 字段 | 说明 |
|-------|-------------|
| 规则名 | 路由标签（如 `api-to-dedicated-pool`） |
| 匹配 | `Prefix`、`Exact` 或 `Regex` |
| 路径 | 以 `/` 开头（如 `/api`），或正则（如 `^/static/.*`） |
| 优先级权重 | 1–60000；留空使用自动权重（exact 1024 / prefix 512 / regex 256） |
| 目标池 | 接收匹配流量的池 |
| Upstream host | 可选；覆盖该路由上游看到的 `Host` 头。空 = 转发客户端的 `Host`（历史行为） |
| 启用 | 不删除即可开关路由 |

`/` 上的前缀路由会被拒绝：会与默认池兜底冲突。

**Upstream host.** 路由的池对接按 `Host` 路由的虚拟主机时填写——例如指向
`google.com` 的镜像路由：不覆盖的话上游看到你的站点域名并返回 404。取值必须是
裸主机名（至多 253 字节，不允许 scheme `://`、路径 `/`、空白或控制字符；接受
端口）。同一路由的其他改写开关：**Strip path prefix** 移除匹配前缀
（`/api/users` → `/users`）；**Rewrite path** 用固定值替换**整个**转发路径，两者
同时设置时 rewrite path 胜出；query string 均保留。`strip_prefix` 只对
`match_type: "prefix"` 路由生效，其他匹配类型组合会 `400`。精确语义见
[API 参考的 Path rewriting](../api.md#path-rewriting-strip_prefix--rewrite_path)
与 [Upstream Host override](../api.md#upstream-host-override-upstream_host)。
详见：[Routes](../user-guide.md#routes)。

## System 节点视图（System node view）

> 英文版用户指南暂无对应小节；本节对应英文 API 参考的
> [GET /system/nodes](../api.md#get-systemnodes) 与控制台 **/system** 页面
> （仅管理员，从账户下拉进入）。

`GET /api/v1/system/nodes` 一次返回机队视图：控制面进程计数器（`control`）+
每个已注册 agent 一行（`agents[]`，按 `name` 排序，轮询时表格不重排）。

**agent CPU 的口径（Unreleased 新增 `cpu_source` 字段）：**

| `cpu_source` | 含义 |
|---|---|
| `probe` | 来自 agent 主机探针的 **5 秒区间差分**样本——瞬时读数，且跨 agent 版本可比（探针口径一致）。轮询之间数值可能剧烈波动，这是预期行为 |
| `heartbeat` | 无探针样本时回退到心跳值。注意：0.28.3 之前的心跳值是**自启动以来的平均**，与探针口径不同，横向比较旧 agent 时要留意 |
| `none` | 探针与心跳都没有读数（`cpu_percent` 为 `null`） |

编排告警或画图时按 `cpu_source` 分组解读：`probe` 与 `heartbeat` 数值不可直接
混排。其余字段沿用既有口径——控制面尚不知道的字段以 `null` 而非 `0` 表达；
`agents.memory_bytes` 在 agent 首次心跳之前携带主机总内存（注册清单值），之后是
探针的使用内存（既知漂移，尚未修复）。`boots[]` 记录控制面启动史：相邻两行的
间隔即上一进程存活时间；**几分钟内多次启动通常意味着容器被循环杀死**，而一周前
的一次启动只是正常发版。
详见：[GET /system/nodes](../api.md#get-systemnodes)。

## Request/Response Rewriting（请求/响应改写）

### Response Body Replacement（响应体替换）

除请求头外，站点还可以重写响应体内的字符串。在站点 **Rewrite Rules** 页配置
（卡片位于规则列表下方），或通过 `PUT /api/v1/sites/{site_id}` 传
`response_replace` 对象——完整字段列表见
[API 参考的 response_replace](../api.md#response-body-replacement-response_replace)：

```json
{
  "path": "^/status",
  "status_codes": [200],
  "rules": [
    { "pattern": "Google", "replacement": "PingWAF", "is_regex": false }
  ]
}
```

| 字段 | 含义 |
|-------|---------|
| `path` | 请求路径须匹配的正则；空 = 所有路径 |
| `status_codes` | 需要改写的状态码；空 = 全部。每项 100–599 |
| `rules` | 至多 20 条替换，按顺序应用 |
| `rules[].pattern` | 文本（1–512 字节），字面量匹配；`is_regex` 时按正则编译（必须能编译） |
| `rules[].replacement` | 替换文本（1–2048 字节）；`is_regex` 时 `$1` / `$name` 引用捕获组 |
| `rules[].is_regex` | 将 `pattern` 按正则而非字面量编译 |

两条硬约束，保存时 `400` 强制：pattern 与 replacement 都**不能包含单引号**，
且两侧都不能为空。清除策略用 `null`——或空的 `rules` 列表，同样视为「无策略」。

**压缩响应的编码支持（Unreleased 扩展）：** 替换功能理解 `gzip` / `x-gzip`、
`deflate`（zlib 封装；raw deflate 回退）以及 **`br`（brotli）** 与 **`zstd`**：
正文解压 → 改写 → 以**同一算法**重压缩，`Content-Encoding` 保留。要点：

- 解压后明文上限 **8 MiB**（防压缩炸弹），超限按原始压缩字节透传；
- 解压失败、重压缩失败或**无任何命中**时输出原始压缩字节——无命中时输出与源
  字节逐位一致，ETag 与完整性摘要保持有效；
- 发生改写的响应：删除 `Content-Length`、改为 chunked、强 `ETag` 弱化为 `W/`、
  删除 `Content-MD5` 与 `Digest`、添加 `Vary: Accept-Encoding`；
- 多值编码（如 `gzip, br`）仍按不支持透传，不做替换。

因此：源站若以 brotli 或 zstd 压缩响应（此前会被整体透传、替换不生效），现在
替换照常工作。
详见：[Response Body Replacement](../user-guide.md#response-body-replacement)。
