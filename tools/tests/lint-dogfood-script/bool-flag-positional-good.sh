#!/usr/bin/env bash
# Fixture: correct boolean flag usage (passes bool-flag-positional rule).
set -euo pipefail

SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?set by check-dogfood-script; run this script via: cargo run -p xtask -- check-dogfood-script <plan.md>}"
rm -f "$SENTINEL"

# Correct: --jq-strict is a boolean flag; --jq takes the expression
set +e
ERR=$(cargo run --quiet -p ff-rdp-cli -- perf audit --jq-strict --jq '.results.does_not_exist_xyz' 2>&1 >/dev/null)
EC=$?
set -e

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
