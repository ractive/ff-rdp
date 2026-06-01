#!/usr/bin/env bash
# iter-95 dogfood gate — session-60 follow-ups.
#
# Exercises all three themes:
#   A — daemon stop process-group kill (port freed after SIGKILL on pgid)
#   B — cascade --prop populates computed field (same value as standalone computed)
#   C — doctor binary-staleness check
#
# Run manually:
#   FF_RDP_LIVE_TESTS=1 bash kb/iterations/iteration-95-session-60-followups.dogfood.sh
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

SENTINEL=/tmp/ff-rdp-iter-95-dogfood-ok
rm -f "$SENTINEL"

PORT=6000

cleanup() {
  ff-rdp --port "$PORT" daemon stop 2>/dev/null || true
  pkill -f 'firefox.*ff-rdp-profile' 2>/dev/null || true
}
trap cleanup EXIT

# Kill any stale Firefox.
pkill -f 'firefox.*ff-rdp-profile' 2>/dev/null || true

echo "=== Launching headless Firefox on port $PORT ==="
ff-rdp launch --headless --port "$PORT"
sleep 2

ARGS="--port $PORT --no-daemon --allow-unsafe-urls"

# ---------------------------------------------------------------------------
# Theme A — daemon stop process-group kill
#
# Navigate to a JS-heavy page (about:blank with a quick inline fetch) so
# Firefox spins up content/GPU child processes. Then `daemon stop` must free
# the port within 15 s via the new pgid SIGKILL escalation step.
# ---------------------------------------------------------------------------
echo ""
echo "=== Theme A — daemon stop process-group kill ==="

# Navigate to a page that triggers multi-process architecture.
ff-rdp $ARGS navigate "data:text/html,<script>fetch('data:,').catch(()=>{})</script><h1>pgid test</h1>"
sleep 2

# Confirm port is open before stop.
if ! nc -z 127.0.0.1 "$PORT" 2>/dev/null; then
  echo "FAIL: Theme A — port $PORT not open before daemon stop"
  exit 1
fi

START_TIME=$(date +%s)
ff-rdp --port "$PORT" daemon stop
END_TIME=$(date +%s)
ELAPSED=$((END_TIME - START_TIME))

echo "  daemon stop completed in ${ELAPSED}s"

# Port must be free (connection refused) within 3 s.
PORT_FREE=false
for i in $(seq 1 30); do
  if ! nc -z 127.0.0.1 "$PORT" 2>/dev/null; then
    PORT_FREE=true
    break
  fi
  sleep 0.1
done

if [ "$PORT_FREE" != "true" ]; then
  echo "FAIL: Theme A — port $PORT still listening after daemon stop (pgid escalation didn't work)"
  exit 1
fi

if [ "$ELAPSED" -gt 15 ]; then
  echo "FAIL: Theme A — daemon stop took ${ELAPSED}s, exceeds 15 s ceiling"
  exit 1
fi
echo "PASS: Theme A — port $PORT freed after daemon stop in ${ELAPSED}s"

# Relaunch for next theme.
echo "=== Re-launching Firefox for Theme B ==="
ff-rdp launch --headless --port "$PORT"
sleep 2

# ---------------------------------------------------------------------------
# Theme B — cascade --prop populates computed field
#
# Navigate to a page with a styled h1 that inherits color from body.
# assert cascade --prop color returns a non-null computed field matching
# the standalone computed command's output.
# ---------------------------------------------------------------------------
echo ""
echo "=== Theme B — cascade --prop populates computed field ==="

ff-rdp $ARGS navigate "data:text/html,<body style='color:red'><h1>cascade test</h1></body>"
sleep 1

CASCADE_JSON=$(ff-rdp $ARGS cascade h1 --prop color)
CASCADE_COMPUTED=$(echo "$CASCADE_JSON" | jq -r '.results[0].computed // "null"')

COMPUTED_JSON=$(ff-rdp $ARGS computed h1 --prop color)
COMPUTED_VALUE=$(echo "$COMPUTED_JSON" | jq -r '.results[0].computed.color // "null"')

echo "  cascade computed: $CASCADE_COMPUTED"
echo "  standalone computed: $COMPUTED_VALUE"

if [ "$CASCADE_COMPUTED" = "null" ] || [ -z "$CASCADE_COMPUTED" ]; then
  echo "FAIL: Theme B — cascade --prop color returned null computed (should match standalone computed)"
  exit 1
fi

if [ "$CASCADE_COMPUTED" != "$COMPUTED_VALUE" ]; then
  echo "FAIL: Theme B — cascade computed ($CASCADE_COMPUTED) != standalone computed ($COMPUTED_VALUE)"
  exit 1
fi
echo "PASS: Theme B — cascade --prop computed matches standalone computed ($CASCADE_COMPUTED)"

# ---------------------------------------------------------------------------
# Theme C — doctor binary-staleness check
#
# Run doctor from within the repo. If the installed binary's SHA differs
# from HEAD, the binary_staleness check should warn. If they match (dev
# flow), it passes. Either way the check must be present in the output.
# ---------------------------------------------------------------------------
echo ""
echo "=== Theme C — doctor binary-staleness check ==="

# Stop Firefox so doctor doesn't trip on an unexpected tab state.
ff-rdp --port "$PORT" daemon stop 2>/dev/null || true

# Run doctor from within the repo root (so git rev-parse HEAD works).
DOCTOR_JSON=$(cd "$REPO_ROOT" && ff-rdp --port "$PORT" doctor 2>/dev/null || true)

STALENESS_STATUS=$(echo "$DOCTOR_JSON" | jq -r '
  .results[] | select(.name == "binary_staleness") | .status
' 2>/dev/null || echo "missing")

echo "  binary_staleness status: $STALENESS_STATUS"

if [ "$STALENESS_STATUS" = "missing" ]; then
  echo "FAIL: Theme C — doctor output missing binary_staleness check"
  exit 1
fi

if [ "$STALENESS_STATUS" != "pass" ] && [ "$STALENESS_STATUS" != "warn" ]; then
  echo "FAIL: Theme C — binary_staleness should be 'pass' or 'warn', got: $STALENESS_STATUS"
  exit 1
fi
echo "PASS: Theme C — binary_staleness check present (status=$STALENESS_STATUS)"

# ---------------------------------------------------------------------------
echo ""
echo "=== All themes PASSED ==="
date -u "+%Y-%m-%dT%H:%M:%SZ ok" > "$SENTINEL"
echo "written: $SENTINEL"
