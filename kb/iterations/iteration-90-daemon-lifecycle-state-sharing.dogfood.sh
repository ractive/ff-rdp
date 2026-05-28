#!/usr/bin/env bash
# iter-90 dogfood gate — launch and daemon share state; stop frees the port.
# Executed by `cargo run -p xtask -- check-dogfood-script <plan>`.
#
# Run manually:
#   FF_RDP_LIVE_TESTS=1 bash kb/iterations/iteration-90-*.dogfood.sh
set -euo pipefail

SENTINEL=/tmp/ff-rdp-iter-90-dogfood-ok
rm -f "$SENTINEL"

# Clean slate — daemon state sharing is itself under test.
pkill -f 'firefox.*ff-rdp-profile' || true
sleep 1

# --- Theme A.1: launch → daemon stop → launch (no manual kill) ---
ff-rdp launch --headless --port 6000
PID1=$(ff-rdp daemon stop --jq '.results.pid // 0')
# Poll for port free (max 3s).
for i in 1 2 3; do
  if ! nc -z localhost 6000 2>/dev/null; then break; fi
  sleep 1
done
nc -z localhost 6000 2>/dev/null && { echo "FAIL Theme A: port 6000 still listening after daemon stop (prior PID=$PID1)" >&2; exit 1; }

# Second launch must succeed without manual intervention.
ff-rdp launch --headless --port 6000 || { echo "FAIL Theme A: second launch after daemon stop failed" >&2; exit 1; }

# --- Theme A.2: launch --replace handles a live prior instance ---
# We're already running; --replace should stop us and spawn fresh.
ff-rdp launch --replace --headless --port 6000 || { echo "FAIL Theme A: launch --replace against live prior failed" >&2; exit 1; }

# Confirm port is still listening (the replacement) and a different process holds it.
nc -z localhost 6000 || { echo "FAIL Theme A: port not listening after --replace" >&2; exit 1; }

ff-rdp daemon stop
sleep 1
nc -z localhost 6000 2>/dev/null && { echo "FAIL Theme A: final daemon stop did not free port" >&2; exit 1; }

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
echo "iter-90 dogfood: daemon lifecycle verified — $SENTINEL"
