---
title: WAF 实测报告
description: PingWAF WAF 引擎在 chaitin/blazehttp 公开语料上的实测效果——拦截率、误报率、延迟、回归与确定性。
minutes: 10
---

## 结论摘要

版本 0.32.4，测试日期 2026-11-02，数据集为公开的 [chaitin/blazehttp](https://github.com/chaitin/blazehttp)：33 877 条原始 HTTP 报文（恶意 658 / 正常 33 219）。以下所有数字均采用严格口径——只有 403 拦截页或 503 质询页才计入拦截。

| 档位 | 拦截 | blocked | challenged | 正常样本受影响 |
| --- | --- | --- | --- | --- |
| Normal | 60.2% (396/658) | 396 | 0 | 0.01% (4/33 219) |
| Normal + body | 71.9% (473/658) | 473 | 0 | 0.04% (12) |
| Strict | 76.0% (500/658) | 490 | 10 | 0.01% (4) |
| Strict + body | 93.8% (617/658) | 607 | 10 | 0.06% (19) |

拦截率与上一轮持平，变化的是正常样本受影响数。Normal 是默认档位，不检测请求体。最高档（Strict + 请求体检测）在 0.06% 误报率下拦截了公开恶意样本的 93.8%。

## 测试方法

- **静态数据面。** 引擎从 TOML 配置启动（`pingwaf -c`），后端为模拟源站，不涉及任何 Agent 行为。
- **raw socket 逐字节回放。** 33 877 条样本逐字节通过原始 HTTP/1.1 socket 回放，保留请求头顺序、编码技巧与畸形输入。
- **严格口径。** `blocked` 为 403 拦截页，`challenged` 为 503 质询页，`intercepted` 为两者之和（与历史数字可比）。只有这些 WAF 判决才计入；协议层拒绝与错误绝不计入拦截。
- **有效口径。** 有 20 条恶意样本是任何代理侧检测器都无法判定的（无请求体的 POST/PUT、协议层拒绝、1 条 URL fragment）。剔除后，推荐档的有效拦截率为 **616/638 = 96.6%**。

## 分类拦截率

| Category | n | Normal | Strict+body |
| --- | --- | --- | --- |
| sqli | 20 | 85.0% (17) | 85.0% (17) |
| xss | 30 | 46.7% (14) | 96.7% (29) |
| lfi | 25 | 80.0% (20) | 96.0% (24) |
| rce | 12 | 25.0% (3) | 83.3% (10) |
| ssrf | 13 | 7.7% (1) | 100% (13) |
| other | 538 | 61.9% (333) | 93.9% (505) |

Normal 与 Strict+body 之间的差距主要来自 payload 只存在于请求体的家族（ssrf 7.7% → 100%）以及编码/混淆变体（`other` 61.9% → 93.9%）。SQL 注入在默认档位已基本覆盖。

在压缩 payload 套件（`compressed`）上，Strict+body 的 8 条恶意样本全部被拦、2 条良性样本全部放行（2/2）；Normal+body 拦截 7/8，漏掉 gzip 压缩的 `$(curl … | sh)` payload。

## 回归与确定性

- `triage` 套件：20 条当前漏报的恶意样本被 pin 住并二次回放——判决与全量回放逐条一致：19 条放行、1 条返回 HTTP 405。
- `fp-pin` 套件：27 条当前被拦截的白样本被 pin 住并二次回放——17 条仍被拦截、2 条质询、8 条在本轮修复后转为放行。
- `spot-advanced`（advanced_mode 路径）与 Strict+body 全量 33 877 条对比：**0 差异**，逐位一致。
- 单元测试：`pingwaf-waf` **250** 个，运行时全部通过。

## 性能与限制

release 构建、loopback 环境下，静态档延迟保持在 **p95 1–2 ms**。我们刻意不宣称吞吐：回放工具是保真回放器，不是压测器。本次测试仅覆盖静态 TOML 数据面——Agent 行为（日志上报、放行 Cookie、自动升级拦截）不在范围内。也没有覆盖 TLS、HTTP/2 或 HTTP/3 流量。

## 横向对比

同一份公开语料也被用于评测其他产品。下表中第三方数字按原样引自 [blazehttp README](https://github.com/chaitin/blazehttp)（33 669 样本口径）；PingWAF 数字来自上文本次实测。

| 产品/模式 | 样本 | 恶意 | 检出率 | 正常 | 误报率 | 准确率 | 耗时 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| PingWAF Strict+body | 33 877 | 658 | 93.8% (617) | 33 219 | 0.06% (19) | 99.82% | p95 2 ms（环回） |
| PingWAF Normal | 33 877 | 658 | 60.2% (396) | 33 219 | 0.01% (4) | 99.21% | p95 2 ms（环回） |
| 雷池免费版·严格（SafeLine strict） | 33 669 | 575 | 76.17% (438) | 33 094 | 0.22% (73) | 99.38% | 64.34 ms（公网） |
| 雷池免费版·平衡（SafeLine balanced） | 33 669 | 575 | 71.65% (412) | 33 094 | 0.07% (23) | 99.45% | 70.05 ms（公网） |
| ModSecurity PL4 | 33 669 | 575 | 94.61% (544) | 33 094 | 52.46% (17 362) | 48.34% | 28.89 ms（公网） |
| ModSecurity PL1 | 33 669 | 575 | 69.74% (401) | 33 094 | 17.58% (5 819) | 82.20% | 31.15 ms（公网） |
| Cloudflare 免费版 | 33 669 | 570 | 10.70% (61) | 32 780 | 0.07% (23) | 98.40% | 288.96 ms（公网） |

读表口径说明：

1. 样本快照不同（官方 33 669 条 / 恶意 570–575；PingWAF 33 877 条 / 恶意 658），两组检出率并非同一分母；
2. 耗时测量方式不同：公网数字含 RTT，PingWAF 数字为环回判决时延，均不可当作吞吐对比；
3. 第三方数字按原样引用，本轮未重测；本表只描述各引擎在同一公开语料下的相对位置。

## 复现与完整报告

含规则级误报归因与工件分析的完整报告在仓库中：

- [完整测试报告（中文）](https://github.com/shuaiZend/PingWAF/blob/main/docs/zh/waf-test-report.md)
- [Full test report (EN)](https://github.com/shuaiZend/PingWAF/blob/main/docs/waf-test-report.md)
- [测试方案与口径定义](https://github.com/shuaiZend/PingWAF/blob/main/docs/waf-test-plan.md)

方法与口径定义见[测试方案](https://github.com/shuaiZend/PingWAF/blob/main/docs/zh/waf-test-plan.md)。
