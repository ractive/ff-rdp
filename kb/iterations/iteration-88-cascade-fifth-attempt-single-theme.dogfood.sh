#!/usr/bin/env bash
# iter-88 dogfood gate — cascade returns non-empty rules on a real site.
# Executed by `cargo run -p xtask -- check-dogfood-script <plan>`.
#
# Run manually:
#   FF_RDP_LIVE_TESTS=1 bash kb/iterations/iteration-88-*.dogfood.sh
set -euo pipefail

# shellcheck source=kb/iterations/dogfood-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/dogfood-lib.sh"

SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?set by check-dogfood-script; run this script via: cargo run -p xtask -- check-dogfood-script <plan.md>}"
rm -f "$SENTINEL"

dogfood_init
PORT="$(dogfood_free_port)"

dogfood_launch "$PORT"
sleep 2

# --- Theme A: cascade returns non-empty rules on tennis-sepp.ch ---
ffrdp --port "$PORT" navigate https://tennis-sepp.ch
N_RULES=$(ffrdp --port "$PORT" cascade 'h1' --prop color --jq '.results[0].rules | length')
test "$N_RULES" -ge 1 || { echo "FAIL Theme A: cascade rules=$N_RULES (expected >=1)" >&2; exit 1; }

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
echo "iter-88 dogfood: cascade verified — $SENTINEL"
