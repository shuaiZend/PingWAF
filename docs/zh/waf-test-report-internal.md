# WAF 测试报告——内部说明（0.32.4）

[测试效果报告](waf-test-report.md) 的工程配套文档。对外报告只陈述结果；本文件保留维护者解读与复现所需的分母、逐样本归因、回归证据、占用数字、已知限制与待办。

## 1. 分母与工件

全量语料：恶意 658、正常 33 219、合计 33 877。

Strict + 请求体检测档的开放漏报：41 条（658 − 617）。

| 分层 | n | 为什么反代侧 WAF 无法判决 |
|---|---|---|
| 无 `Content-Length` / `Transfer-Encoding` 的 `POST`/`PUT` | 18 / 共 19 | RFC 7230 下请求体为空，载荷到不了服务器。19 条中 1 条仅凭头部即被拦 |
| URL fragment 载荷 | 1 | `pingora` 按 RFC 9112 §3.2 在构造请求行前剥离 fragment |
| `protocol_reject`（400/414/431/501） | 2 | HTTP 层拒绝，WAF 未见该请求 |
| `status_405` | 1 | 方法层拒绝，WAF 未见该请求 |
| 引擎真实漏报 | 19 | 全部位于 `other`（编码/混淆）家族 |

三个派生分母：

| 口径 | 分母 | 拦截 | 比率 |
|---|---|---|---|
| 全量 | 658 | 617 | 93.8% |
| 剔除字节级工件（19 无 body + 1 fragment） | 638 | 616 | **96.6%** |
| 再剔除判决级工件（+3） | 635 | 616 | 97.0% |

对外报告以 96.6% 为主指标。那条仅凭头部被拦的无 body 样本同时从分子中剔除，因此 638 − 616 = 22 而不是 21。

## 2. 漏报（19 条，Strict + body）

19 条全部属于编码/混淆的 `other` 家族：多层 base64、JSON 二次编码、overlong UTF-8 与混合转义。原始 658 分母下的 sqli/rce/xss/lfi 漏报全部来自无 body 的 POST 样本，属传输面而非检测面。逐样本归因保留在
[waf-benchmark-report.md](../waf-benchmark-report.md) §15-23（P6 → P13 轮次）。

## 3. 正常样本拦截（19 条，Strict + body）

| 触发点 | n | 评估 |
|---|---|---|
| `PINGWAF-1040` wp-admin 探测（质询） | 2 | 是质询而非拦截：合法 WordPress 访客走一次 cookie 往返即可通过。CMS 专用规则应带栈条件（`stacks=["php"]`），而不是在所有站点触发 |
| `XSS-007` query 源 script URI | 1 | 有意保留：反射面的裸 `javascript:` 维持 critical |
| `SQL-001` + libinjection union-select | 1 | 词法歧义（散文引用了完整的 `union select … from …`） |
| body 档家族（rce 4、xss 3、lfi 3、other 3、ssti 1、crlf 1） | 15 | 反射型 XSS 散文、HTML 上传、携带 `/bin/sh` 的 CI 载荷——Strict 档成员级检测的结构性代价 |

Normal 档的 4 条是 `PINGWAF-1040` ×2（质询）、`XSS-007` ×1、`SQL-001` ×1。规则级归因来自
`pingwaf-waf/examples/_fp_triage.rs`（打印 `matched_rules` 与 `details`）。

## 4. 回归与确定性

| 检查 | 结果 |
|---|---|
| `triage`（20 条 pinned 漏报） | 19 `passed` + 1 `status_405`，与全量逐条一致 |
| `fp-pin`（27 条 pinned 正常样本） | 17 `blocked` + 2 `challenged` + **8 `passed`**——已修家族保持修复 |
| smoke 套件同配置连跑两次 | 61/61 判决一致 |
| `spot-advanced` 对比 Strict + body（全部 33 877 条） | 0 gained、0 lost、0 翻转 |
| 修复后首版构建 与 最终构建，四档对比 | 0 判决差异 |

`suites.py pin` 只增不删：已转为放行的 8 条仍留在 `fp-pin`，用于证明下一轮没有回归。

## 5. 能力验证

| 能力 | 证据 |
|---|---|
| 托管规则 15 条定义 / Normal 装配 13 条 | 装配条数单测；`PINGWAF-1070` 在 Normal 档永不命中 |
| 请求体解压（gzip / deflate，1 MiB 上限） | `compressed` 套件：Strict + body 档 8/8 恶意压缩体被拦、2/2 良性放行；Normal + body 档 7/8（`$(curl … \| sh)` 形态属 Strict-only） |
| `br` / `zstd` 未检测 | 单测覆盖；语料 0 条 |
| 资源上限（表达式、归一化、扫描预算） | token / 深度 / 体积上限单测；预算耗尽时仍扫描头部与 URI |
| `PINGWAF-1051` 降为 Log | 195 条超长 URI 正常样本（14 条 > 2 KiB，最长 16 711 B）全部放行；14 条恶意样本中 12 条仍由其他家族拦下 |
| 分类监听（`monitor_categories=["sqli"]`） | 探针放行而 `.env`/TRACE 仍 403；smoke 中恰好 3 条 SQLi 翻为放行 |
| 栈监听（`monitor_stacks=["java"]`） | Log4Shell 探针放行；2 条 SSTI 翻转，3 条 SQLi 仍被拦 |

