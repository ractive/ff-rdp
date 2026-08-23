#!/usr/bin/env bash
# Fixture: triggers unanchored-grep rule (iter-86 Theme B case verbatim).
set -euo pipefail

SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?set by check-dogfood-script; run this script via: cargo run -p xtask -- check-dogfood-script <plan.md>}"
rm -f "$SENTINEL"

NOTE="regardless of headless mode, Firefox reports..."
echo "$NOTE" | grep -qi 'headless' && { echo "FAIL: headless mentioned" >&2; exit 1; }

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
