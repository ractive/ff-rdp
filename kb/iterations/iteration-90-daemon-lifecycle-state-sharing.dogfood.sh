#!/usr/bin/env bash
# iter-90 dogfood gate — daemon lifecycle state sharing.
#
# Verifies that `launch` + `daemon stop` work without manual `kill -9`, and
# that `launch --replace` handles a prior instance started via `launch` (not
# `daemon start`).
#
# Run manually:
#   FF_RDP_LIVE_TESTS=1 bash kb/iterations/iteration-90-*.dogfood.sh
#
# Exit 0 on success; writes the sentinel at $FF_RDP_DOGFOOD_SENTINEL.
set -euo pipefail

# shellcheck source=kb/iterations/dogfood-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/dogfood-lib.sh"

SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?set by check-dogfood-script; run this script via: cargo run -p xtask -- check-dogfood-script <plan.md>}"
rm -f "$SENTINEL"

dogfood_init
PORT="$(dogfood_free_port)"

# No "clean slate" step: the port is this run's alone, so there is nothing
# lingering on it to clear, and nothing on any other port is ours to touch
# (iter-193).

# Helper: assert port is free within N seconds.
assert_port_free() {
    local port=$1 secs=$2 i
    for i in $(seq 1 "$secs"); do
        if ! nc -z 127.0.0.1 "$port" 2>/dev/null; then return 0; fi
        sleep 1
    done
    echo "FAIL: port $port still listening after ${secs}s" >&2
    return 1
}

# Helper: assert port is open within N seconds.
assert_port_open() {
    local port=$1 secs=$2 i
    for i in $(seq 1 "$secs"); do
        if nc -z 127.0.0.1 "$port" 2>/dev/null; then return 0; fi
        sleep 1
    done
    echo "FAIL: port $port not open after ${secs}s" >&2
    return 1
}

echo "--- Theme A: launch → daemon stop → port free → launch again ---"

# 1. Launch Firefox.
dogfood_launch "$PORT" || { echo "FAIL Theme A: launch failed" >&2; exit 1; }
assert_port_open "$PORT" 10 || exit 1
echo "  [ok] Firefox listening on port $PORT"

# 2. daemon stop (the bug: on origin/main this returned "not running").
# `|| true` keeps `set -e` from aborting the script on a non-zero exit so
# that the regression check below can run and emit a precise diagnostic.
STOP_OUT=$(ffrdp --port "$PORT" daemon stop) || true
echo "  daemon stop response: $STOP_OUT"

echo "$STOP_OUT" | grep -q '"not running"' && {
    echo "FAIL Theme A: daemon stop returned 'not running' — iter-90 regression" >&2
    exit 1
}

# Port must be free within 4s.
assert_port_free "$PORT" 4 || { echo "FAIL Theme A: port $PORT still listening after daemon stop" >&2; exit 1; }
echo "  [ok] port $PORT freed after daemon stop"

# 3. Re-launch on the same port must succeed.
dogfood_launch "$PORT" || { echo "FAIL Theme A: re-launch after daemon stop failed" >&2; exit 1; }
assert_port_open "$PORT" 10 || exit 1
echo "  [ok] re-launch succeeded on port $PORT"

echo "--- Theme A-replace: launch --replace handles prior instance ---"

# 4. launch --replace while an instance is already running.
dogfood_launch "$PORT" --replace || {
    echo "FAIL Theme A-replace: launch --replace failed" >&2; exit 1
}
assert_port_open "$PORT" 10 || exit 1
echo "  [ok] launch --replace succeeded on port $PORT"

# 5. Final cleanup via daemon stop. Anything it leaves behind is reaped by
#    dogfood_teardown, which knows the exact pids this run launched.
ffrdp --port "$PORT" daemon stop || true
assert_port_free "$PORT" 5 || echo "  [warn] port $PORT still listening; leaving it to dogfood_teardown" >&2

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
echo "iter-90 dogfood: all themes verified — $SENTINEL"