## 6. 性能与占用

| 档位 | p50 | p95 | error |
|---|---|---|---|
| Normal | 0 ms | 2 ms | 0% |
| Normal + body | 0 ms | 2 ms | 0% |
| Strict | 1 ms | 2 ms | 0% |
| Strict + body | 1 ms | 2 ms | 0% |

body 档不再出现早期轮次 ~3 s 的 p95：mock 上游会排空请求体，被拦的上传不会让客户端卡在 `sendall`。该指标由回放链路主导，只报不设门。

占用（`waf_scale_bench`，统计分配器包装系统分配器；只作相对值）：

| 模式 | 2 站点 | 10 站点 | 100 站点 | 每站点 |
|---|---|---|---|---|
| legacy（每站点独立引擎） | 242 940 B | 1 214 700 B | 12 147 000 B | ~121 KB |
| shared（共享基座） | 1 260 107 B | 1 262 347 B | 1 287 547 B | 280 B |

## 7. 已知限制

- 只覆盖静态 TOML 数据面。agent 形态行为（auto-block、日志上报、clearance cookie）不在范围内；无 agent 时 `log_event` 是空操作，monitor 判决不可观测。
- 无 TLS / HTTP/2 / HTTP/3 覆盖：矩阵为明文 HTTP/1.1。
- 不测吞吐。`replay.py` 是保真回放器而非压测工具；32 线程 / 0 ms 会产生 1.4% 连接超时，故取 16 线程 / 5 ms。
- 类目标签来自 `replay.py` 的启发式正则，只作统计维度，不参与判决。
- 占用为相对值：示例用统计分配器包装系统分配器，发布二进制用 mimalloc。
- 19 条真实漏报本轮只按类目统计，未逐条重跑归因。
- 第三方对比数字引自 blazehttp README，本轮未重测。

## 8. 待办

1. **Normal + body 漏一种压缩形态**（`$(curl … | sh)`）。把命令替换签名从 Strict-only 释放即可覆盖，代价是表单字段里合法的 `$(…)` 带来误报，需要带计数的灰度。
2. **`PINGWAF-1040` 仍会质询合法 WordPress 流量。** 应加栈条件，而不是在所有站点触发。
3. **Strict + body 仍有 19 条编码/混淆漏报**，与基准附录记录的同一批多层家族。
4. **官方对比数据是快照**，上游 README 更新时需同步（文档任务）。

## 9. 历史对照

| 指标 | 0.20.0 线（P13） | 0.32.4（本轮） |
|---|---|---|
| Normal 拦截 | 60.2%（396） | 60.2%（396） |
| Normal + body | 71.9%（473） | 71.9%（473） |
| Strict | 76.0%（500） | 76.0%（500） |
| Strict + body | 93.8%（617） | 93.8%（617） |
| Normal 正常样本受影响 | 5 | 4 |
| Strict + body 正常样本受影响 | 21 | 19 |
| 单元测试 | 194 | 250 |

## 10. 完整复现

```bash
git clone --depth 1 https://github.com/chaitin/blazehttp /tmp/blazehttp
export BLAZEHTTP_ROOT=/tmp/blazehttp
python3 tools/waf-bench/make_local_samples.py
cargo test --workspace --features=full
cargo build --release --bin pingwaf --features=full

for tier in normal normal-body strict strict-body; do
  ./tools/waf-bench/run_suite.sh \
    --conf tools/waf-bench/conf/level-$tier.toml \
    --suite full --group $tier --out results/0.32.4/$tier \
    --concurrency 16 --delay-ms 5
done

for tier in strict-body normal-body; do
  ./tools/waf-bench/run_suite.sh \
    --conf tools/waf-bench/conf/level-$tier.toml \
    --suite compressed --group compressed-$tier \
    --out results/0.32.4/compressed-$tier --concurrency 2 --delay-ms 10
done

python3 tools/waf-bench/diff.py \
  results/0.32.4/strict-body/run.jsonl results/0.32.4/spot-advanced/run.jsonl
python3 tools/waf-bench/suites.py pin \
  --jsonl results/0.32.4/strict-body/run.jsonl --suite triage --suite fp-pin
```
