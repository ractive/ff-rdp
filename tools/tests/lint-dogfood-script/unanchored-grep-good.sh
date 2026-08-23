#!/usr/bin/env bash
# Fixture: correct anchored grep form (passes unanchored-grep rule).
set -euo pipefail

SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?set by check-dogfood-script; run this script via: cargo run -p xtask -- check-dogfood-script <plan.md>}"
rm -f "$SENTINEL"

NOTE="regardless of headless mode, Firefox reports..."
# Correct: anchored so it won't match "regardless of headless mode"
echo "$NOTE" | grep -qiE '(^|[^a-z])headless Firefox' && { echo "FAIL: lcp_note mentions headless Firefox" >&2; exit 1; }

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
