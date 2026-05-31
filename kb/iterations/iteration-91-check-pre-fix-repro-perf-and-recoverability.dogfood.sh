#!/usr/bin/env bash
# iter-91 dogfood gate — exercises the SHA-keyed result cache and
# SKIP_WORKTREE path of check-pre-fix-repro.
# Executed by `cargo run -p xtask -- check-dogfood-script <plan>`.
#
# Run manually:
#   bash kb/iterations/iteration-91-check-pre-fix-repro-perf-and-recoverability.dogfood.sh
set -euo pipefail

SENTINEL=/tmp/ff-rdp-iter-91-dogfood-ok
rm -f "$SENTINEL"

REPO_ROOT="$(git rev-parse --show-toplevel)"
CACHE_DIR="$(mktemp -d -t ff-rdp-iter91-cache-XXXX)"
PLAN_DIR="$(mktemp -d -t ff-rdp-iter91-plan-XXXX)"
trap 'rm -rf "$CACHE_DIR" "$PLAN_DIR"' EXIT

FIXTURE_PLAN="$PLAN_DIR/fixture-plan.md"
SHA_OVERRIDE="iter91-dogfood-sha-abcdef"
SLUG="my_repro_test"
CRATE="xtask"

# --- Write a minimal valid plan with one pre_fix_repro_test annotation ---
cat > "$FIXTURE_PLAN" << 'PLAN_EOF'
---
title: "Dogfood Fixture Plan for iter-91"
status: planned
type: iteration
---

### Theme A — fix something [pre_fix_repro_test: my_repro_test]

Some content.
PLAN_EOF

# --- Pre-seed the cache with a FAIL result ---
RESULTS_DIR="$CACHE_DIR/results"
mkdir -p "$RESULTS_DIR"
CACHE_KEY="${SHA_OVERRIDE}-${CRATE}-${SLUG}"
printf 'FAIL\n2026-01-01T00:00:00Z\n' > "$RESULTS_DIR/$CACHE_KEY"

# --- First run: should be a cache hit ---
FIRST_START=$(date +%s)
FF_RDP_PRE_FIX_REPRO_CACHE_DIR="$CACHE_DIR" \
  FF_RDP_PRE_FIX_REPRO_SKIP_WORKTREE=1 \
  FF_RDP_PRE_FIX_REPRO_SHA_OVERRIDE="$SHA_OVERRIDE" \
  cargo run -q -p xtask -- check-pre-fix-repro \
    --plan "$FIXTURE_PLAN" \
    --crate-name "$CRATE" > /tmp/iter91-run1.out 2>&1
FIRST_END=$(date +%s)
FIRST_ELAPSED=$(( FIRST_END - FIRST_START ))

grep -q 'cache hit' /tmp/iter91-run1.out || {
  echo "FAIL: first run did not report cache hit" >&2
  cat /tmp/iter91-run1.out >&2
  exit 1
}

# --- Second run: also a cache hit, must complete in < 5 seconds ---
SECOND_START=$(date +%s)
FF_RDP_PRE_FIX_REPRO_CACHE_DIR="$CACHE_DIR" \
  FF_RDP_PRE_FIX_REPRO_SKIP_WORKTREE=1 \
  FF_RDP_PRE_FIX_REPRO_SHA_OVERRIDE="$SHA_OVERRIDE" \
  cargo run -q -p xtask -- check-pre-fix-repro \
    --plan "$FIXTURE_PLAN" \
    --crate-name "$CRATE" > /tmp/iter91-run2.out 2>&1
SECOND_END=$(date +%s)
SECOND_ELAPSED=$(( SECOND_END - SECOND_START ))

grep -q 'cache hit' /tmp/iter91-run2.out || {
  echo "FAIL: second run did not report cache hit" >&2
  cat /tmp/iter91-run2.out >&2
  exit 1
}

if [ "$SECOND_ELAPSED" -ge 5 ]; then
  echo "FAIL: second cache-hit run took ${SECOND_ELAPSED}s (expected < 5s)" >&2
  exit 1
fi

# --- Verify output does not contain forbidden phrase ---
if grep -q 'green on branch HEAD' /tmp/iter91-run1.out; then
  echo "FAIL: output contains 'green on branch HEAD' in run1" >&2
  exit 1
fi
if grep -q 'green on branch HEAD' /tmp/iter91-run2.out; then
  echo "FAIL: output contains 'green on branch HEAD' in run2" >&2
  exit 1
fi

echo "[iter-91 dogfood] first run: ${FIRST_ELAPSED}s, second run: ${SECOND_ELAPSED}s"

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
echo "iter-91 dogfood: cache-hit path verified (run2: ${SECOND_ELAPSED}s) — $SENTINEL"
