#!/usr/bin/env bash
# iter-85 dogfood gate — reproduces every user-visible fix this iteration claims.
# Executed by `cargo run -p xtask -- check-dogfood-script <plan>`.
# Must exit 0 AND write the sentinel on the final line.
#
# Run manually:
#   FF_RDP_LIVE_TESTS=1 bash kb/iterations/iteration-85-*.dogfood.sh
set -euo pipefail

SENTINEL=/tmp/ff-rdp-iter-85-dogfood-ok
rm -f "$SENTINEL"

# Fresh Firefox — avoid cross-run state pollution
pkill -f 'firefox.*ff-rdp-profile' || true
sleep 1
ff-rdp launch --headless --port 6000
sleep 2

# --- Theme A: cascade returns non-empty rules on a real site ---
ff-rdp navigate https://tennis-sepp.ch
N_RULES=$(ff-rdp cascade 'h1' --prop color --jq '.results[0].rules | length')
test "$N_RULES" -ge 1 || { echo "FAIL Theme A: cascade rules=$N_RULES" >&2; exit 1; }

# --- Theme B: screenshot on FF 151 produces a valid PNG ---
ff-rdp navigate https://example.com
rm -f /tmp/iter85-shot.png
ff-rdp screenshot -o /tmp/iter85-shot.png
test -s /tmp/iter85-shot.png || { echo "FAIL Theme B: screenshot empty" >&2; exit 1; }
file /tmp/iter85-shot.png | grep -q 'PNG image' || { echo "FAIL Theme B: not a PNG" >&2; exit 1; }

# --- Theme C: default navigate completes in < 3000 ms on example.com ---
START=$(python3 -c 'import time; print(int(time.time()*1000))')
ff-rdp navigate https://example.com >/dev/null
END=$(python3 -c 'import time; print(int(time.time()*1000))')
ELAPSED=$((END - START))
test "$ELAPSED" -lt 3000 || { echo "FAIL Theme C: navigate took ${ELAPSED}ms (>=3000)" >&2; exit 1; }

# --- Theme K-followup: --timeout alias emits deprecation on stderr ---
ff-rdp navigate https://example.com
ff-rdp wait --selector 'body' --timeout 1000 2>/tmp/iter85-wait.err || true
grep -qi 'deprecat' /tmp/iter85-wait.err || { echo "FAIL Theme K: no deprecation warning" >&2; exit 1; }

# --- Theme L: cookies surfaces Set-Cookie response header ---
ff-rdp navigate 'https://httpbin.org/cookies/set?session=abc123'
ff-rdp cookies --jq '[.results[].name] | contains(["session"])' | grep -q '^true$' \
  || { echo "FAIL Theme L: session cookie not surfaced" >&2; exit 1; }

# --- Theme M: check-dogfood-script gate exists and rejects missing sentinel ---
# (self-referential smoke — verified separately by xtask integration test)

pkill -f 'firefox.*ff-rdp-profile' || true

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
echo "iter-85 dogfood: all themes verified — $SENTINEL"
