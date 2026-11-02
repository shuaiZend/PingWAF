---
title: WAF 实测报告
description: PingWAF WAF 引擎在 chaitin/blazehttp 公开语料上的实测结果——拦截率、误报率、准确率与延迟。
minutes: 8
---

## 结果速览

版本 0.32.4，测试日期 2026-11-02，语料为公开的 [chaitin/blazehttp](https://github.com/chaitin/blazehttp)：33 877 条原始 HTTP 报文（恶意 658 / 正常 33 219）。

| 指标 | 结果 |
| --- | --- |
| 有效口径拦截率 | 96.6%（616/638） |
| 全量口径拦截率 | 93.8%（617/658） |
| 正常流量受影响 | 0.06%（19/33 219） |
| 准确率 | 99.82% |
| 静态档 p95 | ≤ 2 ms |

该用哪个数字？96.6% 的分母是 638：从 658 条恶意样本中剔除了 20 条任何反代侧 WAF 都无法判决的样本——19 条无 `Content-Length` / `Transfer-Encoding` 的 POST/PUT（按 RFC 7230，请求体为空）、1 条载荷藏在 URL fragment 里（HTTP 解析器在构造请求行之前已剥离 fragment）。全量口径（93.8%）保留供对照。

## 与其他产品的对比

| 产品/模式 | 样本 | 恶意 | 检出率 | 正常 | 误报率 | 准确率 | 耗时 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| PingWAF Strict+body | 33 877 | 658 | 93.8%（617） | 33 219 | 0.06%（19） | 99.82% | p95 2 ms（环回） |
| PingWAF Normal | 33 877 | 658 | 60.2%（396） | 33 219 | 0.01%（4） | 99.21% | p95 2 ms（环回） |
| 雷池免费版·严格（SafeLine strict） | 33 669 | 575 | 76.17%（438） | 33 094 | 0.22%（73） | 99.38% | 64.34 ms（公网） |
| 雷池免费版·平衡（SafeLine balanced） | 33 669 | 575 | 71.65%（412） | 33 094 | 0.07%（23） | 99.45% | 70.05 ms（公网） |
| ModSecurity PL4 | 33 669 | 575 | 94.61%（544） | 33 094 | 52.46%（17 362） | 48.34% | 28.89 ms（公网） |
| ModSecurity PL1 | 33 669 | 575 | 69.74%（401） | 33 094 | 17.58%（5 819） | 82.20% | 31.15 ms（公网） |
| Cloudflare 免费版 | 33 669 | 570 | 10.70%（61） | 32 780 | 0.07%（23） | 98.40% | 288.96 ms（公网） |

PingWAF 两行为自测（有效口径 96.6%，见上节）；其余数字按原样引自 [blazehttp README](https://github.com/chaitin/blazehttp)，未重测。样本口径不同（33 877 vs 33 669 条，恶意 658 vs 570–575），且第三方耗时为公网往返、PingWAF 为环回判决时延——请比较检出与误报，不要比较毫秒。

## 分类检出率

| 类目 | 样本数 | Strict+body | Normal |
| --- | --- | --- | --- |
| SQL 注入 | 20 | 85.0% | 85.0% |
| XSS | 30 | 96.7% | 46.7% |
| 路径穿越 | 25 | 96.0% | 80.0% |
| 命令注入 | 12 | 83.3% | 25.0% |
| SSRF | 13 | 100% | 7.7% |
| 反序列化 | 6 | 100% | 16.7% |
| CRLF 注入 | 5 | 100% | 60.0% |
| SSTI | 5 | 100% | 60.0% |
| XXE | 2 | 50.0% | 0% |
| Log4Shell | 1 | 100% | 100% |
| 扫描器 | 1 | 100% | 0% |
| 编码混淆 | 538 | 93.9% | 61.9% |

## 本版本改进

1. 中文 `and 1=1` 搜索/埋点取值不再判为 SQLi（7 条），`.github` 路径不再当作 dotfile 探测（1 条）——四档检出数不变（396/473/500/617），正常样本受影响从 12/20/12/27 降到 4/12/4/19。
2. 压缩攻击体端到端覆盖：8/8 恶意 gzip 被拦、2/2 良性放行。
3. 质询（503）与拦截（403）分列统计。

| 档位 | 拦截合计 | blocked | challenged | 正常受影响 | p95 |
| --- | --- | --- | --- | --- | --- |
| Normal | 60.2%（396/658） | 396 | 0 | 0.01%（4） | 2 ms |
| Normal+body | 71.9%（473） | 473 | 0 | 0.04%（12） | 2 ms |
| Strict | 76.0%（500） | 490 | 10 | 0.01%（4） | 2 ms |
| Strict+body | 93.8%（617） | 607 | 10 | 0.06%（19） | 2 ms |

## 数字怎么来的

release 构建、静态 TOML 数据面（`pingwaf -c`，后端为模拟源站）；33 877 条原始报文通过 raw HTTP/1.1 socket 逐字节环回回放，16 并发、5 ms 间隔；拦截只计 403 拦截页与 503 质询页，HTTP 协议层与方法层拒绝不计入；250 个引擎单元测试通过；所有数字均可按下一节的命令复现。

## 复现

```bash
git clone --depth 1 https://github.com/chaitin/blazehttp /tmp/blazehttp
export BLAZEHTTP_ROOT=/tmp/blazehttp
cargo build --release --bin pingwaf --features=full
./tools/waf-bench/run_suite.sh --conf tools/waf-bench/conf/level-strict-body.toml --suite full --group strict-body --out results/0.32.4/strict-body --concurrency 16 --delay-ms 5
```
