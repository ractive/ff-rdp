#!/usr/bin/env bash
# Fixture: triggers fixed-sentinel-path (pre-iter-184 hardcoded sentinel path).
# Everything else about this script is clean; only the SENTINEL assignment is
# the old shared /tmp path, which two concurrent gate runs would collide on.
set -euo pipefail

SENTINEL=/tmp/ff-rdp-iter-99-dogfood-ok
rm -f "$SENTINEL"

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
