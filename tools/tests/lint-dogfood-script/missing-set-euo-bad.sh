#!/usr/bin/env bash
# Fixture: triggers missing-set-euo-pipefail rule (no set -euo pipefail).

SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?set by check-dogfood-script; run this script via: cargo run -p xtask -- check-dogfood-script <plan.md>}"
rm -f "$SENTINEL"

echo "doing something"

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
