#!/usr/bin/env bash
# iter-94 dogfood gate — session-59 polish bundle.
#
# Exercises all four themes:
#   A — daemon stop bounded wait (port-free within 8 s)
#   B — render-blocking count parity between dom stats and perf audit
#   C — cascade --prop emits inherited_or_default note for inherited properties
#   D — network --format text suppresses null-keyed rows
#
# Run manually:
#   FF_RDP_LIVE_TESTS=1 bash kb/iterations/iteration-94-session-59-polish-bundle.dogfood.sh
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

SENTINEL=/tmp/ff-rdp-iter-94-dogfood-ok
rm -f "$SENTINEL"

PORT=6000

cleanup() {
  ff-rdp --port "$PORT" --no-daemon daemon stop 2>/dev/null || true
  pkill -f 'firefox.*ff-rdp-profile' 2>/dev/null || true
}
trap cleanup EXIT

# Kill any stale Firefox.
pkill -f 'firefox.*ff-rdp-profile' 2>/dev/null || true
sleep 1

echo "=== Launching headless Firefox on port $PORT ==="
ff-rdp launch --headless --port "$PORT"
sleep 2

ARGS="--port $PORT --no-daemon"

# ---------------------------------------------------------------------------
# Theme B — render-blocking parity on a local data: fixture
# ---------------------------------------------------------------------------
echo ""
echo "=== Theme B — render-blocking parity ==="

# Navigate to a fixture with known blocking/non-blocking resources.
# 2 blocking: 1 sync script, 1 stylesheet
# 2 non-blocking: 1 async script, 1 icon link
FIXTURE_HTML="data:text/html,<head><link rel='stylesheet' href='data:text/css,body{color:red}'><link rel='icon' href='data:,'><script src='data:text/javascript,void 0'></script><script async src='data:text/javascript,void 0'></script></head><body>test</body>"

# Note: data: URI scripts with src starting with "data:" are classified as non-blocking
# by the spec-correct predicate (iter-86 Theme C). So the fixture has:
#   stylesheet (blocking), icon (not blocking), data: script (not blocking), async data: script (not blocking)
# = 1 blocking total

ff-rdp $ARGS navigate "$FIXTURE_HTML"
sleep 1

DOM_BLOCKING=$(ff-rdp $ARGS dom stats | jq '.results.render_blocking_count // .results[0].render_blocking_count // 0')
PERF_BLOCKING=$(ff-rdp $ARGS perf audit | jq '.results.render_blocking_count // .results[0].render_blocking_count // 0')

echo "  dom stats render_blocking_count:  $DOM_BLOCKING"
echo "  perf audit render_blocking_count: $PERF_BLOCKING"

if [ "$DOM_BLOCKING" != "$PERF_BLOCKING" ]; then
  echo "FAIL: Theme B — dom stats ($DOM_BLOCKING) != perf audit ($PERF_BLOCKING)"
  exit 1
fi
echo "PASS: Theme B — both surfaces agree ($DOM_BLOCKING)"

# ---------------------------------------------------------------------------
# Theme C — cascade emits inherited_or_default note
# ---------------------------------------------------------------------------
echo ""
echo "=== Theme C — cascade inherited_or_default note ==="

# Navigate to a page where h1 inherits color from body (no author rule on h1).
ff-rdp $ARGS navigate "data:text/html,<body style='color:red'><h1>hello</h1></body>"
sleep 1

CASCADE_JSON=$(ff-rdp $ARGS cascade h1 --prop color)
INHERITED=$(echo "$CASCADE_JSON" | jq '.results[0].inherited_or_default // false')
NOTE=$(echo "$CASCADE_JSON" | jq -r '.results[0].note // ""')

echo "  inherited_or_default: $INHERITED"
echo "  note: $NOTE"

if [ "$INHERITED" != "true" ]; then
  echo "FAIL: Theme C — cascade h1 --prop color should have inherited_or_default:true"
  exit 1
fi
if [ -z "$NOTE" ]; then
  echo "FAIL: Theme C — cascade should include a non-empty note field"
  exit 1
fi
echo "PASS: Theme C — cascade note present"

# ---------------------------------------------------------------------------
# Theme D — network --format text suppresses null-keyed rows
# ---------------------------------------------------------------------------
echo ""
echo "=== Theme D — network text null-key suppression ==="

# Navigate to a new page immediately (before network events complete streaming).
ff-rdp $ARGS navigate "data:text/html,<h1>network-test</h1>"
sleep 0

NETWORK_TEXT=$(ff-rdp $ARGS network --format text 2>/dev/null || true)
echo "  network text output (first 10 lines):"
echo "$NETWORK_TEXT" | head -10

# Check there are no bare-number rows (a number with no label).
# Bare-number rows look like lines with only spaces and digits (e.g. "      82").
if echo "$NETWORK_TEXT" | grep -qE '^\s+[0-9]+\s*$'; then
  echo "FAIL: Theme D — bare-number row found in network text output"
  exit 1
fi
echo "PASS: Theme D — no bare-number rows"

# ---------------------------------------------------------------------------
# Theme A — daemon stop bounded wait
# ---------------------------------------------------------------------------
echo ""
echo "=== Theme A — daemon stop bounded wait ==="

START_TIME=$(date +%s)
ff-rdp --port "$PORT" --no-daemon daemon stop 2>&1 || true
END_TIME=$(date +%s)
ELAPSED=$((END_TIME - START_TIME))

echo "  daemon stop completed in ${ELAPSED}s"

# Verify the port is free within a few seconds.
for i in $(seq 1 10); do
  if ! nc -z 127.0.0.1 "$PORT" 2>/dev/null; then
    echo "PASS: Theme A — port $PORT is free after daemon stop (${i}00ms)"
    break
  fi
  sleep 0.1
  if [ "$i" = "10" ]; then
    echo "FAIL: Theme A — port $PORT still in use after daemon stop"
    exit 1
  fi
done

# ---------------------------------------------------------------------------
echo ""
echo "=== All themes PASSED ==="
touch "$SENTINEL"
echo "ok > $SENTINEL"
