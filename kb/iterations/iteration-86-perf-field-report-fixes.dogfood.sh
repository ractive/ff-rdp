#!/usr/bin/env bash
# iter-86 dogfood gate — reproduces every fix from the perf field report.
# Executed by `cargo run -p xtask -- check-dogfood-script <plan>`.
#
# Run manually:
#   FF_RDP_LIVE_TESTS=1 bash kb/iterations/iteration-86-*.dogfood.sh
set -euo pipefail

# shellcheck source=kb/iterations/dogfood-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/dogfood-lib.sh"

SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?set by check-dogfood-script; run this script via: cargo run -p xtask -- check-dogfood-script <plan.md>}"
rm -f "$SENTINEL"

dogfood_init
PORT="$(dogfood_free_port)"

# `daemon stop` is itself under test here, so the run needs a port nobody else
# is on. A private port plus a private $FF_RDP_HOME gives that without touching
# any Firefox this script did not start (iter-193).

# --- Theme A: daemon stop frees the port; relaunch works without kill -9 ---
dogfood_launch "$PORT"
ffrdp --port "$PORT" daemon stop
# Port must be free within 3s
for i in 1 2 3; do
  if ! nc -z localhost "$PORT" 2>/dev/null; then break; fi
  sleep 1
done
nc -z localhost "$PORT" 2>/dev/null && { echo "FAIL Theme A: port $PORT still listening after daemon stop" >&2; exit 1; }
dogfood_launch "$PORT" || { echo "FAIL Theme A: relaunch after daemon stop failed" >&2; exit 1; }

# Theme A-followup: --replace handles a stuck instance
# (We're already running — simulate by relaunching with --replace)
dogfood_launch "$PORT" --replace || { echo "FAIL Theme A: launch --replace failed" >&2; exit 1; }
ffrdp --port "$PORT" daemon stop
sleep 1

# --- Theme B: lcp_note is headless-state-honest + mentions Firefox limitation ---
dogfood_launch_headful "$PORT"  # non-headless
ffrdp --port "$PORT" navigate https://example.com
NOTE=$(ffrdp --port "$PORT" perf audit --jq '.results.vitals.lcp_note // .meta.lcp_note // ""')
# Use anchored pattern: the note should NOT claim "headless Firefox" when launched non-headless.
# "regardless of headless mode" must NOT match — so we test for the phrase "headless Firefox".
echo "$NOTE" | grep -qiE '(^|[^a-z])headless Firefox' && { echo "FAIL Theme B: lcp_note claims 'headless Firefox' after non-headless launch: $NOTE" >&2; exit 1; }
echo "$NOTE" | grep -qiE '(^|[^a-z])Firefox' || { echo "FAIL Theme B: lcp_note does not mention Firefox limitation: $NOTE" >&2; exit 1; }
ffrdp --port "$PORT" daemon stop
sleep 1

# --- Theme C: render-blocking filter excludes favicons + non-blocking rels ---
dogfood_launch "$PORT"
ffrdp --port "$PORT" navigate https://example.com
RB=$(ffrdp --port "$PORT" perf audit --jq '.results.render_blocking // [] | map(.url) | join(" ")')
echo "$RB" | grep -qi 'favicon\|\.ico' && { echo "FAIL Theme C: render_blocking contains favicon: $RB" >&2; exit 1; }

# --- Theme D: --jq missing-path policy ---
# Default: silent omit, exit 0
OUT=$(ffrdp --port "$PORT" perf audit --jq '.results.does_not_exist_xyz' 2>/dev/null) || { echo "FAIL Theme D: default missing-path exited non-zero" >&2; exit 1; }
test -z "$OUT" || test "$OUT" = "null" && {
  # transitional: accept empty OR null until silent-omit lands; but flag null
  if [ "$OUT" = "null" ]; then
    echo "FAIL Theme D: default missing-path emitted 'null', expected empty" >&2; exit 1
  fi
}
# Strict: non-zero exit, stderr mentions "not found"
set +e
ERR=$(ffrdp --port "$PORT" perf audit --jq-strict --jq '.results.does_not_exist_xyz' 2>&1 >/dev/null)
EC=$?
set -e
test "$EC" -ne 0 || { echo "FAIL Theme D: --jq-strict missing-path exited 0" >&2; exit 1; }
echo "$ERR" | grep -qi 'not found' || { echo "FAIL Theme D: --jq-strict stderr missing 'not found': $ERR" >&2; exit 1; }

# --- Theme E: perf audit --help mentions Lighthouse for LCP ---
ffrdp perf audit --help 2>&1 | grep -qi 'lighthouse' || { echo "FAIL Theme E: --help does not mention Lighthouse" >&2; exit 1; }

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
echo "iter-86 dogfood: all themes verified — $SENTINEL"
