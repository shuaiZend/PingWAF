# WAF Test Report — Internal Notes (0.32.4)

Engineering companion to [waf-test-report.md](./waf-test-report.md). The public
report states results only; this file keeps the denominators, per-sample
attribution, regression evidence, footprint numbers, known limits and open work
that a maintainer needs to interpret or reproduce them.

## 1. Denominators and artefacts

Full corpus: 658 malicious, 33 219 benign, 33 877 total.

Open misses at Strict + body: 41 samples (658 − 617).

| Bucket | n | Why it cannot be decided by a proxy-side WAF |
|---|---|---|
| `POST`/`PUT` without `Content-Length` or `Transfer-Encoding` | 18 of 19 | Empty body under RFC 7230 — the payload never reaches the server. One of the 19 is blocked on its headers alone |
| URL fragment payload | 1 | `pingora` strips the fragment per RFC 9112 §3.2 before the request line is built |
| `protocol_reject` (400/414/431/501) | 2 | The HTTP layer refuses the request; the WAF never sees it |
| `status_405` | 1 | The method layer refuses; the WAF never sees it |
| Real engine misses | 19 | All in the `other` (encoded/obfuscated) family |

Two derived denominators:

| Calibre | Denominator | Intercepted | Rate |
|---|---|---|---|
| Full corpus | 658 | 617 | 93.8 % |
| Byte-level artefacts excluded (19 body-less + 1 fragment) | 638 | 616 | **96.6 %** |
| All artefacts excluded (+3 verdict-layer) | 635 | 616 | 97.0 % |

The public report leads with 96.6 %. The one body-less sample that *is* blocked
(headers alone) is excluded from the numerator as well, which is why 638 − 616
= 22 rather than 21.

## 2. Missed attacks (19, Strict + body)

All 19 sit in the encoded/obfuscated `other` family — multi-layer base64, JSON
re-encoding, overlong UTF-8 and mixed escaping. The raw 658 denominator's
sqli/rce/xss/lfi misses all come from body-less POST samples, i.e. from the
transport, not from detection. Per-sample attribution stays in
[waf-benchmark-report.md](./waf-benchmark-report.md) §15-23 (P6 → P13 rounds).

## 3. Benign interceptions (19, Strict + body)

| Trigger | n | Assessment |
|---|---|---|
| `PINGWAF-1040` wp-admin probe (challenge) | 2 | Challenge, not a block — a legitimate WordPress visitor passes after the cookie round trip. The CMS-specific rule should carry a stack condition (`stacks=["php"]`) rather than firing on every site |
| `XSS-007` script URI in query | 1 | Intentional: a bare `javascript:` on a reflected surface stays critical |
| `SQL-001` + libinjection union-select | 1 | Word-level ambiguity (prose quoting a full `union select … from …`) |
| Body-tier families (rce 4, xss 3, lfi 3, other 3, ssti 1, crlf 1) | 15 | Reflected-XSS prose, HTML uploads, CI payloads carrying `/bin/sh` — the structural cost of member-level inspection at the Strict tier |

The Normal tier's 4 are `PINGWAF-1040` ×2 (challenged), `XSS-007` ×1 and
`SQL-001` ×1. Per-rule attribution came from
`pingwaf-waf/examples/_fp_triage.rs`, which prints `matched_rules` and `details`.

## 4. Regression and determinism

| Check | Result |
|---|---|
| `triage` (20 pinned misses) | 19 `passed` + 1 `status_405`, identical to the full run |
| `fp-pin` (27 pinned benign samples) | 17 `blocked` + 2 `challenged` + **8 `passed`** — the fixed families stay fixed |
| Smoke suite twice, same configuration | 61/61 identical verdicts |
| `spot-advanced` vs Strict + body, all 33 877 samples | 0 gained, 0 lost, 0 flips |
| First post-fix build vs final build, four tiers | 0 verdict differences |

`suites.py pin` only adds: the 8 samples that now pass remain in `fp-pin` on
purpose, so the next round proves they did not regress.

## 5. Capability checks

