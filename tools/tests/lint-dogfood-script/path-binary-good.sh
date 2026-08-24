#!/usr/bin/env bash
# Fixture: drives the binary under test (passes path-binary). `ffrdp` runs the
# build produced from this working tree, never a $PATH lookup.
set -euo pipefail

SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?set by check-dogfood-script; run this script via: cargo run -p xtask -- check-dogfood-script <plan.md>}"
rm -f "$SENTINEL"

# shellcheck source=kb/iterations/dogfood-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/../../../kb/iterations/dogfood-lib.sh"

dogfood_init
PORT="$(dogfood_free_port)"
dogfood_launch "$PORT"
TITLE=$(ffrdp --port "$PORT" eval 'document.title' --jq '.results')
echo "title: $TITLE"

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
