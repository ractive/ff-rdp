#!/usr/bin/env bash
# Fixture: triggers unscoped-pkill (the opening line every checked-in dogfood
# script carried until iter-193, verbatim). The pattern is not scoped to this
# run, so it terminates every ff-rdp-profile Firefox on the host — including a
# sibling agent's.
set -euo pipefail

SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?set by check-dogfood-script; run this script via: cargo run -p xtask -- check-dogfood-script <plan.md>}"
rm -f "$SENTINEL"

pkill -f 'firefox.*ff-rdp-profile' || true
sleep 1

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
