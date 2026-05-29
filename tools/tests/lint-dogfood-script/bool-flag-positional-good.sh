#!/usr/bin/env bash
# Fixture: correct boolean flag usage (passes bool-flag-positional rule).
set -euo pipefail

SENTINEL=/tmp/ff-rdp-iter-99-dogfood-ok
rm -f "$SENTINEL"

# Correct: --jq-strict is a boolean flag; --jq takes the expression
set +e
ERR=$(ff-rdp perf audit --jq-strict --jq '.results.does_not_exist_xyz' 2>&1 >/dev/null)
EC=$?
set -e

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
