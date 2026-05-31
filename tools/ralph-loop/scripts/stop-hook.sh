#!/usr/bin/env bash
# stop-hook.sh — Lightweight heartbeat for ralph-loop resilience.
#
# Fires on every Claude Code Stop event. No-ops outside ralph-loop sessions:
# bails fast if there's no state.json for the current repo's cache dir.
# When active, writes a timestamp to last-stop so the next /loop tick can
# detect "I was interrupted recently" and resume cleanly.
#
# Install in ~/.claude/settings.json:
#   {
#     "hooks": {
#       "Stop": [
#         { "type": "command",
#           "command": "$HOME/.claude/skills/ralph-loop/scripts/stop-hook.sh",
#           "timeout": 5 }
#       ]
#     }
#   }
#
# Safe to install globally — exits 0 in milliseconds when not in a ralph-loop run.

set -euo pipefail

# Bail fast if not in a git repo.
REPO=$(git rev-parse --show-toplevel 2>/dev/null) || exit 0
SLUG=$(basename "$REPO")
DIR="${RALPH_CACHE_DIR:-$HOME/.cache/ralph-loop/$SLUG}"

# Bail fast if no ralph-loop state for this repo.
[[ -f "$DIR/state.json" ]] || exit 0

# Write heartbeat. Don't fail the hook on errors.
date -u +"%Y-%m-%dT%H:%M:%SZ" > "$DIR/last-stop" 2>/dev/null || true

exit 0
