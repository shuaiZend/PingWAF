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

### P1 检测面扩展（中风险，预计再 +3~5pp、FP 可控）——已落地 ✅

4. **JSON 递归解包**（L7）✅：`unpack_json` 以 `serde_json` 解析 JSON body（限深 6 层/值长 4KB/最多提取 64 值；深度预算覆盖埋点 JSON-in-JSON 三四层嵌套），成员 string 值按「离散输入字段」独立进解码+扫描；原 body 保留为 blob 走弱信号。`Content-Type` 含 json 或内容形态判别双入口，同时解决 `\u003c` 转义混淆（serde_json 解析后天然还原）。**字符串成员本身是序列化 JSON 时就地再解包并替换容器**（`unpack_string_json`：埋点 icon/载荷内嵌 JSON；纯数字/布尔成员也产出，保证替换语义；深度预算拒绝时不替换容器避免丢值）；表单字段值同语义（`param={"add":5,"delete":0}` 不再踩 libinjection quote-keyword）。
5. **JS 调用形态检测**（L4）✅：`detect_js_call` 落地为 Strict-only（`parent['eval'](`、`this["constructor"]["constructor"](`、hex 转义变体），Strict xss 33.3%→80%。hex 解码未扩到 Normal（`\x` 标记值在 Normal 的收益面为零——该形态样本全部被 Strict 语义检测覆盖，Normal 扩面只增误报不增拦截）。
6. **联合评分升级**（L6）✅：由 P0-3 的 PT-013/014/015 敏感文件 needle（sev5 critical）提前化解——「遍历前缀 + 敏感文件」复合形态直接单命中拦截，无需组合规则。
7. **CRLF/SSRF needle 扩面** ✅：CRLF 分源 critical 收敛为**头名注入形态**——解码值中 CRLF/CR/LF 混排（`\r\n` 与 `\r\r\n\n` 规避等价处理）后紧跟 `name:` 头名形态才 critical（query/path/header/cookie 任意面）；裸多行文本（表单多行输入回传 query、散文）只评分不拦（白样本实测：百度翻译多行 query、mathb.in 文档、StackBlitz 代码文件均为此形态）。body 面始终只评分。SSRF 补回环变体 SSRF-013~017（`localhost`/`127.0.0.1` sev4、`0x7f000001`/`2130706433`/`0177.0.0.1` sev5）与 CI-032~037（`invoke-expression`/`iex (`/`cmd /c`/`powershell -enc`/`system(`）。
8. **SQL union-select 语义门**（P2-8 的第一步落地）✅：`sqli_union_statement_shaped` 判定解码值是否具备注入语句结构特征（引号破出 `['"`]union`、常量探测 `union select <数字/引号/null>`、注释终止符/FROM 子句）；四特征全缺的 `union select` 命中（needle SQL-001/002 与 libinjection union-select 指纹）在 Normal **零分跳过**（needle 与 lib 指纹描述同一字符串，镜像多面不放大），压掉搜索短语类误报（`site:x.com union select 关键词怎么用` 是 FP 大头，sqli FP 23.9%→1.5%）；Strict 不设门（该档即激进）。
9. **弱信号评分语义修正** ✅：两轮实测迭代后的最终形态——(a) 降权命中仍进 family 子评分（sev2→24），**三轮实测证明完全脱离 family 会漏掉 Referer/UA 承载的真实反射攻击**（DVWA 型 3 个独立弱特征过门样本，拦截率 -6pp）；(b) 无语句结构的 `union select` 短语命中改为**零分跳过**（needle 与 libinjection 指纹同现只描述同一字符串，镜像多面不放大，Pass 快路径零开销）；(c) `FIELD_STRONG_MAX_LEN`（256B）：离散 body 字段解码值超长按 bulk 内容弱信号化——粘贴的代码/文档（mathb.in 7KB 数学文档、StackBlitz 源码文件）常含 `<script>`/引号关键词/CRLF 样板，短载荷（真实注入 <256B）不受影响。
10. **script-URI HTML 上下文门 + 分源语义** ✅：`xss_script_uri_html_shaped` 判定值内是否具备 HTML 标签结构（`<svg onload=…>`、`"><img …>`）——真注入需要标签上下文执行，遥测埋点把 DOM 属性原样序列化进 JSON（`{"href":"javascript: void(0);"}`，beacon 类 16 条误报全部此形态）。语义：**body 源**无标签结构的 `javascript:` 命中（XSS-007 + libinjection js-uri 指纹）在 Normal 零分跳过——blob 与解包成员的镜像不能靠弱信号累加踩过 XSS family 门（PINGWAF-1003）；**query/cookie/header 源**的裸 `javascript:` 保留 critical（`?url=javascript:alert(1)` 是真实反射形态）。Strict 不设门。

