#!/usr/bin/env bash
# Fixture: passes all lint rules.
set -euo pipefail

SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?set by check-dogfood-script; run this script via: cargo run -p xtask -- check-dogfood-script <plan.md>}"
rm -f "$SENTINEL"

# Correct anchored grep
NOTE="regardless of headless mode, Firefox reports..."
echo "$NOTE" | grep -qiE '(^|[^a-z])headless Firefox' && { echo "FAIL: lcp_note mentions headless Firefox" >&2; exit 1; }
true  # placeholder — no headless mention expected, grep returns non-zero

# Correct boolean flag usage
set +e
ERR=$(cargo run --quiet -p ff-rdp-cli -- perf audit --jq-strict --jq '.results.does_not_exist_xyz' 2>&1 >/dev/null) || true
EC=$?
set -e

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
echo "dogfood: all checks passed"
