#!/usr/bin/env python3
"""Manage the blazehttp sample suites declared in `suites.json`.

The dataset is never copied: a suite is materialised as a tree of symlinks that
mirrors the original relative paths, so `replay.py` reports the same `file`
value as a direct run against the dataset. Only the `.black` / `.white` suffix
decides the expectation, which means the mirrored layout needs no grouping.

Subcommands:

  check    validate the manifest against the dataset and print suite sizes
  build    materialise one suite as a symlink tree (replay.py input)
  pin      write missed attacks / false positives from a JSONL run back into
           the manifest, so the next round can prove it did not regress
  stats    category distribution plus the artefact counts used for the
           effective denominator in the report
"""

import argparse
import json
import os
import re
import shutil
import sys
import time
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from pathlib import Path

HERE = Path(__file__).resolve().parent
DEFAULT_MANIFEST = HERE / "suites.json"

sys.path.insert(0, str(HERE))
from replay import INTERCEPTED, classify  # noqa: E402  (same-directory module)

REQUEST_LINE = re.compile(rb"^([A-Z]+) (\S+) HTTP/1\.[01]$")


@dataclass
class Sample:
    rel: str
    path: Path
    expected: str  # "black" | "white"
    category: str
    artefacts: list[str] = field(default_factory=list)


def fail(message: str) -> None:
    print(f"error: {message}", file=sys.stderr)
    raise SystemExit(1)


def load_manifest(path: Path) -> dict:
    try:
        with path.open() as handle:
            return json.load(handle)
    except FileNotFoundError:
        fail(f"manifest not found: {path}")
    except json.JSONDecodeError as exc:
        fail(f"manifest is not valid JSON ({path}): {exc}")


def resolve_dataset(root: Path) -> Path:
    """Accept either the clone root or the `testcases` directory itself."""
    root = root.expanduser().resolve()
    nested = root / "testcases"
    if nested.is_dir():
        return nested
    if root.is_dir() and any(root.glob("**/*.black")):
        return root
    fail(f"{root} is neither a blazehttp clone root nor a testcases directory")


def header_block(raw: bytes) -> tuple[bytes, bytes]:
    for separator in (b"\r\n\r\n", b"\n\n"):
        index = raw.find(separator)
        if index >= 0:
            return raw[:index], raw[index + len(separator):]
    return raw, b""


def request_meta(raw: bytes) -> tuple[str | None, bytes, dict[str, bytes]]:
    head, _ = header_block(raw)
    lines = head.replace(b"\r\n", b"\n").split(b"\n")
    if not lines:
        return None, b"", {}
    match = REQUEST_LINE.match(lines[0].rstrip(b"\r"))
    if not match:
        return None, b"", {}
    method, target = match.group(1).decode(), match.group(2)
    headers: dict[str, bytes] = {}
    for line in lines[1:]:
        if b":" not in line:
            continue
        name, _, value = line.partition(b":")
        headers[name.strip().lower().decode("latin-1")] = value.strip()
    return method, target, headers


def artefact_rules(manifest: dict, raw: bytes) -> list[str]:
    method, target, headers = request_meta(raw)
    hits: list[str] = []
    for rule in manifest.get("exclude", []):
        methods = rule.get("methods")
        if methods and method not in methods:
            continue
        missing = rule.get("missing_all_headers")
        if missing and not all(name not in headers for name in missing):
            continue  # at least one framing header is present, so not this artefact
        contains = rule.get("request_target_contains")
        if contains is not None and contains.encode() not in target:
            continue
        if "verdicts" in rule:
            continue  # verdict-based artefacts are applied to JSONL, not to bytes
        hits.append(rule["id"])
    return hits


LOCAL_PREFIX = "local/"


def walk_tree(root: Path, manifest: dict, prefix: str) -> list[Sample]:
    samples: list[Sample] = []
    for path in sorted(root.rglob("*")):
        if not path.is_file() or path.suffix not in (".black", ".white"):
            continue
        raw = path.read_bytes()
        samples.append(
            Sample(
                rel=f"{prefix}{path.relative_to(root)}",
                path=path,
                expected="black" if path.suffix == ".black" else "white",
                category=classify(raw),
                artefacts=artefact_rules(manifest, raw),
            )
        )
    return samples


def local_root(manifest: dict) -> Path | None:
    """Samples that ship with this repository rather than with blazehttp."""
    raw = manifest.get("dataset", {}).get("local_root")
    if not raw:
        return None
    path = (HERE / raw).resolve()
    return path if path.is_dir() else None


def discover(root: Path, manifest: dict) -> list[Sample]:
    samples = walk_tree(root, manifest, "")
    if not samples:
        fail(f"no .black/.white samples under {root}")
    extra = local_root(manifest)
    if extra:
        samples.extend(walk_tree(extra, manifest, LOCAL_PREFIX))
    return samples


