#!/usr/bin/env bash
# Fixture: triggers bool-flag-positional rule (iter-86 Theme D case verbatim).
set -euo pipefail

SENTINEL=/tmp/ff-rdp-iter-99-dogfood-ok
rm -f "$SENTINEL"

# Bug: --jq-strict is boolean but is used with a positional value here
set +e
ERR=$(ff-rdp perf audit --jq-strict '.results.does_not_exist_xyz' 2>&1 >/dev/null)
EC=$?
set -e

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
