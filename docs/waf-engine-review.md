# PingWAF 引擎复盘与语义检测演进方案

- 复盘日期：2026-10-04
- 对象：`pingwaf-waf` 引擎全部源码（5839 行）+ 基准实测数据（blazehttp 33877 样本，Normal/Strict 两档）
- 方法：源码通读 + 漏报/误报样本逐条归因 + 关键假设 curl 实测复现 + 业界方案对标
- 基线：Normal 拦截 33.1% / 误报 0.17%；Strict 37.8% / 0.30%（见 `waf-benchmark-report.md` §9）

## 1. 引擎现状全景

### 1.1 检测流水线

```
RequestData
  │ normalize_request          借用式规范化：percent 多层解码(≤3)→HTML 实体→路径折叠；
  │ (Strict 另开 \xHH\uHHHH)   产出 decoded_values（path/query k-v/headers/cookies/body blob）
  ▼
Stage 1（每个解码值）───────────────────────────────
  │ Aho-Corasick 113 needles（LeftmostLongest、case-insensitive、栈过滤）
  │   → SignatureHit{u32 索引+Copy}，按 pattern 去重，sev→评分+critical 判定
  │ libinjection 算法（自研移植）：SQL 词法指纹 + 7 条强正则 / XSS 7 条正则
  │   → strong 命中即 sev5 critical
  │ (Strict) detect_expr_injection：${}/{{ }}/{% %}/<%= %> 容器内结构特征
  ▼
fast-path：critical_hit → 立即 Block（不进规则引擎）
  ▼
Stage 2：14 条托管规则（Cloudflare 风格 DSL）
  │   子评分门 1002/1003（≥60）·聚合 1004（≥80）·Strict 收紧至 50/70 + 1061(rce≥40)
  │   allow 预短路 → Block/Challenge/Log
  ▼
评分合成：total≥threshold(40) Block；>0 Monitor；否则 Pass
（Pass 零字符串构建；其余延迟格式化）
```

### 1.2 性能画像

| 阶段 | 算法 | 复杂度 | 实测 |
|---|---|---|---|
| 规范化 | 线性扫描 + `+`/`%`/`&` 检测门 | O(n)，几乎零分配（method Cow、body 借用） | — |
| 签名扫描 | aho-corasick 3.x（含 SIMD prefilter） | O(n)，字面量集天花板级 | p95 2ms 全流水线 |
| libinjection | 词法指纹 O(n) + 正则集（Aho prefilter 门控） | O(n) 均摊 | — |
| 表达式 | 线性求值，14 条 | O(rules) | — |

结论：**匹配层已在性能天花板附近，瓶颈不在速度，在覆盖**。p95 2ms 对"高性能"目标的余量在于规则面扩张时保持常数级——这决定 P2 方案的选型（见 §4）。

### 1.3 结构亮点（复盘确认保留）

- 栈分类（StackSet）在 automaton 装配期过滤，收窄零请求时开销；
- LeftmostLongest 修复了 `${` 遮蔽 `${jndi:` 的语义缺陷（log4shell 漏报根因）；
- Referer/UA 降权把 FP 从 137 压到 55（-60%），代价仅 -0.9pp 拦截；
- 静态模式规则全内存（`&'static str` + Copy hit）、Pass 判决零格式化。

## 2. 漏报/误报损失面（实测归因）

对 Normal 档 437 条放行黑样本逐条归因（启发式特征 + 抽样验证），按根因聚合：