def select(suite_name: str, suite: dict, samples: list[Sample], manifest: dict) -> list[Sample]:
    mode = suite.get("select")
    external = [sample for sample in samples if not sample.rel.startswith(LOCAL_PREFIX)]
    if mode == "local":
        # The repository's own samples (compressed attack bodies and their
        # benign twins) are kept out of every upstream-corpus suite so the
        # headline numbers stay comparable with earlier rounds.
        local = [sample for sample in samples if sample.rel.startswith(LOCAL_PREFIX)]
        if not local:
            fail("no local samples found; run tools/waf-bench/make_local_samples.py")
        return local
    if mode == "all":
        return external
    if mode == "pinned":
        by_rel = {sample.rel: sample for sample in samples}
        chosen: list[Sample] = []
        missing: list[str] = []
        for pin in suite.get("pins", []):
            sample = by_rel.get(pin["file"])
            if sample is None:
                missing.append(pin["file"])
            else:
                chosen.append(sample)
        if missing:
            fail(
                f"suite '{suite_name}' references {len(missing)} pinned sample(s) "
                f"missing from the dataset, e.g. {missing[0]}"
            )
        return chosen
    if mode == "per-category":
        buckets: dict[tuple[str, str], list[Sample]] = defaultdict(list)
        for sample in external:
            buckets[(sample.expected, sample.category)].append(sample)
        chosen = []
        for category in manifest.get("categories", []):
            for expected, key in (("black", "black"), ("white", "white")):
                limit = suite.get(key, 0)
                bucket = sorted(buckets[(expected, category)], key=lambda item: item.rel)
                if limit == "all":
                    chosen.extend(bucket)
                elif isinstance(limit, int) and limit > 0:
                    chosen.extend(bucket[:limit])
        return chosen
    fail(f"suite '{suite_name}' has unknown select mode '{mode}'")


def manifest_path(args) -> Path:
    return Path(args.manifest).expanduser() if args.manifest else DEFAULT_MANIFEST


def write_atomic(path: Path, text: str) -> None:
    """Write through a sibling temp file so an interrupt cannot truncate it."""
    tmp = path.with_name(f"{path.name}.tmp{os.getpid()}")
    tmp.write_text(text)
    os.replace(tmp, path)


def refuse_dangerous_out(out: Path) -> None:
    """`build` replaces its output directory; make that impossible to aim wrong."""
    resolved = out.resolve()
    home = Path.home().resolve()
    if resolved.parent == resolved:
        fail(f"refusing to replace the filesystem root: {resolved}")
    if resolved in (home, home.parent, HERE, HERE.parent, HERE.parent.parent):
        fail(f"refusing to replace {resolved}: pick a dedicated output directory")
    manifest_out = out.with_suffix(".manifest.json")
    if out.exists() and not manifest_out.is_file():
        fail(
            f"{out} exists but was not created by suites.py "
            f"(no {manifest_out.name}); remove it yourself or choose another --out"
        )


def acquire_lock(out: Path, timeout: float = 120.0) -> Path:
    """Serialise concurrent builds of the same tree (mkdir is atomic everywhere)."""
    lock = out.with_name(f"{out.name}.lock")
    deadline = time.monotonic() + timeout
    while True:
        try:
            lock.mkdir()
            return lock
        except FileExistsError:
            if time.monotonic() > deadline:
                fail(f"timed out waiting for {lock}; another build may be stuck")
            time.sleep(0.2)
        except OSError as exc:
            fail(f"cannot create the build lock {lock}: {exc}")