### P2/P3 确定性扩面（第三轮实测驱动）——已落地 ✅

对 Strict+body 档 238 条漏报全量归因（b64 递归解码 + URL 双轮 + `\u` 反转义三轮脚本迭代 + 6 个疑问样本 spot 回放确认引擎缺口）后落地的确定性方案：

11. **SQL 强正则扩面 + comment-stripped 重扫** ✅：10 个强检查抽为 `sqli_strong_checks` 共享函数（直接 pass 与注释剥离重扫复用）。新增 bare-select（裸 `SELECT … FROM` 表枚举，列名清单形态要求，散文 `select a gift from our store` 不触发）、blind-probe（`SELECT CASE WHEN (…) THEN …`/`TO_CHAR(1/0)` Oracle 盲注词典）、truncated-tautology（`1' or ''='` 截断恒真尾）、inline-comment 重扫（`OR/**/"1"="1"` 关键词注释切分）。三个新正则各带语义门：blind-probe 的泛化 `case when…then` 备选**要求 SQL 操作符前缀**（select/and/or/`||`/`;`/引号/括号）——散文 "in the case when … then"（waf-bench 白样本 7 条）不触发；bare-select **跳过完整查询子句**（WHERE/GROUP BY/ORDER BY/HAVING/LIMIT/OFFSET/JOIN）——词法上不可分的合法查询类（`q=SELECT * FROM users WHERE slug='x' LIMIT 1`）保持通过，与 P1 散文 union-select pin 同一取舍。
12. **base64 值级展开**（normalize）✅：charset 快检 → core≥12 → 补 padding → STANDARD 解码 → UTF-8+可打印率 >90% 门（md5 hex 解码为二进制被拒），解码值含 `%` 再走一层 `decode_value`，形似 JSON 则 `unpack_string_json` 替换容器（quote-keyword FP 防线语义复用），否则以 `name.b64` 附加值进扫描面（blob/超长弱信号门自然兜底）。双层递归（`B64_MAX_DEPTH=2`）覆盖 JSON+\u 嵌套的 b64 包装家族。
13. **overlong UTF-8 字节级还原** ✅：`urlencoding::decode` 遇 `%C0%BC`（无效 UTF-8）整体 Err 导致后续载荷不可见；重写 `multi_decode` 为字节级 percent 还原 + `bytes_to_scannable`（C0/C1 lead 按双字节规则折叠，剩余 Latin-1 映射），`<C0%BCscript%3E` 类绕过面闭合。UTF-7 明确不做（现代浏览器已废弃，收益面为零）。
14. **needle/正则扩面** ✅：PT-016 `win.ini` sev5；CI-038~042（`|whoami` sev5、`;whoami`/`|uname`/`` `uname` ``/`;uname` sev4）；DZ-012~015 OGNL/Struts2（sev4，配合 deser-shape 分级）；XSS-020/021 prototype 链/JSFuck + `XSS_DOM_CHAIN` 正则（`constructor.prototype` 调用链）；CI-043 `getruntime().exec` sev5——OGNL/SpEL RCE 的载荷核心（`@java.lang.Runtime@getRuntime().exec(`、`T(java.lang.Runtime).getRuntime().exec(`），Normal 即 critical。
15. **deser/OGNL 形态分级** ✅：`detect_deser_shape`（OGNL 静态调用 `@class@method(`/PHP 序列化 `O:\d+:"`/`s:\d+:"…";s:\d+:"` 记录形态）在 Normal sev4 评分（OGNL 对非 Java 站点无解释面）、Strict sev5 critical（deser 家族在 strict 档整体 critical）；弱信号门（meta header/body blob/超长）与 needle/lib 门一致。
16. **P2 实测驱动的语义门回修**（两轮 bench FP/回退归因后）✅：SQLI_TAUTOLOGY 收紧为「数字自等/引号对/1<>0」三形态——`word = string` 比较（Lucene/API 过滤语法 `author=="CT Stack"`）不再是注入信号；SQLI_QUOTE_KEYWORD 从「任意距离引号+关键词」收紧为**邻接判定**（引号后 `\s);` 容差内直接跟关键词）——缩写撇号（"couldn't select"）与 b64 词表长距误触发闭合。收紧连带暴露两类 P1 靠宽松 quote-keyword「误打误撞」拦截的攻击面，补确定性检测承接：**bool-subquery 强检查**（操作符前缀子查询 `and (select …` / `N=(SELECT …)` 比较，PortSwigger 布尔盲注家族）、**SQL-020 `utl_inaddr`**（Oracle 报错注入）、**CI-043 `getruntime().exec`**（OGNL/SpEL RCE 载荷核心，承接 6 条 b64 包裹 OGNL 黑样本在 Normal 的拦截）、**CI-044 `securegroovy`**（Jenkins 沙箱绕过端点）、**DZ-016 `getclass().forname`**（EL/SpEL 反射 gadget 链，Nexus CVE-2020-10199/10204 家族）。回修后 16 条新增误报全部消除、59 条新增拦截零回退、8 条 PortSwigger 盲注重新拦截，并顺带压掉 18 条 P1 遗留遥测类误报。

