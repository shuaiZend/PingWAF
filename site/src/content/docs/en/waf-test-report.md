---
title: WAF test report
description: Measured results of the PingWAF WAF engine on the chaitin/blazehttp public corpus — interception, false positives, accuracy and latency.
minutes: 8
---

## Results at a glance

Version 0.32.4, tested on 2026-11-02 against the public [chaitin/blazehttp](https://github.com/chaitin/blazehttp) corpus: 33 877 raw HTTP messages (658 malicious, 33 219 benign).

| Metric | Result |
| --- | --- |
| Effective-calibre interception | 96.6% (616/638) |
| Full-calibre interception | 93.8% (617/658) |
| Benign traffic affected | 0.06% (19/33 219) |
| Accuracy | 99.82% |
| Static-tier p95 | ≤ 2 ms |

Which number should you use? The 96.6% denominator is 638: 20 of the 658 malicious samples are excluded because no proxy-side WAF can decide them — 19 POST/PUT requests with no `Content-Length` / `Transfer-Encoding` (an empty request body under RFC 7230) and 1 sample whose payload hides in a URL fragment, which the HTTP parser strips before building the request line. The full-calibre figure (93.8%) is kept for reference.

## Comparison with other products

| Product / mode | Samples | Malicious | Detection rate | Benign | False positives | Accuracy | Time |
| --- | --- | --- | --- | --- | --- | --- | --- |
| PingWAF Strict+body | 33 877 | 658 | 93.8% (617) | 33 219 | 0.06% (19) | 99.82% | p95 2 ms (loopback) |
| PingWAF Normal | 33 877 | 658 | 60.2% (396) | 33 219 | 0.01% (4) | 99.21% | p95 2 ms (loopback) |
| SafeLine free, strict | 33 669 | 575 | 76.17% (438) | 33 094 | 0.22% (73) | 99.38% | 64.34 ms (public) |
| SafeLine free, balanced | 33 669 | 575 | 71.65% (412) | 33 094 | 0.07% (23) | 99.45% | 70.05 ms (public) |
| ModSecurity PL4 | 33 669 | 575 | 94.61% (544) | 33 094 | 52.46% (17 362) | 48.34% | 28.89 ms (public) |
| ModSecurity PL1 | 33 669 | 575 | 69.74% (401) | 33 094 | 17.58% (5 819) | 82.20% | 31.15 ms (public) |
| Cloudflare free | 33 669 | 570 | 10.70% (61) | 32 780 | 0.07% (23) | 98.40% | 288.96 ms (public) |

The PingWAF rows are self-measured (effective calibre 96.6%, see the section above); the remaining figures are quoted as-is from the [blazehttp README](https://github.com/chaitin/blazehttp) and were not re-tested. The sample sets differ (33 877 vs 33 669 samples; 658 vs 570–575 malicious), and third-party timings are public-network round trips while PingWAF figures are loopback verdict latency — compare detection and false positives, not milliseconds.

## Detection by category

| Category | n | Strict+body | Normal |
| --- | --- | --- | --- |
| SQL injection | 20 | 85.0% | 85.0% |
| XSS | 30 | 96.7% | 46.7% |
| Path traversal | 25 | 96.0% | 80.0% |
| Command injection | 12 | 83.3% | 25.0% |
| SSRF | 13 | 100% | 7.7% |
| Deserialization | 6 | 100% | 16.7% |
| CRLF injection | 5 | 100% | 60.0% |
| SSTI | 5 | 100% | 60.0% |
| XXE | 2 | 50.0% | 0% |
| Log4Shell | 1 | 100% | 100% |
| Scanners | 1 | 100% | 0% |
| Encoding & obfuscation | 538 | 93.9% | 61.9% |

## What changed in this round

1. Chinese search and telemetry values containing `and 1=1` are no longer judged as SQLi (7 samples), and `.github` paths are no longer treated as dotfile probes (1 sample) — the four tier counts are unchanged (396/473/500/617) while benign samples affected drop from 12/20/12/27 to 4/12/4/19.
2. End-to-end coverage of compressed attack bodies: all 8 malicious gzip payloads blocked, both benign ones passed (2/2).
3. Challenges (503) and blocks (403) are now counted separately.

| Tier | Interception | blocked | challenged | Benign affected | p95 |
| --- | --- | --- | --- | --- | --- |
| Normal | 60.2% (396/658) | 396 | 0 | 0.01% (4) | 2 ms |
| Normal+body | 71.9% (473) | 473 | 0 | 0.04% (12) | 2 ms |
| Strict | 76.0% (500) | 490 | 10 | 0.01% (4) | 2 ms |
| Strict+body | 93.8% (617) | 607 | 10 | 0.06% (19) | 2 ms |

## Where the numbers come from

A release build of the static TOML data plane (`pingwaf -c` with a mock origin); the 33 877 raw messages are replayed byte for byte over a raw HTTP/1.1 socket on loopback, 16 concurrent connections with 5 ms spacing; only the 403 block page and the 503 challenge page count as interceptions — protocol-layer and method-level rejections never count; 250 engine unit tests pass; and every figure is reproducible with the commands below.

## Reproducing

```bash
git clone --depth 1 https://github.com/chaitin/blazehttp /tmp/blazehttp
export BLAZEHTTP_ROOT=/tmp/blazehttp
cargo build --release --bin pingwaf --features=full
./tools/waf-bench/run_suite.sh --conf tools/waf-bench/conf/level-strict-body.toml --suite full --group strict-body --out results/0.32.4/strict-body --concurrency 16 --delay-ms 5
```