def suite_manifest_matches(out: Path, suite_name: str, root: Path) -> bool:
    """Is the existing tree the same suite over the same dataset, still readable?"""
    manifest_out = out.with_suffix(".manifest.json")
    if not out.is_dir() or not manifest_out.is_file():
        return False
    try:
        data = json.loads(manifest_out.read_text())
    except (OSError, json.JSONDecodeError):
        return False
    if data.get("suite") != suite_name or data.get("dataset") != str(root):
        return False
    files = data.get("files") or []
    if not files:
        return False
    # A re-cloned dataset leaves every symlink dangling; sample three of them.
    for rel in {files[0], files[len(files) // 2], files[-1]}:
        if not (out / rel).is_file():
            return False
    return True


def dataset_root(args) -> Path:
    if args.root:
        return resolve_dataset(Path(args.root))
    env = os.environ.get("BLAZEHTTP_ROOT")
    if not env:
        fail("pass --root or set BLAZEHTTP_ROOT to the blazehttp clone")
    return resolve_dataset(Path(env))


def cmd_check(args) -> None:
    manifest = load_manifest(manifest_path(args))
    root = dataset_root(args)
    samples = discover(root, manifest)
    external = [s for s in samples if not s.rel.startswith(LOCAL_PREFIX)]
    local = [s for s in samples if s.rel.startswith(LOCAL_PREFIX)]
    black = sum(1 for s in external if s.expected == "black")
    white = len(external) - black
    print(f"dataset: {root}")
    print(f"samples: {len(external)} (black {black}, white {white})")
    if local:
        local_black = sum(1 for s in local if s.expected == "black")
        print(f"local additions: {len(local)} ({local_black} black, {len(local) - local_black} white)")

    expected = manifest.get("dataset", {})
    for key, actual in (("black", black), ("white", white)):
        declared = expected.get(key)
        if isinstance(declared, int) and declared != actual:
            fail(f"manifest declares {declared} {key} samples, dataset has {actual}")

    seen_categories = {sample.category for sample in external}
    for category in manifest.get("categories", []):
        if category not in seen_categories:
            fail(f"category '{category}' has no samples in the dataset")

    for name, suite in manifest.get("suites", {}).items():
        chosen = select(name, suite, samples, manifest)
        selected_black = sum(1 for s in chosen if s.expected == "black")
        status = "ok"
        if not chosen and suite.get("select") == "pinned":
            status = "empty (pin it from a run)"
        print(
            f"  {name:10s} {len(chosen):6d} samples "
            f"({selected_black} black, {len(chosen) - selected_black} white)  {status}"
        )

    dupes = [rel for rel, count in Counter(s.rel for s in samples).items() if count > 1]
    if dupes:
        fail(f"dataset contains duplicate relative paths, e.g. {dupes[0]}")
    print("manifest: ok")


def cmd_build(args) -> None:
    manifest = load_manifest(manifest_path(args))
    root = dataset_root(args)
    samples = discover(root, manifest)
    suites = manifest.get("suites", {})
    suite_name = args.suite[0]
    if suite_name not in suites:
        fail(f"unknown suite '{suite_name}' (known: {', '.join(suites)})")
    chosen = select(suite_name, suites[suite_name], samples, manifest)
    if not chosen:
        fail(f"suite '{suite_name}' is empty — pin it from a run first")

    out = Path(args.out).expanduser().resolve()
    if args.reuse and suite_manifest_matches(out, suite_name, root):
        print(f"reusing suite '{suite_name}': {out}")
        return

    # Validate the target before locking: the lock lives next to it.
    refuse_dangerous_out(out)

    lock = acquire_lock(out)
    try:
        # Re-check under the lock: a sibling process may have just built it.
        if args.reuse and suite_manifest_matches(out, suite_name, root):
            print(f"reusing suite '{suite_name}': {out}")
            return

        # Build into a scratch tree and swap it in: two tiers sharing one suite
        # directory must never observe a half-populated tree.
        staging = out.with_name(f"{out.name}.tmp{os.getpid()}")
        if staging.exists():
            shutil.rmtree(staging)
        staging.mkdir(parents=True)
        for sample in chosen:
            target = staging / sample.rel
            target.parent.mkdir(parents=True, exist_ok=True)
            target.symlink_to(sample.path)
        if out.exists():
            shutil.rmtree(out)
        staging.rename(out)

        manifest_out = out.with_suffix(".manifest.json")
        write_atomic(
            manifest_out,
            json.dumps(
                {
                    "suite": suite_name,
                    "dataset": str(root),
                    "total": len(chosen),
                    "black": sum(1 for s in chosen if s.expected == "black"),
                    "white": sum(1 for s in chosen if s.expected == "white"),
                    "files": sorted(sample.rel for sample in chosen),
                },
                indent=2,
            )
            + "\n",
        )
        print(
            f"built suite '{suite_name}': {len(chosen)} samples "
            f"({sum(1 for s in chosen if s.expected == 'black')} black) -> {out}"
        )
    finally:
        lock.rmdir()


def plain_verdict(record: dict) -> str:
    return record.get("verdict", "error")


def is_artefact(record: dict, manifest: dict, samples_by_rel: dict[str, Sample]) -> bool:
    sample = samples_by_rel.get(record["file"])
    if sample and sample.artefacts:
        return True
    verdict = plain_verdict(record)
    for rule in manifest.get("exclude", []):
        if verdict in rule.get("verdicts", []):
            return True
    return False


def cmd_pin(args) -> None:
    path = manifest_path(args)
    manifest = load_manifest(path)
    root = dataset_root(args)
    samples = discover(root, manifest)
    samples_by_rel = {sample.rel: sample for sample in samples}

    records = []
    for jsonl in args.jsonl:
        source = Path(jsonl)
        with source.open() as handle:
            for lineno, line in enumerate(handle, 1):
                line = line.strip()
                if not line:
                    continue
                try:
                    record = json.loads(line)
                except json.JSONDecodeError as exc:
                    fail(f"{source}:{lineno}: invalid JSON ({exc})")
                if not isinstance(record, dict) or "file" not in record or "expected" not in record:
                    fail(f"{source}:{lineno}: record needs 'file' and 'expected'")
                records.append(record)
    if not records:
        fail("no records in the given JSONL files")

    missed = [
        record
        for record in records
        if record.get("expected") == "black"
        and plain_verdict(record) not in INTERCEPTED
        and not is_artefact(record, manifest, samples_by_rel)
    ]
    false_positives = [
        record
        for record in records
        if record.get("expected") == "white" and plain_verdict(record) in INTERCEPTED
    ]

    targets = args.suite or ["triage", "fp-pin"]
    reason = args.reason or f"pinned from {Path(args.jsonl[0]).name}"
    changed = 0
    for target in targets:
        if target not in ("triage", "fp-pin"):
            fail(f"cannot pin into '{target}'; only triage and fp-pin take pins")
        suite = manifest["suites"][target]
        existing = {pin["file"]: pin for pin in suite.get("pins", [])}
        incoming = missed if target == "triage" else false_positives
        for record in incoming:
            rel = record["file"]
            if rel in existing:
                continue
            existing[rel] = {
                "file": rel,
                "expected": record.get("expected"),
                "category": record.get("category", "other"),
                "reason": reason,
            }
            changed += 1
        suite["pins"] = [existing[key] for key in sorted(existing)]

    print(f"missed attacks ({len(missed)}):")
    for record in missed[: args.limit]:
        print(f"  {record['file']}  [{record.get('category')}] {plain_verdict(record)}")
    print(f"false positives ({len(false_positives)}):")
    for record in false_positives[: args.limit]:
        print(f"  {record['file']}  [{record.get('category')}]")

    if args.dry_run:
        print(f"dry run: {changed} new pin(s), manifest not written")
        return
    write_atomic(path, json.dumps(manifest, indent=2) + "\n")
    print(f"wrote {changed} new pin(s) to {path}")


def cmd_stats(args) -> None:
    manifest = load_manifest(manifest_path(args))
    root = dataset_root(args)
    samples = [
        sample
        for sample in discover(root, manifest)
        if not sample.rel.startswith(LOCAL_PREFIX)
    ]
    black = [sample for sample in samples if sample.expected == "black"]
    white = [sample for sample in samples if sample.expected == "white"]

    print(f"dataset: {root}")
    print(f"| category | black | white |")
    print("|---|---|---|")
    black_counts = Counter(sample.category for sample in black)
    white_counts = Counter(sample.category for sample in white)
    for category in manifest.get("categories", []):
        print(f"| {category} | {black_counts.get(category, 0)} | {white_counts.get(category, 0)} |")

    print()
    print("| artefact | black | white |")
    print("|---|---|---|")
    for rule in manifest.get("exclude", []):
        rule_id = rule["id"]
        print(
            f"| {rule_id} "
            f"| {sum(1 for s in black if rule_id in s.artefacts)} "
            f"| {sum(1 for s in white if rule_id in s.artefacts)} |"
        )
    effective_black = sum(1 for s in black if not s.artefacts)
    effective_white = sum(1 for s in white if not s.artefacts)
    print()
    print(
        f"effective denominator (byte-level artefacts excluded): "
        f"black {effective_black}/{len(black)}, white {effective_white}/{len(white)}"
    )
    print("note: verdict-level artefacts (protocol_reject) are counted from a run, not here")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("command", choices=["check", "build", "pin", "stats"])
    parser.add_argument("--manifest", help="path to suites.json (default: next to this script)")
    parser.add_argument("--root", help="blazehttp clone root or testcases directory (default: $BLAZEHTTP_ROOT)")
    parser.add_argument("--suite", action="append", help="suite name (repeatable for pin)")
    parser.add_argument("--out", help="output directory for build")
    parser.add_argument(
        "--reuse",
        action="store_true",
        help="build: keep an existing tree when it is the same suite over the same dataset",
    )
    parser.add_argument("--jsonl", action="append", default=[], help="replay JSONL to pin from")
    parser.add_argument("--reason", help="reason recorded on every new pin")
    parser.add_argument("--dry-run", action="store_true", help="report pins without writing the manifest")
    parser.add_argument("--limit", type=int, default=20, help="max samples printed per list")
    args = parser.parse_args()

    if args.command == "build" and not args.out:
        fail("build needs --out")
    if args.command == "pin" and not args.jsonl:
        fail("pin needs at least one --jsonl")
    if args.command == "build" and not args.suite:
        fail("build needs --suite")
    if args.command == "build" and len(args.suite) != 1:
        fail("build takes exactly one --suite")

    handler = {
        "check": cmd_check,
        "build": cmd_build,
        "pin": cmd_pin,
        "stats": cmd_stats,
    }[args.command]
    handler(args)


if __name__ == "__main__":
    main()
