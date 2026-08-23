#!/usr/bin/env bash
# Fixture: triggers bool-flag-positional rule (iter-86 Theme D case verbatim).
set -euo pipefail

SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?set by check-dogfood-script; run this script via: cargo run -p xtask -- check-dogfood-script <plan.md>}"
rm -f "$SENTINEL"

# Bug: --jq-strict is boolean but is used with a positional value here
set +e
ERR=$(cargo run --quiet -p ff-rdp-cli -- perf audit --jq-strict '.results.does_not_exist_xyz' 2>&1 >/dev/null)
EC=$?
set -e

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
