#!/usr/bin/env bash
# Fixture: correct anchored grep form (passes unanchored-grep rule).
set -euo pipefail

SENTINEL=/tmp/ff-rdp-iter-99-dogfood-ok
rm -f "$SENTINEL"

NOTE="regardless of headless mode, Firefox reports..."
# Correct: anchored so it won't match "regardless of headless mode"
echo "$NOTE" | grep -qiE '(^|[^a-z])headless Firefox' && { echo "FAIL: lcp_note mentions headless Firefox" >&2; exit 1; }

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
