#!/usr/bin/env python3
"""Check that every REST route has a matching section in docs/api.md.

The control plane registers its routes with axum's `.route(path, method-router)`
chain, and the API reference documents each one under a `### METHOD /path`
heading. Neither side is generated, so the two drift silently; this script is
the gate that keeps them in step.

What it compares
----------------
* Every `.route(...)` registered by `pingwaf-server/src/api/*.rs`, with the HTTP
  methods taken from the method-router chain (`get(a).post(b)`) and the path
  from the first string literal.
* Every `### METHOD /path` heading in `docs/api.md`.

Before comparing, both sides are normalised: path parameters collapse to `{}`
(`{agent_id}` and `{id}` are the same endpoint), trailing slashes are dropped,
and a heading's query string (`?file=robots|llms`) is ignored.

What it deliberately ignores
----------------------------
* Code under `#[cfg(test)]`: test modules register throwaway routers.
* Routes mounted outside `/api/v1` (currently the root `/healthz` probe, which
  is documented in the deployment guide rather than the API reference). Keep
  ROOT_ROUTES in sync when adding another root-level route.

Usage
-----
    python3 scripts/check-api-docs.py          # exit 1 on drift
    python3 scripts/check-api-docs.py --list   # also print the matched set

Exit status is 0 only when the two sides agree.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
API_DIR = REPO_ROOT / "pingwaf-server" / "src" / "api"
DOCS = REPO_ROOT / "docs" / "api.md"

METHODS = ("get", "post", "put", "patch", "delete", "head", "options")
METHOD_RE = re.compile(
    r"(?<![A-Za-z0-9_])(" + "|".join(METHODS) + r")\s*\(", re.IGNORECASE
)
DOC_HEADING_RE = re.compile(
    r"^### (GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS)\s+(\S+)", re.MULTILINE
)

# Routes mounted on the root router rather than under `/api/v1`. They are
# documented outside docs/api.md (deployment / quick start), so the API
# reference is not expected to list them.
ROOT_ROUTES = {
    ("GET", "/healthz"),
}

# Endpoints intentionally left out of the reference. Prefer documenting a route
# over adding it here: this list exists for plumbing that is not part of the
# public contract.
IGNORED: set[tuple[str, str]] = set()


def strip_cfg_test(text: str) -> str:
    """Remove every `#[cfg(test)]` item (attributes + the item itself).

    Test modules register routers that look exactly like production ones, so
    they must be gone before parsing. The attribute can sit on a module, a
    function, a `use` or a `const`; scanning for the item's end by depth (not
    by "first brace") keeps array/struct literals from swallowing the code
    that follows.
    """
    out = text
    index = 0
    while True:
        match = re.search(r"#\[cfg\(test\)\]", out[index:])
        if match is None:
            break
        start = index + match.start()
        cursor = start + len(match.group(0))
        # Stacked attributes belong to the same item.
        while True:
            skip = re.match(r"\s*", out[cursor:])
            cursor += skip.end()
            attribute = re.match(r"#\[[^\]]*\]", out[cursor:])
            if attribute is None:
                break
            cursor += attribute.end()

        depth = 0
        in_string = False
        while cursor < len(out):
            char = out[cursor]
            if in_string:
                if char == "\\":
                    cursor += 2
                    continue
                if char == '"':
                    in_string = False
                cursor += 1
                continue
            if char == '"':
                in_string = True
            elif char in "([{":
                depth += 1
            elif char in ")]}":
                depth -= 1
                if depth <= 0 and char == "}":
                    cursor += 1
                    # A `const X: S = S { .. };` ends after the semicolon.
                    tail = re.match(r"\s*;", out[cursor:])
                    if tail is not None:
                        cursor += tail.end()
                    break
            elif char == ";" and depth == 0:
                cursor += 1
                break
            cursor += 1
        out = out[:start] + out[cursor:]
        index = start
    return out


def normalise(path: str) -> str:
    """Compare paths by shape, not by parameter spelling."""
    path = path.split("?", 1)[0].strip()
    path = re.sub(r"\{[^}]*\}", "{}", path)
    if len(path) > 1:
        path = path.rstrip("/")
    return path or "/"


def parse_routes(path: Path) -> list[tuple[str, str, str, int]]:
    """Return `(method, path, file, line)` for every registered route."""
    text = strip_cfg_test(path.read_text(encoding="utf-8"))
    routes: list[tuple[str, str, str, int]] = []

    for match in re.finditer(r"\.route\(", text):
        opening = match.end() - 1
        depth = 0
        cursor = opening
        while cursor < len(text):
            char = text[cursor]
            if char == "(":
                depth += 1
            elif char == ")":
                depth -= 1
                if depth == 0:
                    break
            cursor += 1
        call = text[opening + 1 : cursor]

        literal = re.match(r'\s*"([^"]*)"\s*,(.*)$', call, re.DOTALL)
        if literal is None:
            # Not a literal path (a constant or a computed value): the route
            # cannot be verified statically, so fail loudly instead of
            # silently skipping it.
            line = text[: match.start()].count("\n") + 1
            routes.append(("<dynamic>", call.strip()[:60], path.name, line))
            continue

        route_path = normalise(literal.group(1))
        remainder = literal.group(2)
        methods = {
            method.upper() for method in METHOD_RE.findall(remainder)
        }
        line = text[: match.start()].count("\n") + 1
        if not methods:
            routes.append(("<unknown>", route_path, path.name, line))
            continue
        for method in sorted(methods):
            routes.append((method, route_path, path.name, line))

    return routes


def parse_docs(path: Path) -> dict[tuple[str, str], int]:
    documented: dict[tuple[str, str], int] = {}
    text = path.read_text(encoding="utf-8")
    for match in DOC_HEADING_RE.finditer(text):
        method, raw = match.group(1), match.group(2)
        line = text[: match.start()].count("\n") + 1
        documented[(method, normalise(raw))] = line
    return documented


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--list",
        action="store_true",
        help="print every matched endpoint after the report",
    )
    args = parser.parse_args()

    if not API_DIR.is_dir():
        print(f"error: {API_DIR} not found", file=sys.stderr)
        return 2
    if not DOCS.is_file():
        print(f"error: {DOCS} not found", file=sys.stderr)
        return 2

    registered: dict[tuple[str, str], tuple[str, int]] = {}
    duplicates: list[tuple[str, str, str, int, str, int]] = []
    for source in sorted(API_DIR.glob("*.rs")):
        for method, route_path, name, line in parse_routes(source):
            key = (method, route_path)
            if key in registered:
                first_name, first_line = registered[key]
                duplicates.append(
                    (method, route_path, first_name, first_line, name, line)
                )
                continue
            registered[key] = (name, line)

    documented = parse_docs(DOCS)

    routed = {
        key: value
        for key, value in registered.items()
        if key not in ROOT_ROUTES and key not in IGNORED
    }
    missing_docs = sorted(set(routed) - set(documented))
    stale_docs = sorted(set(documented) - set(routed))

    if duplicates:
        # axum panics at boot on an overlapping method route; catching it here
        # turns a production crash into a review comment.
        print(f"Duplicate route registration: {len(duplicates)}")
        for method, route_path, first_name, first_line, name, line in duplicates:
            print(
                f"  {method:<6} {route_path:<55} "
                f"{first_name}:{first_line} and {name}:{line}"
            )
        return 1

    if missing_docs or stale_docs:
        print(
            f"API documentation drift: {len(missing_docs)} routed endpoint(s) "
            f"undocumented, {len(stale_docs)} documented endpoint(s) not routed."
        )
        if missing_docs:
            print("\nRouted but undocumented (add to docs/api.md):")
            for method, route_path in missing_docs:
                name, line = routed[(method, route_path)]
                print(f"  {method:<6} {route_path:<55} {name}:{line}")
        if stale_docs:
            print("\nDocumented but not routed (remove or fix the route):")
            for method, route_path in stale_docs:
                print(
                    f"  {method:<6} {route_path:<55} "
                    f"docs/api.md:{documented[(method, route_path)]}"
                )
        return 1

    print(f"API docs match {len(routed)} routes in {DOCS.name}.")
    if args.list:
        for method, route_path in sorted(routed):
            name, line = routed[(method, route_path)]
            print(f"  {method:<6} {route_path:<55} {name}:{line}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
