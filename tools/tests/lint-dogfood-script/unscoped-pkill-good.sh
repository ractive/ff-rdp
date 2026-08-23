#!/usr/bin/env bash
# Fixture: scoped teardown (passes unscoped-pkill). The script stops only the
# port and pid it opened itself, so a Firefox it did not launch is untouched.
set -euo pipefail

SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?set by check-dogfood-script; run this script via: cargo run -p xtask -- check-dogfood-script <plan.md>}"
rm -f "$SENTINEL"

# shellcheck source=kb/iterations/dogfood-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/../../../kb/iterations/dogfood-lib.sh"

dogfood_init
PORT="$(dogfood_free_port)"
dogfood_launch "$PORT"
ffrdp --port "$PORT" daemon stop

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
