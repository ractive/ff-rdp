#!/usr/bin/env bash
# iter-96 dogfood gate — temp-profile leak cleanup.
# AC slug: dogfood_script_full_run_iter_96 (exits 0 and writes the sentinel below)
#
# Exercises all three themes:
#   A — daemon stop removes the active temp profile dir (profile_removed JSON)
#   B — launch prunes stale ff-rdp-profile-* orphans older than the threshold
#   C — profiles {list,prune} subcommand + doctor profile_disk_usage check
#
# Run manually:
#   FF_RDP_LIVE_TESTS=1 bash kb/iterations/iteration-96-profile-leak-cleanup.dogfood.sh
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

SENTINEL=/tmp/ff-rdp-iter-96-dogfood-ok
rm -f "$SENTINEL"

PORT=6001

cleanup() {
  ff-rdp --port "$PORT" daemon stop 2>/dev/null || true
  pkill -f 'firefox.*ff-rdp-profile' 2>/dev/null || true
}
trap cleanup EXIT

# Kill any stale Firefox.
pkill -f 'firefox.*ff-rdp-profile' 2>/dev/null || true
sleep 1

# Resolve the profile root via `profiles list` (Theme C surface, also used
# by Themes A/B assertions below).
PROFILE_ROOT=$(ff-rdp profiles list | jq -r '.results.path')
if [ -z "$PROFILE_ROOT" ] || [ "$PROFILE_ROOT" = "null" ]; then
  echo "FAIL: profiles list did not report a profile root path"
  exit 1
fi
echo "profile root: $PROFILE_ROOT"

seed_orphan() {
  # $1 = 16-char suffix; creates a stale (8 days old) fake profile dir.
  local dir="$PROFILE_ROOT/ff-rdp-profile-$1"
  mkdir -p "$dir"
  printf 'fake' > "$dir/prefs.js"
  # 8 days ago, portable-ish: BSD touch on macOS, GNU touch fallback.
  if touch -t "$(date -v-8d +%Y%m%d%H%M 2>/dev/null)" "$dir" 2>/dev/null; then
    :
  else
    touch -d '8 days ago' "$dir"
  fi
}

# ---------------------------------------------------------------------------
# Theme B — launch prunes stale orphan profiles
#
# Seed three stale orphans, launch, assert they are gone and the fresh
# profile exists.
# ---------------------------------------------------------------------------
echo ""
echo "=== Theme B — launch prunes stale orphan profiles ==="

seed_orphan "DOGFOODSTALEaaaa"
seed_orphan "DOGFOODSTALEbbbb"
seed_orphan "DOGFOODSTALEcccc"

LAUNCH_JSON=$(ff-rdp launch --headless --port "$PORT")
sleep 2
PROFILE_PATH=$(echo "$LAUNCH_JSON" | jq -r '.results.profile_path')

for suffix in DOGFOODSTALEaaaa DOGFOODSTALEbbbb DOGFOODSTALEcccc; do
  if [ -d "$PROFILE_ROOT/ff-rdp-profile-$suffix" ]; then
    echo "FAIL: Theme B — stale orphan ff-rdp-profile-$suffix survived launch"
    exit 1
  fi
done

if [ -z "$PROFILE_PATH" ] || [ "$PROFILE_PATH" = "null" ] || [ ! -d "$PROFILE_PATH" ]; then
  echo "FAIL: Theme B — launch JSON profile_path missing or dir absent: $PROFILE_PATH"
  exit 1
fi
echo "PASS: Theme B — 3 stale orphans pruned, fresh profile present ($PROFILE_PATH)"

# ---------------------------------------------------------------------------
# Theme A — daemon stop removes the active profile
# ---------------------------------------------------------------------------
echo ""
echo "=== Theme A — daemon stop removes active profile ==="

STOP_JSON=$(ff-rdp --port "$PORT" daemon stop)
REMOVED=$(echo "$STOP_JSON" | jq -r '.results.profile_removed')
REMOVED_PATH=$(echo "$STOP_JSON" | jq -r '.results.profile_removed_path')

if [ "$REMOVED" != "true" ]; then
  echo "FAIL: Theme A — daemon stop profile_removed != true (got: $REMOVED)"
  exit 1
fi
if [ "$REMOVED_PATH" != "$PROFILE_PATH" ]; then
  echo "FAIL: Theme A — profile_removed_path ($REMOVED_PATH) != launch profile_path ($PROFILE_PATH)"
  exit 1
fi
if [ -d "$PROFILE_PATH" ]; then
  echo "FAIL: Theme A — profile dir still exists after daemon stop: $PROFILE_PATH"
  exit 1
fi
echo "PASS: Theme A — daemon stop removed $REMOVED_PATH"

# ---------------------------------------------------------------------------
# Theme C — profiles subcommand + doctor profile_disk_usage
# ---------------------------------------------------------------------------
echo ""
echo "=== Theme C — profiles list/prune + doctor check ==="

seed_orphan "DOGFOODPRUNEaaaa"
seed_orphan "DOGFOODPRUNEbbbb"

# list must count the seeded orphans.
LIST_COUNT=$(ff-rdp profiles list | jq -r '.results.count')
if [ "$LIST_COUNT" -lt 2 ]; then
  echo "FAIL: Theme C — profiles list count ($LIST_COUNT) < 2 after seeding"
  exit 1
fi

# dry-run lists the orphans without deleting them.
DRY_JSON=$(ff-rdp profiles prune --all --dry-run)
for suffix in DOGFOODPRUNEaaaa DOGFOODPRUNEbbbb; do
  if ! echo "$DRY_JSON" | jq -e --arg b "ff-rdp-profile-$suffix" \
      '.results.would_remove | index($b)' >/dev/null; then
    echo "FAIL: Theme C — dry-run would_remove missing ff-rdp-profile-$suffix"
    exit 1
  fi
  if [ ! -d "$PROFILE_ROOT/ff-rdp-profile-$suffix" ]; then
    echo "FAIL: Theme C — dry-run deleted ff-rdp-profile-$suffix"
    exit 1
  fi
done
echo "PASS: Theme C — prune --dry-run lists orphans without deleting"

# doctor must expose the profile_disk_usage check (status pass/ok/warn, never fail).
DOCTOR_JSON=$(ff-rdp --port "$PORT" doctor 2>/dev/null || true)
DISK_STATUS=$(echo "$DOCTOR_JSON" | jq -r '
  .results[] | select(.name == "profile_disk_usage") | .status
' 2>/dev/null || echo "missing")
case "$DISK_STATUS" in
  pass|ok|warn) echo "PASS: Theme C — doctor profile_disk_usage present (status=$DISK_STATUS)" ;;
  *) echo "FAIL: Theme C — doctor profile_disk_usage status: $DISK_STATUS"; exit 1 ;;
esac

# prune --all with no Firefox running removes everything.
ACTUAL_JSON=$(ff-rdp profiles prune --all)
LEFT=$(find "$PROFILE_ROOT" -maxdepth 1 -type d -name 'ff-rdp-profile-*' | wc -l | tr -d ' ')
if [ "$LEFT" != "0" ]; then
  echo "FAIL: Theme C — $LEFT ff-rdp-profile-* dirs remain after prune --all"
  exit 1
fi
echo "PASS: Theme C — prune --all removed every orphan"

# ---------------------------------------------------------------------------
echo ""
echo "=== All themes PASSED ==="
date -u "+%Y-%m-%dT%H:%M:%SZ ok" > "$SENTINEL"
echo "written: $SENTINEL"
