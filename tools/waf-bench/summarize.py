#!/usr/bin/env python3
"""Aggregate waf-bench replay JSONL results into a markdown summary.

Usage: summarize.py results/g1.jsonl results/g2.jsonl ... > summary.md

Verdict buckets come straight from replay.py. Three reporting calibres:

  blocked       — HTTP 403 (the block page): the request was refused
  challenged    — HTTP 503 (the challenge page): a WAF verdict that costs the
                  visitor a cookie round trip instead of a denial
  intercepted   — blocked + challenged, the WAF verdict surface used for the
                  headline interception rate (comparable with older reports
                  where both statuses shared one bucket)
  wide          — intercepted + `protocol_reject` + `error`
                  (calibre used when reconciling against the official
                  blazehttp binary, which cannot distinguish proxy-layer
                  rejections)
"""

import json
import sys
from collections import defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from replay import INTERCEPTED  # noqa: E402  (one calibre definition, shared)

VERDICTS = ["blocked", "challenged", "passed", "protocol_reject", "rate_limited", "error"]


def load(paths):
    groups = defaultdict(list)
    for path in paths:
        with open(path) as handle:
            for line in handle:
                line = line.strip()
                if line:
                    record = json.loads(line)
                    groups[record["group"]].append(record)
    return groups


def percentile(values, pct):
    if not values:
        return 0
    ordered = sorted(values)
    idx = min(len(ordered) - 1, int(len(ordered) * pct / 100))
    return ordered[idx]


def wide_blocked(record):
    return record["verdict"] in (*INTERCEPTED, "protocol_reject", "error") or (
        record["verdict"].startswith("status_")
        and record.get("detail", "").endswith(("reset", "timeout"))
    )


def group_summary(records):
    black = [r for r in records if r["expected"] == "black"]
    white = [r for r in records if r["expected"] == "white"]

    def count(rows, verdict):
        return sum(1 for r in rows if r["verdict"] == verdict)

    black_blocked = count(black, "blocked")
    black_challenged = count(black, "challenged")
    white_blocked = count(white, "blocked")
    white_challenged = count(white, "challenged")
    black_intercepted = black_blocked + black_challenged
    white_intercepted = white_blocked + white_challenged
    black_wide = sum(1 for r in black if wide_blocked(r))
    white_wide = sum(1 for r in white if wide_blocked(r))
    latencies = [r["latency_ms"] for r in records]
    errors = count(records, "error")
    return {
        "total": len(records),
        "black": len(black),
        "white": len(white),
        "black_blocked": black_blocked,
        "black_challenged": black_challenged,
        "black_intercepted": black_intercepted,
        "white_blocked": white_blocked,
        "white_challenged": white_challenged,
        "white_intercepted": white_intercepted,
        "black_wide": black_wide,
        "white_wide": white_wide,
        "intercept_rate": 100.0 * black_intercepted / len(black) if black else 0,
        "block_rate_strict": 100.0 * black_blocked / len(black) if black else 0,
        "challenge_rate": 100.0 * black_challenged / len(black) if black else 0,
        "fp_rate_strict": 100.0 * white_blocked / len(white) if white else 0,
        "fp_rate_intercepted": 100.0 * white_intercepted / len(white) if white else 0,
        "block_rate_wide": 100.0 * black_wide / len(black) if black else 0,
        "fp_rate_wide": 100.0 * white_wide / len(white) if white else 0,
        "p50": percentile(latencies, 50),
        "p95": percentile(latencies, 95),
        "error_rate": 100.0 * errors / len(records) if records else 0,
        "status_405": count(records, "status_405"),
        "status_500": count(records, "status_500"),
        "status_other": sum(1 for r in records if r["verdict"].startswith("status_")),
    }


