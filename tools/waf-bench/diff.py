#!/usr/bin/env python3
"""Compare two waf-bench JSONL runs sample by sample.

Keys are the `file` values produced by `replay.py` (relative to the replayed
root), so a suite built with `suites.py build` stays comparable with a full-set
run. Latency, HTTP detail and group labels are deliberately ignored: only the
verdict decides whether the engine changed behaviour.

Usage:
  diff.py old.jsonl new.jsonl [--limit N] [--fail-on-regression]

Exit status is 0 unless `--fail-on-regression` is given and a malicious sample
was lost or a benign sample became blocked.
"""

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from replay import INTERCEPTED  # noqa: E402  (one calibre definition, shared)


def load(path: Path) -> dict[str, dict]:
    records: dict[str, dict] = {}
    with path.open() as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            try:
                record = json.loads(line)
            except json.JSONDecodeError as exc:
                raise SystemExit(f"error: {path}: invalid JSON ({exc})") from exc
            key = record.get("file")
            if not key:
                raise SystemExit(f"error: {path}: record without a 'file' field")
            if key in records:
                raise SystemExit(f"error: {path}: duplicate sample '{key}'")
            records[key] = record
    if not records:
        raise SystemExit(f"error: no records in {path}")
    return records


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("old")
    parser.add_argument("new")
    parser.add_argument("--limit", type=int, default=40, help="max rows per detail table")
    parser.add_argument(
        "--fail-on-regression",
        action="store_true",
        help="exit 1 when a black sample is lost or a white sample becomes blocked",
    )
    args = parser.parse_args()

    old = load(Path(args.old))
    new = load(Path(args.new))
    common = sorted(set(old) & set(new))
    added = sorted(set(new) - set(old))
    removed = sorted(set(old) - set(new))

    gained: list[tuple[str, str, str, str]] = []
    lost: list[tuple[str, str, str, str]] = []
    new_fp: list[tuple[str, str, str, str]] = []
    new_challenge: list[tuple[str, str, str, str]] = []
    downgraded: list[tuple[str, str, str, str]] = []
    fixed_fp: list[tuple[str, str, str, str]] = []
    flipped: list[tuple[str, str, str, str]] = []

    for key in common:
        before, after = old[key], new[key]
        old_verdict, new_verdict = before.get("verdict"), after.get("verdict")
        if before.get("expected") == "black":
            if old_verdict not in INTERCEPTED and new_verdict in INTERCEPTED:
                gained.append((key, before.get("category", "?"), old_verdict, new_verdict))
            elif old_verdict in INTERCEPTED and new_verdict not in INTERCEPTED:
                lost.append((key, before.get("category", "?"), old_verdict, new_verdict))
            elif old_verdict != new_verdict:
                flipped.append((key, before.get("category", "?"), old_verdict, new_verdict))
        else:
            if new_verdict == "blocked" and old_verdict != "blocked":
                new_fp.append((key, before.get("category", "?"), old_verdict, new_verdict))
            elif new_verdict == "challenged" and old_verdict not in INTERCEPTED:
                new_challenge.append(
                    (key, before.get("category", "?"), old_verdict, new_verdict)
                )
            elif old_verdict == "blocked" and new_verdict == "challenged":
                # Still intercepted, but the visitor now gets a cookie round
                # trip instead of a refusal: report it separately.
                downgraded.append((key, before.get("category", "?"), old_verdict, new_verdict))
            elif old_verdict == "blocked" and new_verdict != "blocked":
                fixed_fp.append((key, before.get("category", "?"), old_verdict, new_verdict))
            elif old_verdict == "challenged" and new_verdict == "passed":
                fixed_fp.append((key, before.get("category", "?"), old_verdict, new_verdict))
            elif old_verdict != new_verdict:
                flipped.append((key, before.get("category", "?"), old_verdict, new_verdict))

    print("# waf-bench verdict diff\n")
    print(f"- old: `{args.old}` ({len(old)} samples)")
    print(f"- new: `{args.new}` ({len(new)} samples)")
    print(f"- common: {len(common)}, only in old: {len(removed)}, only in new: {len(added)}")
    print()
    print("| change | count |")
    print("|---|---|")
    print(f"| gained (black now blocked) | {len(gained)} |")
    print(f"| lost (black no longer blocked) | {len(lost)} |")
    print(f"| new false positive (white now blocked) | {len(new_fp)} |")
    print(f"| new challenge (white now challenged) | {len(new_challenge)} |")
    print(f"| fixed false positive (white no longer blocked) | {len(fixed_fp)} |")
    print(f"| downgraded (block → challenge) | {len(downgraded)} |")
    print(f"| other verdict flips | {len(flipped)} |")

    for title, rows in (
        ("Lost (regression)", lost),
        ("New false positives (regression)", new_fp),
        ("New challenges (regression)", new_challenge),
        ("Downgraded to challenge", downgraded),
        ("Gained", gained),
        ("Fixed false positives", fixed_fp),
        ("Other verdict flips", flipped),
    ):
        if not rows:
            continue
        print(f"\n## {title}\n")
        print("| sample | category | old | new |")
        print("|---|---|---|---|")
        for key, category, before, after in rows[: args.limit]:
            print(f"| `{key}` | {category} | {before} | {after} |")
        if len(rows) > args.limit:
            print(f"\n({len(rows) - args.limit} more rows omitted)")

    if added or removed:
        print("\n## Set difference\n")
        for key in removed[: args.limit]:
            print(f"- removed: `{key}`")
        for key in added[: args.limit]:
            print(f"- added: `{key}`")

    if args.fail_on_regression and (lost or new_fp or new_challenge):
        print(
            f"\nregression: {len(lost)} lost black sample(s), {len(new_fp)} new false "
            f"positive(s), {len(new_challenge)} new challenge(s)",
            file=sys.stderr,
        )
        raise SystemExit(1)


if __name__ == "__main__":
    main()
