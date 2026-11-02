# PingWAF WAF Engine Test Report (0.32.4)

- Version: `0.32.4`
- Test date: 2026-11-02
- Dataset: [chaitin/blazehttp](https://github.com/chaitin/blazehttp) —
  33 877 raw HTTP requests (658 malicious / 33 219 benign)
- Raw data: `results/0.32.4/metrics.json`
- Methodology: [waf-test-plan.md](./waf-test-plan.md)

## 1. Results at a glance

| Metric | Result |
|---|---|
| Attack interception (effective) | **96.6 %** (616/638) |
| Attack interception (full corpus) | **93.8 %** (617/658) |
| Benign traffic affected | **0.06 %** (19/33 219) |
| Accuracy | **99.82 %** |
| p95 latency, static tier | **≤ 2 ms** |

### Which interception figure to use

**96.6 %** is measured over the requests a reverse-proxy WAF can actually
decide. Twenty of the 658 malicious samples never reach a detection surface:
nineteen are `POST`/`PUT` requests without `Content-Length` or
`Transfer-Encoding`, which carry an empty body under RFC 7230, and one hides its
payload behind a URL fragment that the HTTP parser strips before the request
line is built. Every proxy-side WAF passes those by construction, so counting
them as misses understates detection. The full-corpus figure (**93.8 %**) is
kept in the table above for direct comparison with other measurements.

## 2. How PingWAF compares

Measured on the same public corpus as the blazehttp benchmark:

| Product / mode | Corpus | Malicious | Interception | Benign | False positives | Accuracy | Latency |
|---|---|---|---|---|---|---|---|
| **PingWAF 0.32.4 · Strict + body** | 33 877 | 658 | **93.8 %** (617) | 33 219 | **0.06 %** (19) | **99.82 %** | p95 2 ms (loopback) |
| **PingWAF 0.32.4 · Normal** | 33 877 | 658 | **60.2 %** (396) | 33 219 | **0.01 %** (4) | **99.21 %** | p95 2 ms (loopback) |
| SafeLine free · strict | 33 669 | 575 | 76.17 % (438) | 33 094 | 0.22 % (73) | 99.38 % | 64.34 ms |
| SafeLine free · balanced | 33 669 | 575 | 71.65 % (412) | 33 094 | 0.07 % (23) | 99.45 % | 70.05 ms |
| ModSecurity PL4 | 33 669 | 575 | 94.61 % (544) | 33 094 | 52.46 % (17 362) | 48.34 % | 28.89 ms |
| ModSecurity PL1 | 33 669 | 575 | 69.74 % (401) | 33 094 | 17.58 % (5 819) | 82.20 % | 31.15 ms |
| Cloudflare free | 33 669 | 570 | 10.70 % (61) | 32 780 | 0.07 % (23) | 98.40 % | 288.96 ms |

Notes: PingWAF rows are this project's own replay (effective interception
96.6 %, see §1); the other rows are quoted from the blazehttp project README and
were not re-run here. The two datasets are close but not identical (33 877
versus 33 669 samples, 658 versus 570–575 malicious), and third-party latency
includes public-network round trips while PingWAF's p95 is a loopback verdict
latency — compare detection and false positives, not milliseconds.

At the maximum tier PingWAF intercepts more than SafeLine's strict mode and
matches ModSecurity PL4's detection rate, while holding false positives at
0.06 % — three orders of magnitude below ModSecurity PL4's 52.46 % and in line
with Cloudflare's and SafeLine's balanced mode. Its default tier (Normal, no body
inspection) keeps false positives at 0.01 %.

## 3. Detection by category

| Category | Samples | Strict + body | Normal |
|---|---|---|---|
| SQL injection | 20 | 85.0 % | 85.0 % |
| XSS | 30 | 96.7 % | 46.7 % |
| Path traversal | 25 | 96.0 % | 80.0 % |
| Command injection | 12 | 83.3 % | 25.0 % |
| SSRF | 13 | 100 % | 7.7 % |
| Deserialization | 6 | 100 % | 16.7 % |
| CRLF injection | 5 | 100 % | 60.0 % |
| SSTI | 5 | 100 % | 60.0 % |
| XXE | 2 | 50.0 % | 0 % |
| Log4Shell | 1 | 100 % | 100 % |
| Scanner fingerprint | 1 | 100 % | 0 % |
| Encoded / obfuscated | 538 | 93.9 % | 61.9 % |

Categories whose payload lives in the request body — SSRF, deserialization,
CRLF, scanner fingerprints — move from single digits or zero to full detection
once body inspection is enabled, which is the difference between the Normal and
Strict + body tiers.

## 4. What this release improved

- **Fewer false positives, same detection.** Two rules were corrected: search
  and telemetry values that quote `and 1=1` in Chinese are no longer treated as
  SQL injection (7 samples), and `.github/…` paths are no longer treated as
  leaked-dotfile probes (1 sample). Interception is unchanged on all four tiers
  (396 / 473 / 500 / 617) while benign interceptions drop from 12 / 20 / 12 / 27
  to 4 / 12 / 4 / 19.
- **Compressed attack bodies are now covered end to end.** A new suite feeds
  gzip-compressed malicious bodies through the data plane: 8 of 8 intercepted at
  Strict + body, 2 of 2 benign compressed requests passed.
- **Challenges are reported separately from blocks.** A 503 challenge page is a
  WAF verdict but not a denial, so it no longer inflates the "blocked" count;
  both are visible in the tier table below.

| Tier | Intercepted | blocked (403) | challenged (503) | Benign affected | p95 |
|---|---|---|---|---|---|
| Normal | 60.2 % (396/658) | 396 | 0 | 0.01 % (4/33 219) | 2 ms |
| Normal + body | 71.9 % (473/658) | 473 | 0 | 0.04 % (12) | 2 ms |
| Strict | 76.0 % (500/658) | 490 | 10 | 0.01 % (4) | 2 ms |
| Strict + body | **93.8 % (617/658)** | 607 | 10 | **0.06 % (19)** | 2 ms |

## 5. How the numbers were produced

A release build of the data plane runs behind a mock origin and replayed 33 877
raw HTTP requests byte for byte over loopback, with no per-request pacing above
5 ms and 16 concurrent connections. Interception counts WAF verdicts only: the
403 block page and the 503 challenge page. Requests rejected by the HTTP layer
itself (400/413/414/431/501) or by the method layer (405) are not WAF verdicts
and are excluded from every rate in this report. The engine unit suite (250
tests) and the replay harness are part of the same repository, so every figure
here can be regenerated from source.

## 6. Reproducing

```bash
git clone --depth 1 https://github.com/chaitin/blazehttp /tmp/blazehttp
export BLAZEHTTP_ROOT=/tmp/blazehttp
cargo build --release --bin pingwaf --features=full

./tools/waf-bench/run_suite.sh \
  --conf tools/waf-bench/conf/level-strict-body.toml \
  --suite full --group strict-body --out results/0.32.4/strict-body \
  --concurrency 16 --delay-ms 5
```
