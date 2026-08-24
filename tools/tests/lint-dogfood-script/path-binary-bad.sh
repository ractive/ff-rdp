#!/usr/bin/env bash
# Fixture: triggers path-binary. `ff-rdp` here is whatever is first on $PATH,
# which may be a months-old install rather than the branch under test.
set -euo pipefail

SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?set by check-dogfood-script; run this script via: cargo run -p xtask -- check-dogfood-script <plan.md>}"
rm -f "$SENTINEL"

ff-rdp launch --headless --port 6000
TITLE=$(ff-rdp eval 'document.title' --jq '.results')
echo "title: $TITLE"

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
