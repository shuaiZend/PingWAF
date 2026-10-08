# 代码评审报告：通知系统 / fail_open 站点级改造 / 配置版本回滚

- 日期：2026-10-08
- 范围：`feature/per-site-failover` 分支（08d9c065..8305d719）及前置的配置版本化+通知系统（f6317fef）
- 评审方法：source→sink 攻击链分析 + 数据流追踪 + 语义正确性核对
- 分级：P0（安全不变量被破坏，需立即修）/ P1（可被利用或造成可用性/正确性损失）/ P2（加固与优化建议）

---

## 一、P0 — 版本快照泄露 TLS 私钥（安全不变量回退）

**位置**：`pingwaf-server/src/config_history.rs::capture_site_snapshot`（snapshot_table! 宏）
**涉及表**：`site_ssl`、`mtls_cas`、`mtls_client_certificates`

**证据链**：
1. 三张表均含 `key_pem` 私钥列。`sites.rs:181` 明确声明安全不变量：
   > `key_pem` is a private key; **it is serialised nowhere (the API strips it)** and only leaves the control plane inside the gRPC config pushed to agents.
2. `mtls.rs:5` 同样声明 `key_pem` 是私钥材料，且"写一次、交还操作者一次后可选择清除"。
3. `capture_site_snapshot` 用 `serde_json::to_value(&rows)` 把 `site_ssl`、`mtls_cas`、`mtls_client_certificates` **整行序列化**（含 `key_pem`）写入 `config_versions.snapshot` JSONB。
4. `GET /api/v1/config-versions/{id}`（`api/config_versions.rs::show_version`）把**完整 snapshot 原样返回**给管理员 —— 这是该项目有史以来第一条把 `key_pem` 送出 REST API 的路径。
5. 保留策略（每 scope 50 份 × 30 天）将私钥的暴露窗口从"一份当前配置"放大到"50 份历史快照"，随 DB 备份/逻辑复制/日志采集管道扩散。

**攻击面**：管理员账号被钓鱼/会话劫持 → 一次 GET 拖走全站 TLS 私钥 + mTLS CA 私钥 → 可签发客户端证书通过 mTLS 认证、对域名做中间人。DB 备份泄露同样直接获得全部历史私钥。

**修复建议**：
- 捕获时剥离：`capture_site_snapshot` 对这三张表的行把 `key_pem` 替换为 `null`（或指纹占位）；
- 恢复时回填：`restore_site_table!` 对这三张表复用站点行已有的 `merge_objects` 逻辑——快照中 `key_pem` 为 null 时保留现网行的值；
- 兜底：对已产生的快照，可执行一次性清洗 SQL（`jsonb_set` 置空 `tables->site_ssl[*]->key_pem` 等）。

---

## 二、P1 发现

### P1-1 Webhook 通道 SSRF + 内网响应预言机

**位置**：`pingwaf-server/src/notify/webhook.rs`（send_wecom/send_dingtalk/send_generic）、`api/notifications.rs::test_channel`

**攻击链**：管理员创建 generic webhook 通道，`url` 填 `http://169.254.169.254/latest/meta-data/`（或任意 RFC1918/localhost）→ `POST /notifications/channels/{id}/test` → 控制面服务器发起请求 → `test_channel` 把**远端响应体前 200 字节**（`webhook.rs::post_json` 截断逻辑）作为 error 字符串返回给 API 调用者。

**放大因素**：
- `reqwest::Client` 只设置了 10s 超时，**redirect 策略为默认（跟随最多 10 跳）**：即使加了 URL 校验，302 也能跳进内网；
- 无 scheme 白名单、无私网 IP 黑名单、无 DNS 重绑定防护；
- 通道创建/测试虽为 admin-only，但该功能把"管理员权限"升格为"以控制面主机身份访问内网"，违反最小特权。

**修复建议**：
- 请求前解析目标 host，拒绝 loopback/RFC1918/链路本地/组播地址（解析后逐连接校验，防 DNS rebinding）；
- `redirect(Policy::none())`；
- test 接口的错误只返回状态码与错误类别，不回显响应体；
- 可选：通道 URL 域名白名单（wecom/dingtalk 官方域名 + 运维配置的自定义白名单）。

### P1-2 Host 未归一化 → 断连 fail-closed 策略可被大小写/尾点绕过（并放大既有规则绕过）

**位置**：`pingap-core/src/http_header.rs::get_host`（strip_port 但**不 lower-case、不去尾点**）→ `pingap-plugin/src/waf.rs::resolve_site` → `get_rules_for_domain` / `failover_for_domain`（HashMap 精确匹配）

**攻击链**（断连场景）：站点 `example.com` 策略为 fail-closed → 攻击者发送 `Host: EXAMPLE.com.` 或 `Shop.Example.Com` → 域名索引与 failover 注册表均精确匹配失败 → 落入"未知 host"分支 → 走全局默认（fail-open）→ 503 拒绝被绕过。

**放大**：同一不匹配在**连接正常**时更严重——`get_rules_for_domain` 未命中 → `is_connected()==true` → `EngineChoice::Base` → 该请求**完全绕过站点 WAF 规则/IP 规则/限流**（这是改造前就存在的缺陷，fail_open 只是新增了一个受影响的消费方）。

