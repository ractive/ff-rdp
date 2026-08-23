#!/usr/bin/env bash
# iter-89 dogfood gate — screenshot produces a valid PNG on FF 151.
# Executed by `cargo run -p xtask -- check-dogfood-script <plan>`.
#
# Run manually:
#   FF_RDP_LIVE_TESTS=1 bash kb/iterations/iteration-89-*.dogfood.sh
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

# --- Theme A: screenshot writes a valid PNG ---
ffrdp --port "$PORT" navigate https://example.com
SHOT="$WORK/iter89-shot.png"
ffrdp --port "$PORT" screenshot -o "$SHOT" || { echo "FAIL Theme A: screenshot command exited non-zero" >&2; exit 1; }
test -s "$SHOT" || { echo "FAIL Theme A: screenshot file is empty or missing" >&2; exit 1; }
# Size threshold: > 1000 bytes (a real example.com capture is ~10s of KB).
SZ=$(wc -c < "$SHOT" | tr -d ' ')
test "$SZ" -gt 1000 || { echo "FAIL Theme A: screenshot too small ($SZ bytes)" >&2; exit 1; }
# PNG magic bytes check.
file "$SHOT" | grep -q 'PNG image' || { echo "FAIL Theme A: not a PNG ($(file "$SHOT"))" >&2; exit 1; }

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
echo "iter-89 dogfood: screenshot verified — $SENTINEL"
