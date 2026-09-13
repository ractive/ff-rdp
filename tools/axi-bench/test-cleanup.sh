#!/bin/bash
set -eu
root="$(cd "$(dirname "$0")/../.." && pwd)"
cleanup_source="${FF_RDP_BENCH_TEST_CLEANUP:-$root/tools/axi-bench/cleanup.sh}"
scratch="$(mktemp -d -t ff-rdp-evidence-test-XXXXXX)"
echo "Cleanup fixture artifacts: $scratch"
mkdir "$scratch/bin" "$scratch/harness"
cat > "$scratch/harness/ffrdp-bench.sh" <<'STOP'
#!/bin/bash
exit 0
STOP
cat > "$scratch/bin/cp" <<'COPY'
#!/bin/bash
if [ "${FAIL_COPY:-}" = "$2" ]; then
  echo "fixture injected copy failure: $2" >&2
  exit 74
fi
exec /bin/cp "$@"
COPY
chmod +x "$scratch/bin/cp" "$scratch/harness/ffrdp-bench.sh"

for failure in lifecycle results none; do
  for matrix_status in 0 37; do
    case_root="$scratch/$failure-$matrix_status"
    mkdir -p "$case_root/work/lifecycle" "$case_root/work/axi/bench-browser/results" "$case_root/target" "$case_root/output"
    printf 'lifecycle original\n' > "$case_root/work/lifecycle/identity"
    printf 'paid trajectory original\n' > "$case_root/work/axi/bench-browser/results/trajectory"
    printf 'binary original\n' > "$case_root/target/binary"
    printf '%s\n' "$matrix_status" > "$case_root/output/matrix.exit"
    set +e
    (
      set -eu
      # Exercise the real library's EXIT hook and its `hook || true` context.
      # shellcheck source=kb/iterations/dogfood-lib.sh
      . "$root/kb/iterations/dogfood-lib.sh"
      # shellcheck disable=SC1090
      . "$cleanup_source"
      # shellcheck disable=SC2034 # Variables consumed by the sourced cleanup.
      harness="$scratch/harness"
      # shellcheck disable=SC2034 # Consumed by the sourced cleanup.
      output="$case_root/output"
      work="$case_root/work"
      export FF_RDP_BENCH_STATE="$work/lifecycle" CARGO_TARGET_DIR="$case_root/target"
      export PATH="$scratch/bin:$PATH" FAIL_COPY=""
      if [ "$failure" = lifecycle ]; then export FAIL_COPY="$FF_RDP_BENCH_STATE"; fi
      if [ "$failure" = results ]; then export FAIL_COPY="$work/axi/bench-browser/results"; fi
      dogfood_on_exit cleanup
      trap dogfood_teardown EXIT
      exit "$matrix_status"
    ) > "$case_root/run.log" 2>&1
    status=$?
    set -e
    [ "$status" = "$matrix_status" ]
    [ "$(cat "$case_root/output/matrix.exit")" = "$matrix_status" ]
    if [ "$failure" = none ]; then
      [ "$(cat "$case_root/output/cleanup.exit")" = 0 ]
      [ ! -d "$case_root/work" ] && [ ! -d "$case_root/target" ]
      [ "$(cat "$case_root/output/lifecycle/identity")" = 'lifecycle original' ]
      [ "$(cat "$case_root/output/results/trajectory")" = 'paid trajectory original' ]
    else
      [ "$(cat "$case_root/output/cleanup.exit")" = 74 ] || { echo 'Lost evidence copy failure status' >&2; exit 1; }
      [ "$(cat "$case_root/work/lifecycle/identity")" = 'lifecycle original' ]
      [ "$(cat "$case_root/work/axi/bench-browser/results/trajectory")" = 'paid trajectory original' ]
      [ "$(cat "$case_root/target/binary")" = 'binary original' ]
      rg -q 'Cleanup failed \(74\); recover work=' "$case_root/run.log"
    fi
    echo "PASS copy=$failure matrix=$matrix_status: verdict, evidence and retention/deletion verified"
  done
done