**修复建议**：在 `get_host`（或 resolve_site 入口）统一 `to_ascii_lowercase()` + 去掉尾部 `.`；`failover_registry` 与 `domain_index` 的写入端同样归一化。这是单点修复，同时收敛两个消费方。

### P1-3 旧快照在 schema 演进后回滚必然失败（文档承诺未兑现）

**位置**：`config_history.rs::restore_site_table!`（宏直接 `serde_json::from_value::<Model>`）

`config_history.rs:19-21` 声称 "unknown keys of an older snapshot are filled from the live row, which keeps old versions restorable after a schema migration adds columns"。该逻辑只对 `site` 主行实现了（merge_objects），**19 张子表均未实现**：所有 Model 字段为非 Option 且无 `#[serde(default)]`（已核 `rules.rs`），快照生成之后新增任何非空列 → 旧快照反序列化报 `missing field` → 整个回滚事务报 500，**且此前已删数据的表在事务内回滚（无数据损坏，但功能不可用）**。

**修复建议**：给快照解码容错——为子表实现"以现网行补齐缺失字段"的合并（与站点行同法），或要求所有快照内 Model 字段带 `#[serde(default)]`（成本高）；至少在文档中如实标注该限制。

---

## 三、P2 发现与优化建议

| # | 位置 | 问题 | 建议 |
|---|---|---|---|
| P2-1 | `grpc/mod.rs:55,142` | API 触发的版本记录 `actor: None`，版本历史丢失"谁改的"；回滚有 actor 但普通变更没有 | `notify_config_changed` 增加 `actor: Option<&str>` 参数，API handler 传入 `current.email` |
| P2-2 | `agent/cache/mod.rs::update_from_bundle` | `site_policies` 为空时不刷新注册表：删除全部站点后，agent 断连时仍按已删除站点的旧策略决策 | 去掉 `is_empty()` guard，注册表始终整体替换（服务端保证每 bundle 带全量注册表） |
| P2-3 | `grpc/mod.rs` 推送路径 | `record_version` 内联串行执行：每次配置推送额外做 ~19 张表的读取+序列化+INSERT，放大 API 延迟 | 快照捕获移入 `tokio::spawn`（best-effort，与通知投递同模式）；保留 push 前完成的 hash 去重 |
| P2-4 | `notification_channels.config`（JSONB） | SMTP 密码/钉钉 secret 明文落库（API 层已做 redact/unredact，做得好；但 at-rest 明文） | 引入实例级加密密钥（env/KMS）对 SECRET_KEYS 字段做应用层加密；或至少在文档中把 DB 备份列为敏感资产 |
| P2-5 | `notify/mod.rs::persist_event` | `title` 截断 200 字符但 `message` 不设上限，异常告警可写大文本 | `message` 截断（如 8KB） |
| P2-6 | `api/site_failover.rs` | 策略改为 closed 仅 tracing 日志，不产生 `security_events` 记录，安全审计面板看不到"谁把站点切到断连拒绝" | 复用 notification_event/审计事件通道记录策略变更 |
| P2-7 | `config_history.rs::prune_scope` 等 | 原生 SQL（OFFSET 分页删除、`IS NOT DISTINCT FROM`）仅兼容 PostgreSQL | 如计划支持多后端需改写；仅 PG 则加注释固化前提 |
| P2-8 | `config_versions` 保留策略 | 快照含 rules/cache/ssl 全量文本，50 份/scope 放大 DB 体积（尤其含证书 PEM） | 剥离 key_pem（见 P0）后评估；可对 snapshot 单独列压缩（TOAST 已自动，可考虑显式压缩列） |

---

## 四、做得好的地方（保持）

1. **通知 API 的 secret 处理**（`redact`/`unredact` 占位符协议）：列表/详情永远不回明文，客户端回显 `__REDACTED__` 即保留原值——模式干净，测试友好。
2. **通知投递的架构**：dedup 窗口 + 上限 10k 的抑制表、detached 投递任务不阻塞 gRPC ingest、每小时一次的历史清理、manager 单例 + ArcSwap 热更新。
3. **failover 的三级解析与兼容矩阵**：`optional bool` 防 proto3 缺省误读、未知枚举降级 Inherit（可用性开关宁可不变）、serde default 兼容旧磁盘缓存——四个方向都验证过。
4. **回滚的事务性**：单事务内先删后插、FK 顺序正确（先子后父删除、先父后子恢复）、站点行 merge 保新列——核心路径正确。
5. **failover 端到端测试矩阵**：12 组合优先级断言 + plugin 层注册表覆盖用例，直接锁死了语义回归。

## 五、修复优先级路线

1. **立即**（本分支内）：P0 快照剥离 key_pem + 恢复回填 + 存量清洗；P1-2 host 归一化（一处修复两处收益）。
2. **下个迭代**：P1-1 SSRF 三件套（私网拦截/禁重定向/错误脱敏）；P1-3 子表解码容错；P2-1 actor 贯通；P2-2 注册表空刷。
3. **排期**：P2-3 异步快照；P2-4 静态加密；P2-6 审计事件。
