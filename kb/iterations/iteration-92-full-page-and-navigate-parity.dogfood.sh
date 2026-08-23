#!/usr/bin/env bash
# iter-92 dogfood gate — screenshot --full-page regression + navigate freshness.
# Executed by `cargo run -p xtask -- check-dogfood-script <plan>`.
#
# Run manually:
#   FF_RDP_LIVE_TESTS=1 bash kb/iterations/iteration-92-full-page-and-navigate-parity.dogfood.sh
set -euo pipefail

# shellcheck source=kb/iterations/dogfood-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/dogfood-lib.sh"

SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?set by check-dogfood-script; run this script via: cargo run -p xtask -- check-dogfood-script <plan.md>}"
rm -f "$SENTINEL"

dogfood_init
WORK="$(dogfood_workdir)"
PORT="$(dogfood_free_port)"

dogfood_launch "$PORT"
sleep 2

TALL_URL='data:text/html,<html><body style="height:4000px;background:linear-gradient(to bottom,red,blue)">x</body></html>'

# --- Theme A: --full-page produces a PNG taller than viewport ---

ffrdp --port "$PORT" navigate --allow-unsafe-urls "$TALL_URL"

SHOT_VP="$WORK/viewport.png"
SHOT_FP="$WORK/fullpage.png"

# Viewport capture.
ffrdp --port "$PORT" screenshot -o "$SHOT_VP" || { echo "FAIL Theme A: viewport screenshot failed" >&2; exit 1; }
test -s "$SHOT_VP" || { echo "FAIL Theme A: viewport screenshot file empty" >&2; exit 1; }

# Full-page capture.
ffrdp --port "$PORT" screenshot --full-page -o "$SHOT_FP" || { echo "FAIL Theme A: full-page screenshot failed" >&2; exit 1; }
test -s "$SHOT_FP" || { echo "FAIL Theme A: full-page screenshot file empty" >&2; exit 1; }

# Both must be valid PNGs.
file "$SHOT_VP" | grep -q 'PNG image' || { echo "FAIL Theme A: viewport not a PNG" >&2; exit 1; }
file "$SHOT_FP" | grep -q 'PNG image' || { echo "FAIL Theme A: full-page not a PNG" >&2; exit 1; }

# Full-page file must be larger than viewport file (tall page = more rows).
VP_SZ=$(wc -c < "$SHOT_VP" | tr -d ' ')
FP_SZ=$(wc -c < "$SHOT_FP" | tr -d ' ')
if [ "$FP_SZ" -le "$VP_SZ" ]; then
  echo "FAIL Theme A: full-page size ($FP_SZ bytes) <= viewport size ($VP_SZ bytes) — --full-page flag was not honoured" >&2
  exit 1
fi

echo "Theme A OK: viewport=${VP_SZ}B full-page=${FP_SZ}B"

# --- Theme B: second navigate waits for a fresh commit ---

PAGE_A='data:text/html,A_PAGE'
PAGE_B='data:text/html,B_PAGE'

# First navigate.
ffrdp --port "$PORT" navigate --allow-unsafe-urls "$PAGE_A" || { echo "FAIL Theme B: first navigate failed" >&2; exit 1; }

# Second navigate — must not short-circuit with elapsed_ms:0.
NAV2_OUT=$(ffrdp --port "$PORT" navigate --allow-unsafe-urls "$PAGE_B") || { echo "FAIL Theme B: second navigate failed" >&2; exit 1; }
ELAPSED=$(echo "$NAV2_OUT" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('results',d).get('elapsed_ms',0))" 2>/dev/null || echo "0")

if [ "$ELAPSED" -le 0 ] 2>/dev/null; then
  echo "FAIL Theme B: second navigate elapsed_ms=$ELAPSED (expected > 0); stale dom-complete short-circuit may still be present" >&2
  exit 1
fi

echo "Theme B OK: second navigate elapsed_ms=$ELAPSED"

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
echo "iter-92 dogfood: full-page regression fixed + navigate freshness verified — $SENTINEL"
