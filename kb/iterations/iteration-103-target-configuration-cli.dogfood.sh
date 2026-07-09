#!/usr/bin/env bash
# iter-103 dogfood gate — `emulate` (target-configuration actor).
# AC slug: dogfood_script_full_run_iter_103 (exits 0 and writes the sentinel below)
#
# Exercises the emulate command end to end against a real launched Firefox over
# the daemon path (so emulation persists across separate `ff-rdp` invocations):
#   1. emulate --color-scheme dark  → prefers-color-scheme: dark matches
#   2. emulate --user-agent <S>     → navigator.userAgent equals the override
#   3. emulate --reset              → color scheme reverts to system default
#
# Run manually:
#   FF_RDP_LIVE_TESTS=1 bash kb/iterations/iteration-103-target-configuration-cli.dogfood.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
for candidate in "$REPO_ROOT/target/debug/ff-rdp" "$REPO_ROOT/target/release/ff-rdp"; do
  if [ -x "$candidate" ]; then
    CANDIDATE_DIR="$(dirname "$candidate")"
    export PATH="$CANDIDATE_DIR:$PATH"
    echo "using ff-rdp: $candidate"
    break
  fi
done
unset candidate SCRIPT_DIR

SENTINEL=/tmp/ff-rdp-iter-103-dogfood-ok
rm -f "$SENTINEL"

PORT=6003

cleanup() {
  ff-rdp --port "$PORT" daemon stop 2>/dev/null || true
  pkill -f 'firefox.*ff-rdp-profile' 2>/dev/null || true
}
trap cleanup EXIT

# Fresh Firefox — avoid cross-run state pollution.
pkill -f 'firefox.*ff-rdp-profile' 2>/dev/null || true
sleep 1
ff-rdp launch --headless --port "$PORT"
sleep 2

# Daemon path (no --no-daemon): the persistent daemon connection carries the
# emulation from `emulate` to the following `eval`. The first eval auto-starts
# the daemon.
DAEMON=(--port "$PORT" --timeout 10000)

ff-rdp "${DAEMON[@]}" navigate --allow-unsafe-urls 'data:text/html,<h1>iter-103 emulate dogfood</h1>'

# Baseline: system default is not dark in headless.
BEFORE=$(ff-rdp "${DAEMON[@]}" eval 'matchMedia("(prefers-color-scheme: dark)").matches' --jq '.results')
test "$BEFORE" = "false" || { echo "FAIL: baseline prefers-color-scheme dark=$BEFORE (expected false)" >&2; exit 1; }

# --- Color-scheme simulation ---
# String values compared via a jq equality expression so the output is a bare
# `true`/`false` (avoids the JSON quoting jq applies to raw string values).
APPLIED_OK=$(ff-rdp "${DAEMON[@]}" emulate --color-scheme dark --jq '.results.applied.colorSchemeSimulation == "dark"')
test "$APPLIED_OK" = "true" || { echo "FAIL: emulate envelope applied.colorSchemeSimulation is not dark" >&2; exit 1; }

AFTER=$(ff-rdp "${DAEMON[@]}" eval 'matchMedia("(prefers-color-scheme: dark)").matches' --jq '.results')
test "$AFTER" = "true" || { echo "FAIL: after emulate --color-scheme dark, dark media query matches=$AFTER (expected true)" >&2; exit 1; }

# --- User-agent override ---
ff-rdp "${DAEMON[@]}" emulate --user-agent 'ff-rdp-test/1.0' >/dev/null
UA_OK=$(ff-rdp "${DAEMON[@]}" eval 'navigator.userAgent' --jq '.results == "ff-rdp-test/1.0"')
test "$UA_OK" = "true" || { echo "FAIL: navigator.userAgent is not the override ff-rdp-test/1.0" >&2; exit 1; }

# --- Reset reverts color scheme to system default ---
ff-rdp "${DAEMON[@]}" emulate --reset >/dev/null
REVERTED=$(ff-rdp "${DAEMON[@]}" eval 'matchMedia("(prefers-color-scheme: dark)").matches' --jq '.results')
test "$REVERTED" = "false" || { echo "FAIL: after emulate --reset, dark media query matches=$REVERTED (expected false)" >&2; exit 1; }

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
echo "iter-103 dogfood: emulate color-scheme + user-agent + reset verified — $SENTINEL"
