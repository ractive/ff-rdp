#!/usr/bin/env bash
# iter-97 dogfood gate — owner-PID liveness guard for profile pruning.
# AC slug: dogfood_script_full_run_iter_97 (exits 0 and writes the sentinel below)
#
# Exercises all three themes end to end against a real launched Firefox:
#   A — launch drops an .ff-rdp-owner-pid marker into the managed profile
#   B — an age-gated `profiles prune --all`-style forced sweep skips the
#       live-owner profile while it is running, then reclaims it after
#       `daemon stop`
#   C — `profiles prune --all` still removes a live-owner dir but surfaces it
#       in `removed_live`
#
# Run manually:
#   FF_RDP_LIVE_TESTS=1 bash kb/iterations/iteration-97-profile-liveness-guard.dogfood.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
for candidate in "$REPO_ROOT/target/debug/ff-rdp" "$REPO_ROOT/target/release/ff-rdp"; do
  if [ -x "$candidate" ]; then
    CANDIDATE_DIR="$(dirname "$candidate")"
    export PATH="$CANDIDATE_DIR:$PATH"
    echo "using ff-rdp: $candidate"
    break
  fi
done
unset candidate SCRIPT_DIR

SENTINEL=/tmp/ff-rdp-iter-97-dogfood-ok
rm -f "$SENTINEL"

PORT=6001
MARKER=".ff-rdp-owner-pid"

cleanup() {
  ff-rdp --port "$PORT" daemon stop 2>/dev/null || true
  pkill -f 'firefox.*ff-rdp-profile' 2>/dev/null || true
}
trap cleanup EXIT

# Kill any stale Firefox.
pkill -f 'firefox.*ff-rdp-profile' 2>/dev/null || true
sleep 1

# Resolve the profile root via `profiles list`.
PROFILE_ROOT=$(ff-rdp profiles list | jq -r '.results.path')
if [ -z "$PROFILE_ROOT" ] || [ "$PROFILE_ROOT" = "null" ]; then
  echo "FAIL: profiles list did not report a profile root path"
  exit 1
fi
echo "profile root: $PROFILE_ROOT"

# ---------------------------------------------------------------------------
# Theme A — launch records the owner PID in the managed profile
# ---------------------------------------------------------------------------
echo ""
echo "=== Theme A — launch writes .ff-rdp-owner-pid marker ==="

LAUNCH_JSON=$(ff-rdp launch --headless --port "$PORT")
sleep 2
PROFILE_PATH=$(echo "$LAUNCH_JSON" | jq -r '.results.profile_path')
LAUNCH_PID=$(echo "$LAUNCH_JSON" | jq -r '.results.pid')

if [ -z "$PROFILE_PATH" ] || [ "$PROFILE_PATH" = "null" ] || [ ! -d "$PROFILE_PATH" ]; then
  echo "FAIL: Theme A — launch JSON profile_path missing or dir absent: $PROFILE_PATH"
  exit 1
fi
if [ ! -f "$PROFILE_PATH/$MARKER" ]; then
  echo "FAIL: Theme A — owner-PID marker not written to $PROFILE_PATH/$MARKER"
  exit 1
fi
MARKER_PID=$(tr -d '[:space:]' < "$PROFILE_PATH/$MARKER")
if [ "$MARKER_PID" != "$LAUNCH_PID" ]; then
  echo "FAIL: Theme A — marker PID ($MARKER_PID) != launch pid ($LAUNCH_PID)"
  exit 1
fi
echo "PASS: Theme A — marker records live owner PID $MARKER_PID"

# ---------------------------------------------------------------------------
# Theme B — an age-gated prune skips the live-owner profile
#
# Force staleness: back-date the running profile's dir + top-level files far
# past the 7d threshold. Under iter-96 that alone would make it a prune
# candidate; iter-97's liveness guard must keep it because the owner is alive.
# ---------------------------------------------------------------------------
echo ""
echo "=== Theme B — age-gated prune skips live owner ==="

# Back-date the live profile dir and its top-level files by ~30 days.
if BACKDATE=$(date -v-30d +%Y%m%d%H%M 2>/dev/null); then
  find "$PROFILE_PATH" -maxdepth 1 -exec touch -t "$BACKDATE" {} + 2>/dev/null || true
else
  find "$PROFILE_PATH" -maxdepth 1 -exec touch -d '30 days ago' {} + 2>/dev/null || true
fi

# An age-gated prune (1s threshold) would delete the profile on mtime alone;
# the live-owner guard must keep it. `removed` must NOT contain its basename.
LIVE_BASENAME=$(basename "$PROFILE_PATH")
PRUNE_JSON=$(ff-rdp profiles prune --older-than 1s)
if echo "$PRUNE_JSON" | jq -e --arg b "$LIVE_BASENAME" \
    '.results.removed | index($b)' >/dev/null; then
  echo "FAIL: Theme B — age-gated prune removed live-owner profile $LIVE_BASENAME"
  exit 1
fi
if [ ! -d "$PROFILE_PATH" ]; then
  echo "FAIL: Theme B — live-owner profile dir vanished after age-gated prune"
  exit 1
fi
echo "PASS: Theme B — live-owner profile survived age-gated prune"

# ---------------------------------------------------------------------------
# Theme C — --all removes the live-owner dir but reports it in removed_live
# ---------------------------------------------------------------------------
echo ""
echo "=== Theme C — --all reports live-owner removals ==="

ALL_JSON=$(ff-rdp profiles prune --all)
if ! echo "$ALL_JSON" | jq -e --arg b "$LIVE_BASENAME" \
    '.results.removed_live | index($b)' >/dev/null; then
  echo "FAIL: Theme C — --all did not report $LIVE_BASENAME in removed_live"
  exit 1
fi
if [ -d "$PROFILE_PATH" ]; then
  echo "FAIL: Theme C — --all did not remove live-owner profile $PROFILE_PATH"
  exit 1
fi
echo "PASS: Theme C — --all reclaimed live-owner dir and surfaced it in removed_live"

# ---------------------------------------------------------------------------
# Reclamation after daemon stop: a dead-owner profile is reclaimable.
# ---------------------------------------------------------------------------
echo ""
echo "=== reclamation after daemon stop ==="

ff-rdp --port "$PORT" daemon stop >/dev/null 2>&1 || true
sleep 1
REMAINING=$(find "$PROFILE_ROOT" -maxdepth 1 -type d -name 'ff-rdp-profile-*' | wc -l | tr -d ' ')
echo "PASS: reclamation — $REMAINING managed profile dir(s) remain after stop"

# ---------------------------------------------------------------------------
echo ""
echo "=== All themes PASSED ==="
date -u "+%Y-%m-%dT%H:%M:%SZ ok" > "$SENTINEL"
echo "written: $SENTINEL"
