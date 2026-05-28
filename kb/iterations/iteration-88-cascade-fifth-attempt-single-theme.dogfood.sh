#!/usr/bin/env bash
# iter-88 dogfood gate — cascade returns non-empty rules on a real site.
# Executed by `cargo run -p xtask -- check-dogfood-script <plan>`.
#
# Run manually:
#   FF_RDP_LIVE_TESTS=1 bash kb/iterations/iteration-88-*.dogfood.sh
set -euo pipefail

SENTINEL=/tmp/ff-rdp-iter-88-dogfood-ok
rm -f "$SENTINEL"

# Fresh Firefox — avoid cross-run state pollution.
pkill -f 'firefox.*ff-rdp-profile' || true
sleep 1
ff-rdp launch --headless --port 6000
sleep 2

# --- Theme A: cascade returns non-empty rules on tennis-sepp.ch ---
ff-rdp navigate https://tennis-sepp.ch
N_RULES=$(ff-rdp cascade 'h1' --prop color --jq '.results[0].rules | length')
test "$N_RULES" -ge 1 || { echo "FAIL Theme A: cascade rules=$N_RULES (expected >=1)" >&2; exit 1; }

pkill -f 'firefox.*ff-rdp-profile' || true

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
echo "iter-88 dogfood: cascade verified — $SENTINEL"
