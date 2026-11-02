#!/usr/bin/env bash
#
# Run one waf-bench suite against a static data plane and record the result.
#
#   ./run_suite.sh --conf conf/level-strict-body.toml --suite full \
#     --group strict-body --binary target/release/pingwaf \
#     --root /tmp/blazehttp --out results/0.32.4/strict-body
#
# The script starts a mock origin, starts `pingwaf -c <conf>`, proves with
# probes that the rules were actually assembled (a TOML key the plugin does not
# recognise is silently ignored), replays the suite, writes a summary and shuts
# both processes down. Everything it starts, it stops.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$HERE/../.." && pwd)"

CONF=""
SUITE="full"
GROUP=""
BINARY="$REPO_ROOT/target/release/pingwaf"
ROOT="${BLAZEHTTP_ROOT:-}"
OUT=""
TARGET="127.0.0.1:6188"
ORIGIN_PORT="6199"
CONCURRENCY="32"
DELAY_MS="0"
SQLI_EXPECT="403"
JNDI_EXPECT="skip"
SKIP_CHECK="0"
SKIP_PROBES="0"
SUITE_DIR=""

ORIGIN_PID=""
PINGWAF_PID=""
STARTED_ORIGIN="0"

usage() {
  sed -n '2,14p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
  cat <<'EOF'

Options:
  --conf PATH          data-plane TOML to test (required)
  --suite NAME         suite from suites.json (default: full)
  --group NAME         result group label (default: conf file name)
  --binary PATH        pingwaf binary (default: target/release/pingwaf)
  --root PATH          blazehttp clone root (default: $BLAZEHTTP_ROOT)
  --out PATH           result directory (default: results/<version>/<group>)
  --target HOST:PORT   data-plane address (default: 127.0.0.1:6188)
  --origin-port PORT   mock origin port (default: 6199)
  --concurrency N      replay threads (default: 32)
  --delay-ms N         delay between submissions (default: 0)
  --sqli-expect CODE   expected status for the SQLi probe, or "skip" (default: 403)
  --jndi-expect CODE   expected status for the Log4Shell probe, or "skip" (default: skip)
  --suite-dir PATH     reuse (or create) a shared symlink tree across tiers;
                       falls back to <out>/suite when omitted
  --skip-check         do not re-validate the manifest (faster repeat runs)
  --skip-probes        do not run the rule-assembly probes
EOF
}

cleanup() {
  if [[ -n "$PINGWAF_PID" ]] && kill -0 "$PINGWAF_PID" 2>/dev/null; then
    kill "$PINGWAF_PID" 2>/dev/null || true
    wait "$PINGWAF_PID" 2>/dev/null || true
  fi
  if [[ "$STARTED_ORIGIN" == "1" && -n "$ORIGIN_PID" ]] && kill -0 "$ORIGIN_PID" 2>/dev/null; then
    kill "$ORIGIN_PID" 2>/dev/null || true
    wait "$ORIGIN_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

while [[ $# -gt 0 ]]; do
  case "$1" in
    --conf) CONF="${2:?}"; shift 2 ;;
    --suite) SUITE="${2:?}"; shift 2 ;;
    --group) GROUP="${2:?}"; shift 2 ;;
    --binary) BINARY="${2:?}"; shift 2 ;;
    --root) ROOT="${2:?}"; shift 2 ;;
    --out) OUT="${2:?}"; shift 2 ;;
    --target) TARGET="${2:?}"; shift 2 ;;
    --origin-port) ORIGIN_PORT="${2:?}"; shift 2 ;;
    --concurrency) CONCURRENCY="${2:?}"; shift 2 ;;
    --delay-ms) DELAY_MS="${2:?}"; shift 2 ;;
    --sqli-expect) SQLI_EXPECT="${2:?}"; shift 2 ;;
    --jndi-expect) JNDI_EXPECT="${2:?}"; shift 2 ;;
    --suite-dir) SUITE_DIR="${2:?}"; shift 2 ;;
    --skip-check) SKIP_CHECK="1"; shift ;;
    --skip-probes) SKIP_PROBES="1"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "error: unknown option $1" >&2; usage >&2; exit 2 ;;
  esac
done

if [[ -z "$CONF" ]]; then
  echo "error: --conf is required" >&2
  usage >&2
  exit 2
fi
[[ -f "$CONF" ]] || { echo "error: conf not found: $CONF" >&2; exit 2; }
[[ -x "$BINARY" ]] || { echo "error: binary not found or not executable: $BINARY" >&2; exit 2; }
[[ -n "$ROOT" ]] || { echo "error: pass --root or set BLAZEHTTP_ROOT" >&2; exit 2; }
command -v curl >/dev/null || { echo "error: curl is required for probes and readiness checks" >&2; exit 2; }

workspace_version() {
  awk '/^\[workspace\.package\]/{f=1;next} f&&/^version = /{gsub(/"/,"",$3);print $3;exit}' \
    "$REPO_ROOT/Cargo.toml"
}

if [[ -z "$GROUP" ]]; then
  GROUP="$(basename "$CONF" .toml)"
fi
if [[ -z "$OUT" ]]; then
  OUT="$REPO_ROOT/results/$(workspace_version)/$GROUP"
fi
mkdir -p "$OUT"

TARGET_HOST="${TARGET%%:*}"
TARGET_PORT="${TARGET##*:}"

echo "[run_suite] conf=$CONF suite=$SUITE group=$GROUP out=$OUT"

if [[ "$SKIP_CHECK" != "1" ]]; then
  python3 "$HERE/suites.py" check --root "$ROOT"
fi
if [[ -z "$SUITE_DIR" ]]; then
  SUITE_DIR="$OUT/suite"
