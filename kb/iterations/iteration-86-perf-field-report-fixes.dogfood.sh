#!/usr/bin/env bash
# iter-86 dogfood gate — reproduces every fix from the perf field report.
# Executed by `cargo run -p xtask -- check-dogfood-script <plan>`.
#
# Run manually:
#   FF_RDP_LIVE_TESTS=1 bash kb/iterations/iteration-86-*.dogfood.sh
set -euo pipefail

SENTINEL=/tmp/ff-rdp-iter-86-dogfood-ok
rm -f "$SENTINEL"

# Clean slate — daemon stop is itself under test, don't start with a stuck Firefox
pkill -f 'firefox.*ff-rdp-profile' || true
sleep 1

# --- Theme A: daemon stop frees the port; relaunch works without kill -9 ---
ff-rdp launch --headless --port 6000
ff-rdp daemon stop
# Port must be free within 3s
for i in 1 2 3; do
  if ! nc -z localhost 6000 2>/dev/null; then break; fi
  sleep 1
done
nc -z localhost 6000 2>/dev/null && { echo "FAIL Theme A: port 6000 still listening after daemon stop" >&2; exit 1; }
ff-rdp launch --headless --port 6000 || { echo "FAIL Theme A: relaunch after daemon stop failed" >&2; exit 1; }

# Theme A-followup: --replace handles a stuck instance
# (We're already running — simulate by relaunching with --replace)
ff-rdp launch --replace --headless --port 6000 || { echo "FAIL Theme A: launch --replace failed" >&2; exit 1; }
ff-rdp daemon stop
sleep 1

# --- Theme B: lcp_note is headless-state-honest + mentions Firefox limitation ---
ff-rdp launch --port 6000  # non-headless
ff-rdp navigate https://example.com
NOTE=$(ff-rdp perf audit --jq '.results.vitals.lcp_note // .meta.lcp_note // ""')
# Use anchored pattern: the note should NOT claim "headless Firefox" when launched non-headless.
# "regardless of headless mode" must NOT match — so we test for the phrase "headless Firefox".
echo "$NOTE" | grep -qiE '(^|[^a-z])headless Firefox' && { echo "FAIL Theme B: lcp_note claims 'headless Firefox' after non-headless launch: $NOTE" >&2; exit 1; }
echo "$NOTE" | grep -qiE '(^|[^a-z])Firefox' || { echo "FAIL Theme B: lcp_note does not mention Firefox limitation: $NOTE" >&2; exit 1; }
ff-rdp daemon stop
sleep 1

# --- Theme C: render-blocking filter excludes favicons + non-blocking rels ---
ff-rdp launch --headless --port 6000
ff-rdp navigate https://example.com
RB=$(ff-rdp perf audit --jq '.results.render_blocking // [] | map(.url) | join(" ")')
echo "$RB" | grep -qi 'favicon\|\.ico' && { echo "FAIL Theme C: render_blocking contains favicon: $RB" >&2; exit 1; }

# --- Theme D: --jq missing-path policy ---
# Default: silent omit, exit 0
OUT=$(ff-rdp perf audit --jq '.results.does_not_exist_xyz' 2>/dev/null) || { echo "FAIL Theme D: default missing-path exited non-zero" >&2; exit 1; }
test -z "$OUT" || test "$OUT" = "null" && {
  # transitional: accept empty OR null until silent-omit lands; but flag null
  if [ "$OUT" = "null" ]; then
    echo "FAIL Theme D: default missing-path emitted 'null', expected empty" >&2; exit 1
  fi
}
# Strict: non-zero exit, stderr mentions "not found"
set +e
ERR=$(ff-rdp perf audit --jq-strict --jq '.results.does_not_exist_xyz' 2>&1 >/dev/null)
EC=$?
set -e
test "$EC" -ne 0 || { echo "FAIL Theme D: --jq-strict missing-path exited 0" >&2; exit 1; }
echo "$ERR" | grep -qi 'not found' || { echo "FAIL Theme D: --jq-strict stderr missing 'not found': $ERR" >&2; exit 1; }

# --- Theme E: perf audit --help mentions Lighthouse for LCP ---
ff-rdp perf audit --help 2>&1 | grep -qi 'lighthouse' || { echo "FAIL Theme E: --help does not mention Lighthouse" >&2; exit 1; }

pkill -f 'firefox.*ff-rdp-profile' || true

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
echo "iter-86 dogfood: all themes verified — $SENTINEL"
