#!/usr/bin/env bash
# Fixture: triggers missing-set-euo-pipefail rule (no set -euo pipefail).

SENTINEL=/tmp/ff-rdp-iter-99-dogfood-ok
rm -f "$SENTINEL"

echo "doing something"

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
