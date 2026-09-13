#!/bin/bash
# Sourced by run.sh. dogfood_teardown invokes this in an errexit-suppressed
# context, so every evidence operation must explicitly contribute to the verdict.
# shellcheck disable=SC2154 # harness/output/work are owned by the sourcing runner.
# shellcheck disable=SC2329 # Registered with dogfood_on_exit.
cleanup() {
  local status=$? cleanup_status=0
  "$harness/ffrdp-bench.sh" stop >> "$output/cleanup.log" 2>&1 || cleanup_status=$?
  cp -R "$FF_RDP_BENCH_STATE" "$output/lifecycle" >> "$output/cleanup.log" 2>&1 || cleanup_status=$?
  if [ -d "$work/axi/bench-browser/results" ]; then
    cp -R "$work/axi/bench-browser/results" "$output/results" >> "$output/cleanup.log" 2>&1 || cleanup_status=$?
  fi
  # An unwritable verdict is itself a failure: do not destroy the originals.
  printf '%s\n' "$cleanup_status" > "$output/cleanup.exit" || cleanup_status=$?
  if [ "$cleanup_status" = 0 ]; then
    rm -rf "$work" "$CARGO_TARGET_DIR" || cleanup_status=$?
    printf '%s\n' "$cleanup_status" > "$output/cleanup.exit" || cleanup_status=$?
  fi
  if [ "$cleanup_status" != 0 ]; then
    printf 'Cleanup failed (%s); recover work=%s target=%s\n' "$cleanup_status" "$work" "$CARGO_TARGET_DIR" >&2
  fi
  # The matrix status is independent; consumers must inspect cleanup.exit too.
  return "$status"
}
