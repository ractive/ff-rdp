#!/usr/bin/env bash
# iter-89 dogfood gate — screenshot produces a valid PNG on FF 151.
# Executed by `cargo run -p xtask -- check-dogfood-script <plan>`.
#
# Run manually:
#   FF_RDP_LIVE_TESTS=1 bash kb/iterations/iteration-89-*.dogfood.sh
set -euo pipefail

SENTINEL=/tmp/ff-rdp-iter-89-dogfood-ok
rm -f "$SENTINEL"

# Fresh Firefox.
pkill -f 'firefox.*ff-rdp-profile' || true
sleep 1
ff-rdp launch --headless --port 6000
sleep 2

# --- Theme A: screenshot writes a valid PNG ---
ff-rdp navigate https://example.com
SHOT=/tmp/iter89-shot.png
rm -f "$SHOT"
ff-rdp screenshot -o "$SHOT" || { echo "FAIL Theme A: screenshot command exited non-zero" >&2; exit 1; }
test -s "$SHOT" || { echo "FAIL Theme A: screenshot file is empty or missing" >&2; exit 1; }
# Size threshold: > 1000 bytes (a real example.com capture is ~10s of KB).
SZ=$(wc -c < "$SHOT" | tr -d ' ')
test "$SZ" -gt 1000 || { echo "FAIL Theme A: screenshot too small ($SZ bytes)" >&2; exit 1; }
# PNG magic bytes check.
file "$SHOT" | grep -q 'PNG image' || { echo "FAIL Theme A: not a PNG ($(file "$SHOT"))" >&2; exit 1; }

pkill -f 'firefox.*ff-rdp-profile' || true

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
echo "iter-89 dogfood: screenshot verified — $SENTINEL"
