#!/usr/bin/env bash
# iter-91 dogfood gate — check-pre-fix-repro is fast on the cache-hit path.
# Executed by `cargo run -p xtask -- check-dogfood-script <plan>`.
#
# Run manually:
#   bash kb/iterations/iteration-91-*.dogfood.sh
#
# This iteration touches no Firefox; the gate is a pure tooling test.
set -euo pipefail

SENTINEL=/tmp/ff-rdp-iter-91-dogfood-ok
rm -f "$SENTINEL"

REPO_ROOT=$(cd "$(dirname "$0")/../.." && pwd)
FIXTURE_DIR=$(mktemp -d -t iter-91-fixture.XXXXXX)
trap 'rm -rf "$FIXTURE_DIR"' EXIT

# --- Theme A: cache-hit invocation completes in < 5s ---
# Use a real annotated plan from the repo (iter-89's plan has one annotation).
FIXTURE_PLAN="$REPO_ROOT/kb/iterations/iteration-89-screenshot-fifth-attempt-single-theme.md"
test -f "$FIXTURE_PLAN" || { echo "FAIL Theme A: fixture plan missing: $FIXTURE_PLAN" >&2; exit 1; }

cd "$REPO_ROOT"

# First invocation: populate cache + warm main worktree. Don't time-bound this —
# fresh-machine first run is allowed up to ~15 min cold compile.
echo "[iter-91 dogfood] first run (populates cache)..."
cargo run -q -p xtask -- check-pre-fix-repro --plan "$FIXTURE_PLAN" >/dev/null 2>&1 \
  || { echo "FAIL Theme A: first invocation exited non-zero" >&2; exit 1; }

# Second invocation: must hit the cache and finish fast.
echo "[iter-91 dogfood] second run (must hit cache, < 5s)..."
START=$(python3 -c 'import time; print(int(time.time()*1000))')
cargo run -q -p xtask -- check-pre-fix-repro --plan "$FIXTURE_PLAN" >/dev/null 2>&1 \
  || { echo "FAIL Theme A: second invocation exited non-zero" >&2; exit 1; }
END=$(python3 -c 'import time; print(int(time.time()*1000))')
ELAPSED=$((END - START))
test "$ELAPSED" -lt 5000 \
  || { echo "FAIL Theme A: cache-hit run took ${ELAPSED}ms (expected <5000)" >&2; exit 1; }

# Working tree integrity: no stash entries, no detached HEAD, no modified files
# left behind by the check.
STASH_COUNT=$(git stash list | wc -l | tr -d ' ')
test "$STASH_COUNT" = "0" \
  || { echo "FAIL Theme A: check left $STASH_COUNT stash entries behind" >&2; git stash list >&2; exit 1; }

git symbolic-ref --quiet HEAD >/dev/null \
  || { echo "FAIL Theme A: HEAD is detached after check" >&2; exit 1; }

DIRTY=$(git status --porcelain | wc -l | tr -d ' ')
test "$DIRTY" = "0" \
  || { echo "FAIL Theme A: working tree dirty after check" >&2; git status --porcelain >&2; exit 1; }

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
echo "iter-91 dogfood: check-pre-fix-repro cache + worktree verified — $SENTINEL"
