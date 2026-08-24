#!/usr/bin/env bash
# iter-103 dogfood gate — `emulate` (target-configuration actor).
# AC slug: dogfood_script_full_run_iter_103 (exits 0 and writes the sentinel below)
#
# Exercises the emulate command end to end against a real launched Firefox over
# the daemon path (so emulation persists across separate CLI invocations):
#   1. emulate --color-scheme dark  → prefers-color-scheme: dark matches
#   2. emulate --user-agent <S>     → navigator.userAgent equals the override
#   3. emulate --reset              → color scheme reverts to system default
#
# Run manually:
#   FF_RDP_LIVE_TESTS=1 bash kb/iterations/iteration-103-target-configuration-cli.dogfood.sh
set -euo pipefail

# shellcheck source=kb/iterations/dogfood-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/dogfood-lib.sh"

SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?set by check-dogfood-script; run this script via: cargo run -p xtask -- check-dogfood-script <plan.md>}"
rm -f "$SENTINEL"

dogfood_init
PORT="$(dogfood_free_port)"

dogfood_launch "$PORT"
sleep 2

# Daemon path (no --no-daemon): the persistent daemon connection carries the
# emulation from `emulate` to the following `eval`. The first eval auto-starts
# the daemon.
DAEMON=(--port "$PORT" --timeout 10000)

ffrdp "${DAEMON[@]}" navigate --allow-unsafe-urls 'data:text/html,<h1>iter-103 emulate dogfood</h1>'

# Baseline: system default is not dark in headless.
BEFORE=$(ffrdp "${DAEMON[@]}" eval 'matchMedia("(prefers-color-scheme: dark)").matches' --jq '.results')
test "$BEFORE" = "false" || { echo "FAIL: baseline prefers-color-scheme dark=$BEFORE (expected false)" >&2; exit 1; }

# --- Color-scheme simulation ---
# String values compared via a jq equality expression so the output is a bare
# `true`/`false` (avoids the JSON quoting jq applies to raw string values).
APPLIED_OK=$(ffrdp "${DAEMON[@]}" emulate --color-scheme dark --jq '.results.applied.colorSchemeSimulation == "dark"')
test "$APPLIED_OK" = "true" || { echo "FAIL: emulate envelope applied.colorSchemeSimulation is not dark" >&2; exit 1; }

AFTER=$(ffrdp "${DAEMON[@]}" eval 'matchMedia("(prefers-color-scheme: dark)").matches' --jq '.results')
test "$AFTER" = "true" || { echo "FAIL: after emulate --color-scheme dark, dark media query matches=$AFTER (expected true)" >&2; exit 1; }

# --- User-agent override ---
ffrdp "${DAEMON[@]}" emulate --user-agent 'ff-rdp-test/1.0' >/dev/null
UA_OK=$(ffrdp "${DAEMON[@]}" eval 'navigator.userAgent' --jq '.results == "ff-rdp-test/1.0"')
test "$UA_OK" = "true" || { echo "FAIL: navigator.userAgent is not the override ff-rdp-test/1.0" >&2; exit 1; }

# --- Reset reverts color scheme to system default ---
ffrdp "${DAEMON[@]}" emulate --reset >/dev/null
REVERTED=$(ffrdp "${DAEMON[@]}" eval 'matchMedia("(prefers-color-scheme: dark)").matches' --jq '.results')
test "$REVERTED" = "false" || { echo "FAIL: after emulate --reset, dark media query matches=$REVERTED (expected false)" >&2; exit 1; }

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
echo "iter-103 dogfood: emulate color-scheme + user-agent + reset verified — $SENTINEL"
