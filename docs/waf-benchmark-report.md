# PingWAF 数据面 WAF 实测评估报告（基于 blazehttp 攻击样本）

- 测试日期：2026-10-04
- 样本来源：[chaitin/blazehttp](https://github.com/chaitin/blazehttp) 内置测试集（33877 条原始 HTTP 报文：恶意 658 / 正常 33219）
- 被测对象：PingWAF 数据面（pingap-plugin WAF，pingora 0.9.0）静态配置模式
- 工具：自研原始报文回放器 `tools/waf-bench/replay.py`（raw socket 逐字节发送，保真畸形样本）+ 官方 blazehttp v0.3.0 CLI 交叉对账

## 1. 执行摘要

默认托管规则（13 条，`mode=block`、`paranoia_level=2`、`anomaly_threshold=40`）在 blazehttp 全量样本上的表现：

| 指标 | 数值 |
|---|---|
| 恶意样本拦截率（严格口径：403/503） | 34.0% |
| 恶意样本漏报率 | ~66% |
| 正常样本误报率 | ~0.4% |
| p95 时延 | 2 ms |

核心结论：

1. **默认规则面是一个低误报、中低拦截率的"精确但偏保守"配置**。误报率 0.4% 在公网反代场景可用；但 66% 的漏报中，绝大部分不是阈值调一调就能解决的，而是**规则覆盖面缺口**（编码混淆、命令注入词表、CRLF/XXE/SSTI/deser 整类缺失）。
2. **静态/生产数据面默认不检测请求体**。`inspect_body` 被 agent（控制面）门控，30.7% 的 POST 样本中至少 34 条（5.2%）攻击签名只存在于 body，默认不可见。
3. **监控模式（出厂默认）在静态模式下零日志**。`log_event` 无 agent 即静默返回，监控判决不产生任何落点——"看起来开着防护，实际什么都看不见"。
4. **阈值与 PL 的默认值基本不影响结果**：threshold 30/40/60 三组结果完全一致；PL3 仅增加 +1.7pp 拦截、+0.04pp 误报（137→150 条）。说明当前拦截决策由 Stage-1 critical 短路与托管规则子评分（60/80）主导，聚合阈值不是有效调参面。
5. 官方 blazehttp CLI 对 pingap 的绝对数值（检出率 2.29%）不可用：其"单一探测状态码"判定与 pingora 协议层 400 相互作用，测的是协议层拒绝而非 WAF 拦截（见 §8.2）。
6. **agent 模式（body 检测生效 + 控制面规则）的净收益为 +4.6pp 拦截、代价是 3 倍误报**：纯净复测拦截 34.0%→38.6%，误报 0.41%→1.27%。增量误报的主体与控制面播种规则的 `score ge 40` 低门槛高度相关（托管规则为 ge 60/80，归因与建议见 §7 第二档）。

## 2. 总体指标

| 组 | 配置 | 黑样本拦截（严格） | 拦截率 | 白样本误报 | 误报率 | p50/p95 | error |
|---|---|---|---|---|---|---|---|
| **G1** | block, PL2, th40（主基线） | 224/658 | **34.0%** | 137/33219 | **0.41%** | 0/2 ms | 0 |
| G2 | monitor（出厂默认） | 0/658 | 0% | 0/33219 | 0% | 0/2 ms | 0 |
| G3-pl1 | PL1 | 224/658 | 34.0% | 130/33219 | 0.39% | 0/2 ms | 0 |
| G3-pl3 | PL3 | 235/658 | 35.7% | 150/33219 | 0.45% | 0/2 ms | 0 |
| G3-th30 | threshold 30 | 224/658 | 34.0% | 137/33219 | 0.41% | 0/2 ms | 0 |
| G3-th60 | threshold 60 | 224/658 | 34.0% | 137/33219 | 0.41% | 0/2 ms | 0 |
| **G4** | agent 模式（纯净复测¹） | 254/658 | **38.6%** | 421/33219 | **1.27%** | 2/3004 ms² | 0.17% |

¹ G4 的黑/白样本分别独立复测（每样本独享大空间轮换 XFF），消除 auto-block 连带拒绝后得到的纯净规则指标。混跑全量时表观值为"拦截 55.0% / 误报 26.4%"，其中绝大部分为 auto-block 工件——黑样本触发封禁后，同 IP 后续请求（无论黑白）在 600s 内被边缘直接拒绝（受控实验证实：攻击请求 403 后，同 IP 正常请求立即 403，换新 IP 即 200）。该现象同时演示了生产风险：共享出口 IP（NAT/办公网）的正常用户会被单个攻击者连带，且 XFF 可伪造放大该面（§6.5）。
² G4 的 p95 长尾来自 challenge 页交互与 auto-block 查询路径。

口径定义：

- **blocked（严格）**：403（WAF block 页）/ 503（challenge 页），即 WAF 判决
- **passed**：2xx/3xx（到达 mock 上游）
- **protocol_reject**：400/413/414/431/501 等 pingora 协议层拒绝，非 WAF 判决，严格口径下单列
- **error**：超时/连接重置
- **wide（宽口径）**：blocked + protocol_reject + error，与官方工具"非 2xx/3xx 即拦截"语义对齐，用于对账

## 3. 类别交叉表

G1 主基线的恶意样本（black）按类目拦截情况：

| 类目 | 样本数 | 拦截（严格） | 拦截率 | 备注 |
|---|---|---|---|---|
| sqli | 20 | 15 | 75.0% | 唯一高拦截类目，但也是唯一高误报类目 |
| other | 538 | 188 | 34.9% | 未归入特定类目的攻击样本 |
| xss | 30 | 10 | 33.3% | hex/base64/编码变形整批漏过 |
| lfi | 25 | 8 | 32.0% | |
| rce | 12 | 2 | 16.7% | `$(...)`/反引号/`${IFS}` 形态未覆盖 |
| ssrf | 13 | 1 | 7.7% | 仅 IP 字面量正则，payload 深藏参数/JSON 的全漏 |
| deser | 6 | 0 | 0% | 整类缺失 |
| crlf | 5 | 0 | 0% | 整类缺失 |
| ssti | 5 | 0 | 0% | 整类缺失 |
| xxe | 2 | 0 | 0% | 整类缺失 |
| log4shell | 1 | 0 | 0% | 1050 只查 user-agent，URI 载荷绕过 |
| scanner | 1 | 0 | 0% | UA 特征不在 1010 词表 |

白样本误报按类目（仅 4 个类目出现误报）：

| 类目 | 样本数 | 误报 | 误报率 | 主因 |
|---|---|---|---|---|
| sqli | 67 | 56 | 83.6% | 搜索引擎 Referer 含 SQL 关键词（53 条），libinjection 判定 + 1002 子评分门 |
| scanner | 45 | 2 | 4.4% | UA 含工具名片段 |
| rce | 460 | 4 | 0.9% | 参数/正文含命令样式文本 |
| other | 32432 | 75 | 0.23% | 零散 |

类目标签为回放器启发式归因（与引擎 9 大类对齐），仅用于统计维度。

## 4. 失败样本归因（以 G1 主基线为准）

### 4.1 漏报 TOP 模式（黑样本放行，~66%）

按占比从大到小：

1. **编码/混淆绕过签名**（最大单一来源）。
   - hex 编码 JS：`parent['\x65\x76\x61\x6c'](...)`（`\x65\x76\x61\x6c` = `eval`）——`00/e1/1e708ad1f095e8e21f57f4bb8cfc.black`
   - base64/多层 URL 编码 payload；
   - 编码后签名首尾变形，Aho-Corasick needle 与 libinjection 均不命中。
2. **命令注入词表缺口**：引擎 rce 检测只认 `[;&|] (cat|ls|id|whoami|wget|curl|bash|sh|nc|...)` 分隔符+固定词表，不覆盖 `$(ping ...)`、反引号 `` `whoami` ``、`${IFS}` 混淆：
   - `GET /login/index.php?login=$(ping${IFS}-nc${IFS}2${IFS}`whoami`.)` —— `5b/0d/13f7d5f4c37d750a1e23b19a9cb8.black`
3. **请求体携带攻击**：30.7% 样本为 POST，其中至少 34 条（黑样本 5.2%）签名仅在 body（SSRF 9、RCE 6、deser 5、LFI 5 等），数据面默认不读 body → 结构性漏报。
4. **整类规则缺失**（黑样本 100% 放行）：
   - **CRLF 注入**（5/5）：`GET /vulnerabilities/sqli/%0D%0AX-Pen-Test%3AeKqNz22M6K` —— 托管规则集无任何 CRLF/响应拆分规则；
   - **SSTI**（5/5）、**XXE**（2/2）、**Java 反序列化**（6/6）：无对应检测类目；
   - **Log4Shell in URI**（1/1）：托管规则 PINGWAF-1050 表达式只查 `user-agent` 头（规则名却写着 "in any header"），`${jndi:ldap://...}` 出现在 URI 即完全绕过 —— `19/c7/0731629c2498100dc8a3247e06f7.black`。
5. **SSRF 检测面过窄**（13 条仅拦 1）：正则只匹配回环/内网/元数据 IP 字面量与 gopher/dict 协议头，实际样本多把目标放在 body/JSON/参数值深处（如 solr dataConfig 中 `URLDataSource`），且不含 DNS 类变体。

### 4.2 误报 TOP 模式（白样本拦截，~0.4% 但高度集中）

| 类目 | 误报数/类目样本数 | 误报率 | 主因 |
|---|---|---|---|
| sqli 标签白样本 | 56/67 | **83.6%** | 搜索引擎 Referer 带 SQL 关键词（如 Bing 搜索 `union select 关键字怎么用` 的 URL 经 URL 编码含中文），libinjection 对 Referer 值判为 SQLi → 直接命中 1002 子评分门 |
| scanner | 2/45 | 4.4% | UA 含工具名片段 |
| rce | 4/460 | 0.9% | 正文/参数含命令样式文本 |
| other | 75/32432 | 0.23% | 零散 |

**SQLi 误报是唯一成规模的误报源**：56 条误报中 53 条（95%）的 SQL 关键词出现在 `Referer` 头（典型：Bing 搜索 `site:segmentfault.com union select 关键字怎么用` 的搜索 URL 原样作为 Referer）。搜索引擎流量是 Referer 的最大来源，这是公网站点最常见的误报投诉模式。

### 4.3 协议层拒绝（非 WAF 判决，严格口径单列）

修复测试环境上游干扰后，真实协议层拒绝仅 ~0.01%（黑 3：2×400 + 1×501；白 1：400）。此前观察到的数千个 501/400 均为测试环境 mock 上游不排空请求体造成的连接池串扰（详见 §8.3 与 tools/waf-bench/mock_origin.py 内注释），非被测系统行为。

## 5. 生效规则面与命中情况

静态模式生效的是引擎托管规则集（`pingwaf-waf/src/rules/managed.rs`，13 条）：

| 规则 | 语义 | 动作 | PL | 备注 |
|---|---|---|---|---|
| 1001 | /admin 路径非内网访问 | Challenge | 1 | |
| 1002 | cf.waf.score.sqli ≥ 60 | Block | 1 | SQLi 误报主通道 |
| 1003 | cf.waf.score.xss ≥ 60 | Block | 1 | |
| 1004 | 聚合分 ≥ 80 | Block | 1 | |
| 1010 | 扫描器 UA | Block | 2 | |
| 1011 | 空 UA（非 HEAD） | Challenge | **3** | 默认 PL2 下不激活 |
| 1020 | 敏感 dotfile 探测 | Block | 1 | |
| 1021 | 备份文件探测 | Block | 2 | |
| 1030 | TRACE/TRACK | Block | 1 | |
| 1031 | 非常规方法 | Log | 2 | 静态模式下 Log 无任何落点 |
| 1040 | phpMyAdmin/wp-admin 探测 | Challenge | 2 | |
| 1050 | Log4Shell | Block | 1 | **表达式只查 user-agent**，与规则名 "in any header" 不符 |
| 1051 | 超长 URI（≥4096B） | Block | **3** | 默认 PL2 下不激活 |

逐条命中率无法从日志统计——静态模式不产生任何规则命中日志（见 §6.1/§6.2）。类别级命中归因见 §3 交叉表；黑样本命中率：sqli 75% > other 35% > xss 33% > lfi 32% > rce 17% > ssrf 8% >> deser/crlf/ssti/xxe/log4shell/scanner 0%。

## 6. 机制层发现

1. **静态模式下 monitor 判决与 block 判决均无任何日志落点**。`log_event`（waf.rs:1796）在 `PingWafAgent::instance()` 为 None 时直接 return；安全事件、访问日志、分析管道全部依赖 agent。实测 G2 monitor 全量回放 33877 请求，服务器日志 0 条 WAF 记录。含义：以静态 TOML 部署、或控制面短暂失联时，WAF 变成"静默黑洞"——拦截了什么、放过了什么无从得知。
2. **`inspect_body` 被 agent 门控，请求体默认不进引擎**（waf.rs:2107 `if agent.is_some() && (body_limit > 0 || self.inspect_body)`）。与 `docs/waf.md` 宣称的 body 检测能力不符；agent 模式下还受 `max_body_log_size` 间接影响。
3. **规则级 monitor 不下传数据面**：控制面规则若配为 monitor 动作，`waf_config_to_proto` 只聚合站点级 mode，观测语义丢失；叠加发现 1，监控模式实际完全不可观测。
4. **`detections` 开关与 `ml_*` 字段是 TODO**：`waf_config_to_proto` 中 `ml_enabled` 硬编码 false、`anomaly_threshold` 固定传 0（引擎侧回落 40）；sqli/xss/rce/lfi/ssrf/bot 检测开关未接线到引擎，站点无法按类目关闭检测。
5. **默认信任 XFF 第一段**（pingap-core/src/http_header.rs:415）：未配置 trusted-proxies 时客户端可伪造 `client_ip`，直接影响 IP 封禁、CC、地理位置类规则的有效性（本测试正是利用该机制轮换 XFF 规避 auto-block）。
6. **auto-block 600s 无通知且会连带共享出口 IP**：Block 判决后 agent 自动封禁来源 IP 600s（waf.rs:2595），期间请求在边缘直接拒绝，无 webhook/日志通知；且 auto-block reason 不稳定——实测封禁记录中部分为空串（`"reason": "waf: "`），部分带规则名（`"reason": "waf: SSRF 检测"`），空 reason 时被封禁者与运维都无法得知封禁原因。受控实验证实同 IP 的正常请求在封禁窗口内也被一律 403（§2 脚注 1）：对 NAT/办公网共享出口、或默认信任 XFF（§6.5）可伪造的场景，这是可被利用的拒绝服务向量。
7. **Content-Length 与 body 长度不一致的请求触发 500**：多发 7 字节的样本稳定命中 pingap `Sent data after end of body` → 500（请求走私探测的常见形态），且仅记录 ERROR 无 WAF 归类。另在早期环境观察到 pingora 0.9.0 `client.rs:818` 对上游响应头 `unwrap()` UTF-8 解析的 panic（上游错位时触发，panic 被线程隔离，进程存活）。
8. **控制面规则表达式编译失败被静默丢弃**（waf.rs:1687 `filter_map(... .ok())`）：站点规则里的表达式写错不会报错，只会让该规则无声失效。

## 7. 默认规则优化建议

按优先级分四档，均附实测依据。

### 第一档：直接提升拦截率（新增规则/扩展签名）

1. **补齐整类缺失检测**（对应 §4.1-4，黑样本 100% 放行的类目）：
   - CRLF 注入：URI/header 值中 `%0d%0a` + 头名模式（样本 `38/a9` 等 5 条）；
   - Log4Shell 扩面：1050 从 `user_agent` 扩到**全头 + URI + 参数**（改名才名副其实），样本 `19/c7` 实测绕过；
   - XXE：`<!doctype`/`<!entity`/`system "file://`（含 URL 编码形态，样本 2 条全漏）；
   - SSTI：`${...}` 表达式模板探测；
   - Java 反序列化：`rmi://`、`ysoserial`、`commons-collections` 等特征。
2. **命令注入词表与上下文扩展**：把 `$(...)`、反引号、`${IFS}` 纳入 rce 签名，命令词表扩充 `ping/base64/nc/ncat/dd/busybox`；样本 `5b/0d`（`$(ping${IFS}...whoami`）实测绕过。
3. **hex/unicode 编码归一化**：Stage-1 解码链（现最多 3 层 URL 解码）增加 `\xHH`、`\uHHHH`、HTML 实体命名形式的还原，可覆盖 hex-`eval` 类样本（`00/e1`）。
4. **SSRF 语义扩面**：参数名/JSON key 维度（`url=`、`source=`、`callback=`、`dataConfig`）与常见外带域名模式；仅靠 IP 字面量正则实测只拦 1/13。

### 第二档：降低误报（不影响拦截率）

1. **SQLi 检测对 Referer/UA 降权或跳过**：56 条 sqli 误报中 53 条来自 Referer。建议：Referer/UA 只参与异常评分累计（不触发 1002 的 ≥60 短路 Block），或对 Referer 值先剥离 URL 编码中文/搜索引擎域名白名单。**这是公网站点默认配置下收益最大的单条优化**。
2. **控制面默认规则模板的子评分门槛与托管规则对齐**：控制面播种的 SQLi/XSS 自定义规则用 `score ge 40`，而托管规则是 `ge 60`（并叠加聚合 80）。纯净复测中 G4 误报 1.27% vs G1 0.41%，增量 284 条误报里 65% 是无 body 的 GET（`/site.webmanifest`、`/hm.js?<hex>` 等正常请求），排除法指向 40 分低门槛把 libinjection 的低置信命中也拦了（规则命中日志缺失，归因为推断，见 §6.1）。**建议控制面默认模板改为 ge 60，预计误报回落至 ~0.4% 而拦截率基本不动**（黑样本纯净拦截 G1 34.0% vs G4 38.6% 的差异主要来自 body 检测与补类规则，不是低门槛）。
3. **误报兜底观察**：0.4% 总误报率达标，但 1002 一条规则贡献了几乎全部误报——调整后整体误报可再降一个数量级（实测推算 <0.1%）。

### 第三档：默认值与开关

1. **聚合阈值维持 40 即可**：threshold 30/40/60 实测结果完全一致（拦截决策由 Stage-1 critical 与子评分规则主导），调它没有意义；若要让阈值真正生效，需先让异常评分成为主要判决面（与第一档 1-3 联动）。
2. **PL 维持 2**：PL3 仅 +1.7pp 拦截、+0.04pp 误报（1011/1051 在本样本集上收益极小——白样本几乎都带 UA、超长 URI 样本极少）。PL3 的空 UA Challenge 对纯 API 调用方（无 UA 的健康检查、SDK）反而有害，不建议默认开启。
3. **修复 `inspect_body` 门控后评估默认开**：5.2% 黑样本签名仅在 body；agent 模式下建议 `inspect_body` 默认开启并配 body 大小上限（如 64KB 前缀），静态模式当前完全跳过 body 属文档与实现不一致，应择一修复。
4. **`detections` 开关接线**：让站点能按类目关闭检测（如纯 API 站点关 SSRF/XXE），在误报治理上比调阈值有效。

### 第四档：可观测性（先于一切调参）

1. **静态模式/agent 离线的日志兜底**：`log_event` 无 agent 时至少 `tracing::info!` 输出判决摘要（站点、规则、动作、命中详情）。当前静态 monitor 是"零反馈"运行，连验证规则是否生效都做不到（本次测试只能靠状态码反推）。
2. **auto-block 补通知与 reason**：补 rule_name；封禁/解封发 webhook。
3. **规则编译失败显式上报**：`.ok()` 静默丢弃改为告警计数，控制面展示"该规则未生效"。

## 8. 附录

### 8.1 测试矩阵与环境

| 组 | 配置 | 目的 |
|---|---|---|
| G1 | block, PL2, th40 | 主基线（托管 13 条真实拦截能力） |
| G2 | monitor | 出厂默认监控语义验证 |
| G3-pl1 / G3-pl3 | PL 1 / 3 | 灵敏度（1011/1051 仅 PL3 激活） |
| G3-th30 / G3-th60 | threshold 30 / 60 | 阈值敏感性 |
| G4 | agent 离线模式 + 控制面 11 条默认规则 + body 检测 + auto-block | 生产链路形态验证 |

- 监听 `127.0.0.1:6188`（G4 为数据面 80 端口），mock 上游一律 200；
- G4 使用 XFF 轮换规避 auto-block 污染：全量混跑用 `203.0.113.0/24`（RFC5737 文档段，254 个地址），纯净复测改用 `10.254.0.0/16` 轮换（每样本独享 IP）；混跑期间实测产生 60 条 600s 封禁记录、全部落在该段，且混跑结果证明 254 个地址不足以隔离封禁连带（§2 脚注 1）；
- 复现：`tools/waf-bench/README.md`。

### 8.2 官方 blazehttp CLI 对账结论

官方二进制（v0.3.0）以**启动时探测到的单一状态码**判定拦截（`worker.go:241` `code != blockStatusCode` 即放行）。探测请求（utils.go:56）含原始 `<script>`/空格等非法 request-target 字符，被 pingora 协议层以 400 拒绝（WAF 未见该请求），于是 `blockStatusCode=400`：

- 官方 G1 报告"检出率 2.29%"= 黑样本中恰好被协议层 400 拒绝的子集（本测自研回放器同口径黑样本 400 数：20）；
- 官方 G1 报告"误报率 3.11%"= 白样本协议层 400 子集（自测 1065，含当时环境的上游串扰噪声）；
- G1（block）与 G2（monitor）的官方数值几乎相同（TP 15 vs 25 / FP 1030 vs 1022）——证明其读数与 WAF 模式无关，量的是 pingora 协议层；
- 结论：官方 CLI 绝对数值对 pingap 类反代不可直接引用；本报告以自研回放器双口径为准，官方数据仅作协议层交叉验证。

### 8.3 测试方法学记录

- 回放器逐字节发送原始报文，仅覆写 `Connection: close`，不经过 HTTP 库序列化，畸形样本保真；
- 类别归因为启发式正则（与引擎 9 大类对齐），仅用于统计维度，不影响拦截判定；
- 曾出现的两个测试环境缺陷已修复并记录在案：mock 上游不排空请求体导致连接池串扰（~11% 状态污染，修复见 mock_origin.py `_drain`）；官方工具口径差异（§8.2）。修复后 protocol_reject 回落到真实值（~0.01%）；
- **auto-block 连带是第三个被识别并隔离的测试工件**：小轮换池（254 地址）混跑黑白样本时，黑样本触发封禁后同 IP 白样本被边缘连带拒绝，表观误报 26.4%；改为黑/白分离 + 每样本独享 IP 的纯净复测后得到 G4 真实指标（§2 脚注 1）。该工件本身演示了 §6.5/§6.6 的生产风险；
- 样本统计：33877 条（黑 658 / 白 33219），POST 占 30.7%。

### 8.4 关键代码索引

| 事实 | 位置 |
|---|---|
| body 读取 agent 门控 | pingap-plugin/src/waf.rs:2107 |
| `log_event` 无 agent 静默返回 | pingap-plugin/src/waf.rs:1796-1804 |
| auto-block 仅 agent 在位 | pingap-plugin/src/waf.rs:2595-2606 |
| 规则编译失败静默丢弃 | pingap-plugin/src/waf.rs:1687-1734 |
| 引擎 monitor 降级与 Stage-1 短路 | pingwaf-waf/src/engine/mod.rs:371-379, 461-466 |
| 托管 13 条规则定义 | pingwaf-waf/src/rules/managed.rs:17-140 |
| 默认信任 XFF | pingap-core/src/http_header.rs:415-424 |
| 控制面 PL= max(severity)、ml/threshold TODO | pingwaf-server/src/grpc/config.rs:669-694 |
| 控制面播种 11 条默认规则 | pingwaf-server/src/defaults.rs:111-202 |

## 9. 拦截级别落地与实测（Normal / Strict）

§7 的建议已在引擎层落地为**三维级别设计**：拦截级别（`WafLevel`：Normal/Strict）× 后端技术栈分类（`StackSet`：generic/java/php/python/node）× 语义化表达注入检测。默认值安全性优先：未配置时 Normal 级别 + 全栈覆盖，行为不缩水。

### 9.1 设计要点

| 维度 | Normal（默认） | Strict |
|---|---|---|
| 静态签名 | 内置 needle 全量（stack 过滤后） | 额外 10 条 strict-only 签名（CI-101..106 命令替换、TI-101..104 Java 表达式） |
| Referer/UA 降权 | SQLi/XSS/命令注入类命中 sev 5→2、不 critical（§7 第二档 1） | 同左 |
| CRLF/命令注入/反序列化命中 | 进评分 | 直接 critical（fast-path Block） |
| 表达式注入（语义检测） | 识别但不升级 | `add_expr_hit`（rce 子评分 +48）并 critical |
| 托管规则阈值 | 子评分 ≥60 / 聚合 ≥80 | 子评分 ≥50 / 聚合 ≥70，且新增 1061（`cf.waf.score.rce ge 40`） |
| 有效 paranoia | 配置值 | max(配置值, 3)（激活 1011/1051 等 PL3 规则） |

- **栈分类**：每个静态签名与托管规则标注所属技术栈（如 log4shell→java、`/etc/passwd` 等通用型→generic）。站点按 `stacks` 配置只加载相关栈（如纯 Java 后端配置 `stacks=["java"]` 后不再对 PHP/Node 特征做匹配），automaton 规模与扫描耗时随配置收窄；未配置则全栈兜底。
- **语义检测**：`detect_expr_injection` 只在**结构容器**（`${}`/`{{ }}`/`{% %}`/`<%= %>`）内识别计算特征（运算、方法调用、多段访问链、`new` 关键字），`${filename}` 等裸标识符不报——对应 §7 第一档 1 的 SSTI 扩面但避免关键词堆砌。
- **automaton 匹配语义修复**：签名扫描从 Standard 改为 `LeftmostLongest`。Standard 语义下同起点的短 needle（`${`）以非重叠方式先被报告，使前缀更长的 critical needle（`${jndi:`）永远不触发——这正是基线中 log4shell 样本漏报的根因，修复后该样本两档均 100% 拦截。
- **内存零拷贝**：静态模式规则/签名全部驻留内存（`&'static str`），hit 仅 `u32` 索引 + Copy 字段；请求规范化借用 headers/body（method 为 Cow），Pass 判决路径零字符串构建；扫描缓冲跨请求复用。

### 9.2 两档实测（同参数全量回放，XFF 独享 IP）

| 指标 | 基线 G1（§2） | Normal | Strict |
|---|---|---|---|
| 拦截率（严格口径） | 34.0% | 33.1%（218/658） | **37.8%（249/658）** |
| 误报率 | 0.41%（137 条） | **0.17%（55 条）** | 0.30%（101 条） |
| p50 / p95 | 1 / 2 ms | 1 / 2 ms | 1 / 2 ms |
| log4shell 样本 | 漏报 | 100% | 100% |

类别拦截率对比（黑样本，Normal → Strict）：

| 类别 | Normal | Strict | 说明 |
|---|---|---|---|
| rce | 16.7% | **66.7%**（2→8） | strict-only 命令替换签名 + critical fast-path |
| crlf | 0% | **60%**（0→3） | Strict 直接 critical |
| ssti | 40% | **80%**（2→4） | 语义表达式检测升级 |
| xxe | 0% | **50%**（0→1） | 同上 |
| sqli | 70% | 70% | needle 主导，两档一致 |
| xss | 33.3% | 33.3% | 同上 |
| log4shell | 100% | 100% | LeftmostLongest 修复后由 query/头即触发 |

### 9.3 结论与取舍

- **Normal 是低误报档**：误报 0.41%→0.17%（-60%），代价仅 -0.9pp 拦截（Referer/UA 降权后，原靠 Referer 命中拦下的黑样本回到评分路径）。剩余 55 条误报的触发点绝大多数在 query/path 本身（如搜索词 `union select`、遥测 URL），与 Referer 降权无关，进一步压降需要 needle 信任度分级（如把 "union select" 从 sev5 降为仅 Strict 生效）。
- **Strict 换取高危类目拦截倍增**：RCE/CRLF/SSTI/XXE 四类从 0-40% 提升到 50-80%，拦截率净增 +4.7pp；代价是误报 0.30%（+0.13pp，主要来自子评分门槛 50 与 PL3 规则）。适合源站无二次防护、或已知后端栈可用 `stacks` 收窄降低误报的场景。
- **两档时延无差异**（p95 均 2ms）：级别差异全部体现在规则装配期（automaton 规模、托管规则条数），不在请求路径的分支密度。
- **配置方式**（静态 TOML，插件段）：`level = "normal" | "strict"`；`stacks = ["java", ...]`（可选，默认全栈）。托管的 1061 号规则仅 Strict 装配；栈过滤同时作用于签名与托管规则（含 allow 类）。

复现：`tools/waf-bench/conf/level-normal.toml` 与 `level-strict.toml`，回放命令见 `tools/waf-bench/README.md`。