| # | 根因 | 量级（漏报） | 证据 |
|---|---|---|---|
| L1 | **body 不进静态引擎**：插件层 `agent.is_some() && (body_limit>0 \|\| inspect_body)`（pingap-plugin/src/waf.rs:2107），无 agent 时 body 永不读取 | 186 条非幂等方法（POST/PUT/PATCH/DELETE）漏报，其中 **117 条带 payload body**（SSRF/RCE/deser/lfi 主体） | curl POST `cmd=cat /etc/passwd` → 200 |
| L2 | **query 中 `+` 不按空格解码**：`multi_decode` 只处理 percent | sqli 漏报 6 条中的 4 条 | `?id=' OR 1=1--`（`+` 形态）→ 200；`%20` 形态 → 403。实测复现 |
| L3 | **时间盲注无检测面**：`waitfor\s*\(` 要求括号，`WAITFOR DELAY '0:0:10'` 无括号形态完全漏 | sqli 2-4 条 + 潜在（sqlmap 标准载荷） | `WAITFOR%20DELAY` → 200（实测复现） |
| L4 | **JS 属性调用形态无覆盖**：`parent['eval'](...)`、`window['alert'](...)`；即使 Strict 解开 `\x65\x76\x61\x6c` 也没有任何检测面消费它 | xss 漏报 20 条的大头 | hex 样本 Normal/Strict 均 200（实测复现） |
| L5 | **needle 覆盖薄**：CRLF 仅 2 条 needle、SQLi sev5 仅 2 条、缺敏感文件目标（`/etc/passwd` 只能靠 `../` 的 sev4 弱命中）、Python 代码注入 token（`chr(`、`print`）缺失 | lfi 17、rce 10、crlf 3（Strict 升级后才 60%） | needle 分布统计（113 条） |
| L6 | **评分无联合升级**：`../`（sev4）+ 敏感路径同现只累 8 分，远低于阈值 40；多个弱信号无相乘机制 | lfi/traversal 类系统性漏 | `../../../etc/passwd%00.png` → 200 |
| L7 | **JSON/嵌套结构不解包**：非表单 body 作为整 blob 扫描，不提取 JSON 键值独立解码；libinjection 对 JSON 值内 SQLi 失效（Picus 公开绕过面） | deser/ssrf POST 类 | 架构事实 |
| L8 | 混淆解码分层开启：`\xHH` 还原 Strict-only，Normal 放弃一层归一化 | 与 L4 叠加 | — |

误报面（Normal 55 条）：触发点绝大多数在 query/path 本身——搜索词含 `union select`/`and 1=1`、遥测 URL 随机串触发 libinjection。剩余 FP 需要**参数名感知**与 **needle 信任度分级**（如把 `union select` 从 sev5 needle 降为组合信号）而非继续降权 header。

## 3. 业界对标

| 机制 | 代表实现 | PingWAF 现状 | 可借鉴 |
|---|---|---|---|
| 语义化检测：tokenize → 语法结构匹配 | SafeLine 雷池（词法/AST/威胁模型） | 只有字符串 needle + 正则 + 结构容器启发 | 轻量 token 归一化：按语法角色给关键词赋分，而非裸匹配（§4-P2） |
| 异常评分 + PL 分级 + 变换链 | OWASP CRS v4（Coraza/ModSecurity） | 已有评分/PL/transform，缺参数名感知与排除机制 | 规则级参数名 exclude；libinjection 只作加分不作一票否决（现已如此） |
| ML 评分分档 | Cloudflare attack score（1-99 四档） | ScoreClass 四档（规则推导） | 短期可落地：把"多弱信号组合"语义化进评分（L6 联合升级是它的确定性近似） |
| label 管道 + count 灰度 | AWS WAF（rule group+label）、CF observe | 规则命中即影响判决 | DSL 加 label/count 动作，新规则灰度上线（误报治理基建） |
| 正则引擎扩容 | Hyperscan/Vectorscan（Suricata 默认 MPM） | aho-corasick（113 needle）+ regexset（~15 条） | 千级规则时换 Vectorscan 或字面前缀预过滤；当前规模 AC 已够 |
| libinjection 补强 | CRS 正则链兜底 + 自有指纹扩充 | 已有 7 条强正则兜底 | JSON 深层递归解码（L7）是对它最直接的补强 |

## 4. 演进方案（按优先级）

> **落地状态（2026-10）**：P0 四项已全部实现并经 waf-bench 全量复测验证——Normal 33.1%→36.5%，Normal+body 38.1%（FP 0.21%→0.23%），Strict 37.8%→51.1%，Strict+body 63.8%（FP 1.98%）。实测明细见 `waf-benchmark-report.md` §10。

### P0 缺陷级修复（低风险，预计 +10~12pp 拦截）——已落地 ✅

1. **`+` 按空格解码**（L2）：`parse_query`/`parse_form_body` 的 value 解码在 percent 前先做 `+→space`。实测堵 `id=%27+OR+1%3D1--`、`waitfor+delay` 等漏报形态。
2. **静态模式开启受控 body 检测**（L1）：waf.rs 的 body 读取门控改为「日志采集仍需 agent，检测只需 `inspect_body=true`」，静态部署不再漏 POST 攻击。开 body 后 Normal 36.5%→38.1%（+0.02pp FP）。
3. **needle 补齐与 sev 升级**（L3/L5）：时间盲注/文件读写 SQL-004~009 sev 4→5（单命中 critical）；补无斜杠变体 PT-013/014/015 解除 AC 非重叠遮蔽（lfi 32%→68%）；补 CI-026~031 语言级调用、CRLF-003/004 特异头名。
4. **JS 括号调用结构检测**（L4，Strict-only）：`detect_js_call` 识别 `parent['\x65val'](…)` 形态，Strict xss 33.3%→80%（14 条全为此类）。
5. **配套误报治理**：Normal 下 body 中反射类家族（SQLi/XSS/CI）降为弱信号（sev≤2 不 critical），XXE-001 `<!doctype` 5→3；否则白样本 xss 类误报 46%、xxe 类 100%。