| Capability | Evidence |
|---|---|
| Managed rules 15 defined / 13 assembled on Normal | assembly-count unit tests; `PINGWAF-1070` never fires on Normal |
| Body decompression (gzip / deflate, 1 MiB cap) | `compressed` suite: 8/8 malicious gzip bodies intercepted, 2/2 benign passed at Strict + body; Normal + body is 7/8 (the `$(curl … \| sh)` form is Strict-only) |
| `br` / `zstd` uninspected | unit tests; corpus has 0 such samples |
| Resource ceilings (expression, normalisation, scan budget) | token/depth/size unit tests; budget exhaustion keeps scanning headers and URI |
| `PINGWAF-1051` demoted to Log | 195 long-URI benign samples (14 > 2 KiB, up to 16 711 B) all pass; 12 of 14 malicious ones are still caught by other families |
| Category monitor (`monitor_categories=["sqli"]`) | probe passes while `.env`/TRACE stay blocked; exactly the 3 SQLi smoke samples flip to `passed` |
| Stack monitor (`monitor_stacks=["java"]`) | Log4Shell probe passes; the 2 SSTI samples flip, the 3 SQLi samples stay intercepted |

## 6. Performance and footprint

| Tier | p50 | p95 | error |
|---|---|---|---|
| Normal | 0 ms | 2 ms | 0 % |
| Normal + body | 0 ms | 2 ms | 0 % |
| Strict | 1 ms | 2 ms | 0 % |
| Strict + body | 1 ms | 2 ms | 0 % |

Body tiers no longer show the ~3 s p95 of earlier rounds: the mock origin drains
request bodies, so a blocked upload does not leave the client mid-`sendall`. The
metric is harness-dominated and is reported, not gated.

Footprint (`waf_scale_bench`, tracking allocator over the system allocator;
relative values only):

| Mode | 2 sites | 10 sites | 100 sites | Per site |
|---|---|---|---|---|
| legacy (per-site engine) | 242 940 B | 1 214 700 B | 12 147 000 B | ~121 KB |
| shared base | 1 260 107 B | 1 262 347 B | 1 287 547 B | 280 B |

## 7. Known limits

- Static TOML data plane only. Agent-mode behaviour (auto-block, log shipping,
  clearance cookies) is out of scope; without an agent `log_event` is a no-op,
  so a monitor verdict is not observable.
- No TLS / HTTP/2 / HTTP-3 coverage: the matrix is plaintext HTTP/1.1.
- Throughput is not measured. `replay.py` is a fidelity replayer, not a load
  generator; 32 threads with 0 ms pacing produced 1.4 % connect timeouts, hence
  16 threads / 5 ms.
- Category labels are a heuristic regex in `replay.py`; they are a reporting
  dimension, not a verdict.
- Footprint numbers are relative: the scale example uses a tracking allocator
  over the system allocator, the shipped binary uses mimalloc.
- The 19 real misses were classified by category only; per-sample attribution
  is historical, not re-run this round.
- Third-party comparison figures are quoted from the blazehttp README and were
  not re-measured here.

## 8. Open work

1. **Normal + body misses one compressed form** (`$(curl … | sh)`). Releasing the
   command-substitution signature from Strict-only would close it; the cost is
   false positives from legitimate `$(…)` in form fields, so it needs a counted
   roll-out.
2. **`PINGWAF-1040` still challenges legitimate WordPress traffic.** Add a stack
   condition instead of firing on every site.
3. **19 encoded/obfuscated misses remain** at Strict + body — the multi-layer
   families tracked in the benchmark appendix.
4. **Official comparison data is a snapshot** of the upstream README; refreshing
   it is a documentation task.

## 9. History

| Metric | 0.20.0 line (P13) | 0.32.4 (this round) |
|---|---|---|
| Normal interception | 60.2 % (396) | 60.2 % (396) |
| Normal + body | 71.9 % (473) | 71.9 % (473) |
| Strict | 76.0 % (500) | 76.0 % (500) |
| Strict + body | 93.8 % (617) | 93.8 % (617) |
| Normal benign interceptions | 5 | 4 |
| Strict + body benign interceptions | 21 | 19 |
| Unit tests | 194 | 250 |

## 10. Full reproduction

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
