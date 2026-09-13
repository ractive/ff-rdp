#!/bin/bash
# Deliberately remove one protection from private copies, never from source.
set -eu
root="$(cd "$(dirname "$0")" && pwd)"
scratch="${FF_RDP_BENCH_MUTATION_OUTPUT:-$(mktemp -d -t ff-rdp-bench-mutations-XXXXXX)}"
mkdir -p "$scratch"
echo "Mutation evidence: $scratch"
awk '
  /tee "\$audit\/stdout.jsonl" < "\$audit\/stream.pipe" &/ {
    sub(/tee "\$audit\/stdout.jsonl"/, "cat > \"$audit/stdout.jsonl\""); changed++
  }
  { print }
  END { if (changed != 1) exit 2 }
' "$root/claude-record.sh" > "$scratch/no-stream.sh"
awk '
  /if kill -0 -- "-\$child" 2>\/dev\/null; then/ {
    sub(/if kill -0 -- "-\$child" 2>\/dev\/null; then/, "if false; then"); changed++
  }
  { print }
  END { if (changed != 2) exit 2 }
' "$root/claude-record.sh" > "$scratch/no-cancel.sh"
for copy in lifecycle results; do
  awk -v copy="$copy" '
    /cp -R/ && index($0, "\"$output/" copy "\"") {
      sub(/\|\| cleanup_status=\$\?/, "|| true"); changed++
    }
    { print }
    END { if (changed != 1) exit 2 }
  ' "$root/cleanup.sh" > "$scratch/no-$copy-failure.sh"
done
chmod +x "$scratch/no-stream.sh" "$scratch/no-cancel.sh"
expect_failure() {
  local name="$1" diagnostic="$2" status
  shift 2
  set +e
  "$@" > "$scratch/$name.log" 2>&1
  status=$?
  set -e
  printf '%s\n' "$status" > "$scratch/$name.exit"
  [ "$status" != 0 ] || { echo "Mutation survived: $name" >&2; return 1; }
  rg -q "$diagnostic" "$scratch/$name.log" || { cat "$scratch/$name.log" >&2; echo "Wrong failure: $name" >&2; return 1; }
  echo "PASS mutation $name rejected with intended assertion (exit=$status)"
}
expect_failure no-stream 'Missing incremental agent output before child exit' \
  env FF_RDP_BENCH_TEST_RECORDER="$scratch/no-stream.sh" FF_RDP_BENCH_TEST_CASE=stream bash "$root/test-recorder.sh"
expect_failure no-cancel 'Wrapper did not finish within 10 seconds' \
  env FF_RDP_BENCH_TEST_RECORDER="$scratch/no-cancel.sh" FF_RDP_BENCH_TEST_CASE=cancel bash "$root/test-recorder.sh"
for copy in lifecycle results; do
  expect_failure "no-$copy-failure" 'Lost evidence copy failure status' \
    env FF_RDP_BENCH_TEST_CLEANUP="$scratch/no-$copy-failure.sh" bash "$root/test-cleanup.sh"
done