def category_table(records, expected):
    """rows: category -> {n, blocked, challenged, wide, examples}"""
    rows = defaultdict(
        lambda: {"n": 0, "blocked": 0, "challenged": 0, "wide": 0, "examples": []}
    )
    for r in records:
        if r["expected"] != expected:
            continue
        row = rows[r["category"]]
        row["n"] += 1
        if r["verdict"] == "blocked":
            row["blocked"] += 1
        elif r["verdict"] == "challenged":
            row["challenged"] += 1
        elif wide_blocked(r):
            row["wide"] += 1
        if (
            expected == "white"
            and r["verdict"] in INTERCEPTED
            and len(row["examples"]) < 5
        ):
            row["examples"].append(r["file"])
        if expected == "black" and r["verdict"] == "passed" and len(row["examples"]) < 5:
            row["examples"].append(r["file"])
    return rows


def main():
    groups = load([Path(p) for p in sys.argv[1:]])
    if not groups:
        print("no results given", file=sys.stderr)
        return

    print("# waf-bench summary\n")
    print("## Overall (per group)\n")
    print("`blocked` = HTTP 403 block page; `challenged` = HTTP 503 challenge page; "
          "`intercepted` = blocked + challenged.\n")
    print("| group | total | black | white | black blocked | black challenged "
          "| intercept rate | white blocked | white challenged | FP rate (blocked) "
          "| FP rate (intercepted) | p50 ms | p95 ms | error % | status_405 | status_500 |")
    print("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|")
    for group in sorted(groups):
        s = group_summary(groups[group])
        print(
            f"| {group} | {s['total']} | {s['black']} | {s['white']} "
            f"| {s['black_blocked']} | {s['black_challenged']} "
            f"| {s['intercept_rate']:.1f}% "
            f"| {s['white_blocked']} | {s['white_challenged']} "
            f"| {s['fp_rate_strict']:.2f}% | {s['fp_rate_intercepted']:.2f}% "
            f"| {s['p50']} | {s['p95']} | {s['error_rate']:.2f}% "
            f"| {s['status_405']} | {s['status_500']} |"
        )

    for group in sorted(groups):
        records = groups[group]
        print(f"\n## {group}: black samples by category\n")
        print(
            "| category | n | blocked | challenged | intercepted | +reject/error "
            "| intercept rate |"
        )
        print("|---|---|---|---|---|---|---|")
        for category, row in sorted(
            category_table(records, "black").items(),
            key=lambda item: -item[1]["n"],
        ):
            intercepted = row["blocked"] + row["challenged"]
            rate = 100.0 * intercepted / row["n"] if row["n"] else 0
            print(
                f"| {category} | {row['n']} | {row['blocked']} | {row['challenged']} "
                f"| {intercepted} | {row['wide']} | {rate:.1f}% |"
            )

        print(f"\n## {group}: white false positives by category\n")
        print("| category | n | blocked | challenged | FP rate (intercepted) | examples |")
        print("|---|---|---|---|---|---|")
        for category, row in sorted(
            category_table(records, "white").items(),
            key=lambda item: -(item[1]["blocked"] + item[1]["challenged"]),
        ):
            intercepted = row["blocked"] + row["challenged"]
            if intercepted == 0:
                continue
            rate = 100.0 * intercepted / row["n"] if row["n"] else 0
            examples = ", ".join(f"`{e}`" for e in row["examples"])
            print(
                f"| {category} | {row['n']} | {row['blocked']} | {row['challenged']} "
                f"| {rate:.2f}% | {examples} |"
            )

        print(f"\n## {group}: missed black samples (passed) by category\n")
        print("| category | missed | example files |")
        print("|---|---|---|")
        for category, row in sorted(
            category_table(records, "black").items(),
            key=lambda item: -item[1]["n"],
        ):
            missed = row["n"] - row["blocked"] - row["challenged"]
            if missed == 0:
                continue
            examples = ", ".join(f"`{e}`" for e in row["examples"])
            print(f"| {category} | {missed} | {examples} |")


if __name__ == "__main__":
    main()
