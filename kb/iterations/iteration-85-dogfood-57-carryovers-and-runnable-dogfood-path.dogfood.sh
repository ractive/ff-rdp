#!/usr/bin/env bash
# iter-85 dogfood gate — reproduces every user-visible fix this iteration claims.
# Executed by `cargo run -p xtask -- check-dogfood-script <plan>`.
# Must exit 0 AND write the sentinel on the final line.
#
# Run manually:
#   FF_RDP_LIVE_TESTS=1 bash kb/iterations/iteration-85-*.dogfood.sh
set -euo pipefail

# shellcheck source=kb/iterations/dogfood-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/dogfood-lib.sh"

SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?set by check-dogfood-script; run this script via: cargo run -p xtask -- check-dogfood-script <plan.md>}"
rm -f "$SENTINEL"

dogfood_init
WORK="$(dogfood_workdir)"
PORT="$(dogfood_free_port)"

# A private port and a private $FF_RDP_HOME give this run its own Firefox and
# its own profile root, so there is no cross-run state to clear and no sibling
# agent's browser to disturb (iter-193).
dogfood_launch "$PORT"
sleep 2

# --- Theme A: cascade returns non-empty rules on a real site ---
ffrdp --port "$PORT" navigate https://tennis-sepp.ch
N_RULES=$(ffrdp --port "$PORT" cascade 'h1' --prop color --jq '.results[0].rules | length')
test "$N_RULES" -ge 1 || { echo "FAIL Theme A: cascade rules=$N_RULES" >&2; exit 1; }

# --- Theme B: screenshot on FF 151 produces a valid PNG ---
ffrdp --port "$PORT" navigate https://example.com
ffrdp --port "$PORT" screenshot -o "$WORK/shot.png"
test -s "$WORK/shot.png" || { echo "FAIL Theme B: screenshot empty" >&2; exit 1; }
file "$WORK/shot.png" | grep -q 'PNG image' || { echo "FAIL Theme B: not a PNG" >&2; exit 1; }

# --- Theme C: default navigate completes in < 3000 ms on example.com ---
START=$(python3 -c 'import time; print(int(time.time()*1000))')
ffrdp --port "$PORT" navigate https://example.com >/dev/null
END=$(python3 -c 'import time; print(int(time.time()*1000))')
ELAPSED=$((END - START))
test "$ELAPSED" -lt 3000 || { echo "FAIL Theme C: navigate took ${ELAPSED}ms (>=3000)" >&2; exit 1; }

# --- Theme K-followup: --timeout alias emits deprecation on stderr ---
ffrdp --port "$PORT" navigate https://example.com
ffrdp --port "$PORT" wait --selector 'body' --timeout 1000 2>"$WORK/wait.err" || true
grep -qi 'deprecat' "$WORK/wait.err" || { echo "FAIL Theme K: no deprecation warning" >&2; exit 1; }

# --- Theme L: cookies surfaces Set-Cookie response header ---
ffrdp --port "$PORT" navigate 'https://httpbin.org/cookies/set?session=abc123'
ffrdp --port "$PORT" cookies --jq '[.results[].name] | contains(["session"])' | grep -q '^true$' \
  || { echo "FAIL Theme L: session cookie not surfaced" >&2; exit 1; }

# --- Theme M: check-dogfood-script gate exists and rejects missing sentinel ---
# (self-referential smoke — verified separately by xtask integration test)

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
echo "iter-85 dogfood: all themes verified — $SENTINEL"