### P1 检测面扩展（中风险，预计再 +3~5pp、FP 可控）

4. **JSON 递归解包**（L7）：Content-Type JSON 时用 `simd-json`/`serde_json` 流式提取 string 值（限深 4 层/值长 4KB），每个值独立进解码+扫描。同时堵 libinjection 的 JSON 盲区。
5. **JS 调用形态检测**（L4）：扩 `detect_xss`——标识符/字符串下标 + `(` 调用组合（`parent['eval'](`、`window["alert"](`、反引号模板内 `${...}` 已有容器覆盖）；hex 解码改为 Normal 也开但仅对**已含 `\x` 标记**的值（解码器本身零分配，不会拖慢干净请求）。
6. **联合评分升级**（L6）：同请求内 traversal 类 + 敏感文件/`%00` 同现 → critical（表达为引擎内组合规则或 1061 式 DSL：`cf.waf.score.traversal ge 8 and http.request.uri.path contains "/etc/passwd"`——需给 traversal 加子评分）。
7. **CRLF/SSRF needle 扩面**：CRLF `%0d%0a` + 头名模式、`set-cookie:` 注入；SSRF 补参数名信号（`url=`、`callback=`、`source=`）与 metadata 端点（`169.254.169.254` 已有，补 `metadata.google.internal`）。

### P2 语义架构演进（高性能高命中低误报的地基）

8. **token 归一化语义评分**（SafeLine 思路的确定性近似）：解码后对每个值做轻量词法切分（复用 SQL 指纹器），按语法角色计分：`union+select`（两个 keyword 相邻）> `union`（孤立）> 普通词。`union select` 搜索词误报（当前 FP 大头）可用"关键词组合需要动词/对象结构"过滤——比信任度分级更通用，且 O(n)。
9. **label 化管道 + count 灰度**（AWS/CF 模式）：DSL 增加 `label("...")`/`count` 动作，规则命中只打标不判决，管道末端按 label 组合统一裁决；新规则默认 count 上线观察。这是后续所有规则扩张的误报保险。
10. **参数名感知排除**：站点/规则级 `exclude`（按参数名/路径），CRS `ctl:ruleRemoveTargetById` 的静态版；把误报治理从改规则变成加白名单。
11. **Vectorscan 预留**：托管正则规则超 ~500 条时把 `matches` 编进 Vectorscan 数据库做第二层（AC 粗筛 + 正则精确认证）；当前 15 条正则的 regexset 路径不动。

### 收益矩阵（基于 bench 归因的保守估算）

| 方案 | 拦截率提升 | 误报影响 | 工作量 |
|---|---|---|---|
| P0-1 `+` 解码 | ✅ 已落地（sqli 70→85% 的组成部分） | 无（归一化等价） | 小时级 |
| P0-2 body 检测 | ✅ 已落地（+1.6pp Normal / +12.7pp Strict） | 已治理（+0.02pp Normal；Strict 1.98% 为档位设计） | 1 天内 |
| P0-3 needle 补齐 | ✅ 已落地（lfi +36pp、sqli +15pp） | 极低（sev5 均为强特征） | 小时级 |
| P1-4/5/6/7 | +3~5pp | 中（JSON 解包降低误判面） | 2-3 天 |
| P2-8/9/10 | 间接（支撑 FP <0.1% + 规则扩容） | **正向（FP 治理基建）** | 1-2 周 |

P0 全落地实测：Normal 36.5% / 0.21%，Normal+body 38.1% / 0.23%，Strict 51.1% / 0.35%，Strict+body 63.8% / 1.98%（预估区间 44-46% 针对「agent 模式默认开 body」的口径；静态档按 inspect_body 可选项拆分后见 §10 两表）。P1 后预期 ~50% / FP 持平；P2 落地后具备向 60%+（对齐 CRS PL2 水平）扩张规则面的吞吐与误报基建。

## 5. 明确不做

- 内嵌 ML 模型（SafeLine NLP/CF attack score）：样本内收益低于确定性组合信号，且引入模型分发/漂移维护成本；ScoreClass 分档结构已预留未来接入点。
- 全量正则换 Vectorscan：当前规模无收益，113 needle 的 AC 已在 GB/s 级；仅在规则面破千时启用（P2-11 预留）。