### P3 语义架构演进（高性能高命中低误报的地基）

17. **token 归一化语义评分**（SafeLine 思路的确定性近似）：解码后对每个值做轻量词法切分（复用 SQL 指纹器），按语法角色计分：`union+select`（两个 keyword 相邻）> `union`（孤立）> 普通词。比信任度分级更通用，且 O(n)；P2-11 的语句结构门已覆盖其最高价值场景（union select 搜索短语 FP），剩余收益面在深层混淆样本。
18. **label 化管道 + count 灰度**（AWS/CF 模式）：DSL 增加 `label("...")`/`count` 动作，规则命中只打标不判决，管道末端按 label 组合统一裁决；新规则默认 count 上线观察。这是后续所有规则扩张的误报保险，也是 strict 档 2.38% 误报的正解。
19. **参数名感知排除**：站点/规则级 `exclude`（按参数名/路径），CRS `ctl:ruleRemoveTargetById` 的静态版；把误报治理从改规则变成加白名单。
20. **Vectorscan 预留**：托管正则规则超 ~500 条时把 `matches` 编进 Vectorscan 数据库做第二层（AC 粗筛 + 正则精确认证）；当前 15 条正则的 regexset 路径不动。

### 收益矩阵（基于 bench 归因的保守估算）

| 方案 | 拦截率提升 | 误报影响 | 工作量 |
|---|---|---|---|
| P0-1 `+` 解码 | ✅ 已落地（sqli 70→85% 的组成部分） | 无（归一化等价） | 小时级 |
| P0-2 body 检测 | ✅ 已落地（+1.6pp Normal / +12.7pp Strict） | 已治理（+0.02pp Normal；Strict 1.98% 为档位设计） | 1 天内 |
| P0-3 needle 补齐 | ✅ 已落地（lfi +36pp、sqli +15pp） | 极低（sev5 均为强特征） | 小时级 |
| P1-4~10 | ✅ 已落地（Normal+body 38.1→44.1%，+6.0pp；crlf 0→40%、deser 0→33%） | **正向**（Normal 档 FP 0.23%→0.18% 全程最低；Strict 档 2.38% 为成员级检测的档位代价） | 2-3 天 |
| P2/P3-11~16（第三轮实测驱动） | ✅ 已落地（Normal+body 44.1→55.5%，+11.4pp；deser 33.3→100%、xss 33.3→40%、other 42.8→54.8%） | **正向**（回修后 Normal 档 FP 0.18%→0.13%；18 条 P1 遗留 FP 顺带消除；Strict+body 仅 2 条新增 FP） | 2-3 天 |
| P2-8/9/10 | 间接（支撑 FP <0.1% + 规则扩容） | **正向（FP 治理基建）** | 1-2 周 |

P0 全落地实测：Normal 36.5% / 0.21%，Normal+body 38.1% / 0.23%，Strict 51.1% / 0.35%，Strict+body 63.8% / 1.98%。P1 全落地实测：Normal 36.8% / 0.15%，Normal+body **44.1% / 0.18%**，Strict 51.4% / 0.35%，Strict+body 63.8% / 2.38%——主档（Normal+body）拦截 +6.0pp、误报同时下降，语义引擎（SQL 语句结构门、script-URI HTML 上下文门、字符串 JSON 再解包、CRLF 头名形态分源）首次实现拦截与误报同向改善。P2/P3 全落地实测（四轮迭代后）：Normal 46.8% / **0.10%**，Normal+body **55.5% / 0.13%**，Strict 63.8% / 0.32%，Strict+body **77.7% / 2.17%**——四配置零回退，三档零新增 FP；主档累计 +17.4pp（P0 38.1% 起），编码展开（b64 递归 + overlong UTF-8）+ 强正则扩面 + 形态分级把 strict+body 推到 77.7%（对齐 CRS PL3 上限水平）。strict 档剩余误报治理归 P3 label/count 灰度基建。

## 5. 明确不做

- 内嵌 ML 模型（SafeLine NLP/CF attack score）：样本内收益低于确定性组合信号，且引入模型分发/漂移维护成本；ScoreClass 分档结构已预留未来接入点。
- 全量正则换 Vectorscan：当前规模无收益，113 needle 的 AC 已在 GB/s 级；仅在规则面破千时启用（P3-20 预留）。
