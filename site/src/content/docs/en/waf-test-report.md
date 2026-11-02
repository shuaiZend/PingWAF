---
title: WAF test report
description: Measured effectiveness of the PingWAF WAF engine on the chaitin/blazehttp corpus — interception, false positives, latency, regression and determinism.
minutes: 10
---

## Verdict summary

Version 0.32.4, tested on 2026-11-02 against the public [chaitin/blazehttp](https://github.com/chaitin/blazehttp) dataset: 33 877 raw HTTP messages (658 malicious, 33 219 benign). Every figure below uses the strict calibre — only a 403 block page or a 503 challenge page counts as an interception.

| Tier | Interception | blocked | challenged | Benign affected |
| --- | --- | --- | --- | --- |
| Normal | 60.2% (396/658) | 396 | 0 | 0.01% (4/33 219) |
| Normal + body | 71.9% (473/658) | 473 | 0 | 0.04% (12) |
| Strict | 76.0% (500/658) | 490 | 10 | 0.01% (4) |
| Strict + body | 93.8% (617/658) | 607 | 10 | 0.06% (19) |

Interception rates are unchanged from the previous round; the benign-affected counts moved with this round's fixes. Normal is the default tier and does not inspect request bodies. The top tier (Strict + body inspection) blocks 93.8% of the public malicious corpus at a 0.06% false-positive rate.

## Test method

- **Static data plane.** The engine runs from a TOML configuration (`pingwaf -c`) with a mock origin, so no agent behaviour is involved.
- **Raw-socket replay.** Each of the 33 877 samples is replayed byte for byte through a raw HTTP/1.1 socket, preserving header order, encoding tricks and malformed input.
- **Strict calibre.** `blocked` is an HTTP 403 block page, `challenged` is an HTTP 503 challenge page, and `intercepted` is their sum (comparable with historical figures). Only these WAF verdicts count; protocol-layer rejections and errors are never counted as interceptions.
- **Effective calibre.** 20 malicious samples cannot be decided by any proxy-side inspector (body-less POST/PUT, protocol rejections, one URL fragment). Excluding them, the effective figure for the recommended tier is **616/638 = 96.6%**.

## Interception by category

| Category | n | Normal | Strict+body |
| --- | --- | --- | --- |
| sqli | 20 | 85.0% (17) | 85.0% (17) |
| xss | 30 | 46.7% (14) | 96.7% (29) |
| lfi | 25 | 80.0% (20) | 96.0% (24) |
| rce | 12 | 25.0% (3) | 83.3% (10) |
| ssrf | 13 | 7.7% (1) | 100% (13) |
| other | 538 | 61.9% (333) | 93.9% (505) |

The gap between Normal and Strict+body is carried by families whose payload lives in the request body (ssrf 7.7% → 100%) and by encoded or obfuscated variants (`other` 61.9% → 93.9%). SQL injection is already caught almost entirely at the default tier.

On the compressed-payload suite (`compressed`), Strict+body intercepts all 8 malicious samples and passes both benign ones (2/2); Normal+body intercepts 7/8, missing the gzip-compressed `$(curl … | sh)` payload.

## Regression and determinism

- `triage` suite: 20 currently-missed malicious samples pinned and replayed — verdicts identical to the full run, sample by sample: 19 passed, 1 answered with HTTP 405.
- `fp-pin` suite: 27 currently-blocked benign samples pinned and replayed — 17 still blocked, 2 challenged, and 8 now pass (the false positives removed in this round).
- `spot-advanced` (advanced_mode path) vs Strict+body across all 33 877 samples: **0 differences**, bit for bit.
- Unit tests: **250** in `pingwaf-waf`, all green at the time of the run.

## Performance and limitations

Static-tier latency stays at **p95 1–2 ms** on loopback with a release build. Throughput is deliberately not claimed: the replay harness is a fidelity replayer, not a load generator. The test covers the static TOML data plane only — agent behaviour (log shipping, clearance cookies, auto-block escalation) is out of scope. No TLS, HTTP/2 or HTTP/3 traffic is exercised.

## Comparison with other WAFs

The same public corpus has been used to benchmark other products as well. The third-party figures below are quoted as-is from the [blazehttp README](https://github.com/chaitin/blazehttp) (33 669-sample calibre); the PingWAF figures come from the run described above.

| Product / mode | Samples | Malicious | Detection rate | Benign | False positives | Accuracy | Time |
| --- | --- | --- | --- | --- | --- | --- | --- |
| PingWAF Strict+body | 33 877 | 658 | 93.8% (617) | 33 219 | 0.06% (19) | 99.82% | p95 2 ms (loopback) |
| PingWAF Normal | 33 877 | 658 | 60.2% (396) | 33 219 | 0.01% (4) | 99.21% | p95 2 ms (loopback) |
| SafeLine free, strict | 33 669 | 575 | 76.17% (438) | 33 094 | 0.22% (73) | 99.38% | 64.34 ms (public) |
| SafeLine free, balanced | 33 669 | 575 | 71.65% (412) | 33 094 | 0.07% (23) | 99.45% | 70.05 ms (public) |
| ModSecurity PL4 | 33 669 | 575 | 94.61% (544) | 33 094 | 52.46% (17 362) | 48.34% | 28.89 ms (public) |
| ModSecurity PL1 | 33 669 | 575 | 69.74% (401) | 33 094 | 17.58% (5 819) | 82.20% | 31.15 ms (public) |
| Cloudflare free | 33 669 | 570 | 10.70% (61) | 32 780 | 0.07% (23) | 98.40% | 288.96 ms (public) |

How to read this table:

1. The sample snapshots differ (official: 33 669 samples with 570–575 malicious; PingWAF: 33 877 with 658 malicious), so the detection rates do not share the same denominator.
2. Latency is measured differently: public-network figures include RTT, while PingWAF figures are loopback verdict latency — neither is a throughput comparison.
3. Third-party numbers are quoted as-is and were not re-tested in this round; the table only describes the relative position of each engine on the same public corpus.

## Reproducing and the full report

The complete report, with rule-level false-positive attribution and the artefact analysis, lives in the repository:

- [Full test report (EN)](https://github.com/shuaiZend/PingWAF/blob/main/docs/waf-test-report.md)
- [完整测试报告（中文）](https://github.com/shuaiZend/PingWAF/blob/main/docs/zh/waf-test-report.md)
- [Test plan and calibres](https://github.com/shuaiZend/PingWAF/blob/main/docs/waf-test-plan.md)

The full methodology and calibre definitions are in the
[test plan](https://github.com/shuaiZend/PingWAF/blob/main/docs/waf-test-plan.md).