fi
# `--reuse` keeps an existing tree only when it is the same suite over the same
# dataset and its links still resolve; otherwise it is rebuilt.
python3 "$HERE/suites.py" build --root "$ROOT" --suite "$SUITE" --out "$SUITE_DIR" --reuse

port_open() {
  curl -s -o /dev/null --max-time 1 "http://$1/" || return 1
}

origin_ready() {
  [[ "$(curl -s --max-time 2 "http://127.0.0.1:$ORIGIN_PORT/" || true)" == "ok" ]]
}

if origin_ready; then
  echo "[run_suite] mock origin already answering on :$ORIGIN_PORT — reusing it"
else
  if port_open "127.0.0.1:$ORIGIN_PORT"; then
    echo "error: port $ORIGIN_PORT is held by a service that is not mock_origin" >&2
    exit 3
  fi
  python3 "$HERE/mock_origin.py" --port "$ORIGIN_PORT" >"$OUT/origin.log" 2>&1 &
  ORIGIN_PID=$!
  STARTED_ORIGIN="1"
  for _ in $(seq 1 40); do
    if origin_ready; then break; fi
    sleep 0.25
  done
  if ! origin_ready; then
    echo "error: mock origin did not become ready on :$ORIGIN_PORT" >&2
    tail -n 20 "$OUT/origin.log" >&2 || true
    exit 3
  fi
fi

"$BINARY" -c "$CONF" >"$OUT/pingwaf.log" 2>&1 &
PINGWAF_PID=$!

ready="0"
for _ in $(seq 1 80); do
  if ! kill -0 "$PINGWAF_PID" 2>/dev/null; then
    echo "error: data plane exited during startup" >&2
    tail -n 40 "$OUT/pingwaf.log" >&2 || true
    exit 3
  fi
  if port_open "$TARGET"; then
    ready="1"
    break
  fi
  sleep 0.25
done
if [[ "$ready" != "1" ]]; then
  echo "error: data plane did not become ready on $TARGET" >&2
  tail -n 40 "$OUT/pingwaf.log" >&2 || true
  exit 3
fi

probe() {
  local label="$1" expect="$2" code
  shift 2
  if [[ "$expect" == "skip" ]]; then
    echo "[probe] $label: skipped"
    return 0
  fi
  code="$(curl -s -o /dev/null --max-time 10 -w '%{http_code}' "$@" || true)"
  if [[ "$code" == "$expect" ]]; then
    echo "[probe] $label: $code (expected $expect)"
    return 0
  fi
  echo "[probe] $label: got $code, expected $expect — the configuration did not take effect" >&2
  echo "--- pingwaf.log (tail) ---" >&2
  tail -n 40 "$OUT/pingwaf.log" >&2 || true
  exit 4
}

if [[ "$SKIP_PROBES" != "1" ]]; then
  probe "plain request" "200" "http://$TARGET/"
  probe "dotfile" "403" "http://$TARGET/.env"
  probe "TRACE" "403" -X TRACE "http://$TARGET/"
  probe "SQLi" "$SQLI_EXPECT" "http://$TARGET/?id=1%20union%20select%201"
  probe "Log4Shell UA" "$JNDI_EXPECT" -A '${jndi:ldap://127.0.0.1:1389/a}' "http://$TARGET/"
fi

echo "[run_suite] replaying $SUITE on $TARGET"
python3 "$HERE/replay.py" "$SUITE_DIR" \
  -t "$TARGET" -g "$GROUP" -o "$OUT/run.jsonl" \
  --concurrency "$CONCURRENCY" --delay-ms "$DELAY_MS"

python3 "$HERE/summarize.py" "$OUT/run.jsonl" >"$OUT/summary.md"

GIT_SHA="$(git -C "$REPO_ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
if [[ -n "$(git -C "$REPO_ROOT" status --porcelain 2>/dev/null | head -n 1)" ]]; then
  GIT_DIRTY="true"
else
  GIT_DIRTY="false"
fi

# Generated by python so a path with a quote or backslash cannot corrupt the file.
RUN_OUT="$OUT/run.json" \
RUN_VERSION="$(workspace_version)" \
RUN_GIT_SHA="$GIT_SHA" \
RUN_GIT_DIRTY="$GIT_DIRTY" \
RUN_CONF="$CONF" \
RUN_SUITE="$SUITE" \
RUN_GROUP="$GROUP" \
RUN_TARGET="$TARGET" \
RUN_ORIGIN_PORT="$ORIGIN_PORT" \
RUN_CONCURRENCY="$CONCURRENCY" \
RUN_DELAY_MS="$DELAY_MS" \
RUN_DATASET_ROOT="$ROOT" \
python3 - <<'PY'
import json
import os
import time

payload = {
    "version": os.environ["RUN_VERSION"],
    "git_sha": os.environ["RUN_GIT_SHA"],
    "git_dirty": os.environ["RUN_GIT_DIRTY"] == "true",
    "conf": os.environ["RUN_CONF"],
    "suite": os.environ["RUN_SUITE"],
    "group": os.environ["RUN_GROUP"],
    "target": os.environ["RUN_TARGET"],
    "origin_port": int(os.environ["RUN_ORIGIN_PORT"]),
    "concurrency": int(os.environ["RUN_CONCURRENCY"]),
    "delay_ms": float(os.environ["RUN_DELAY_MS"]),
    "dataset_root": os.environ["RUN_DATASET_ROOT"],
    "finished_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
}
with open(os.environ["RUN_OUT"], "w") as handle:
    json.dump(payload, handle, indent=2)
    handle.write("\n")
PY

echo "[run_suite] wrote $OUT/run.jsonl, $OUT/summary.md, $OUT/run.json"
