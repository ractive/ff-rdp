#!/bin/bash
# Zero-cost live process fixtures; never invokes authenticated Claude.
set -eu
root="$(cd "$(dirname "$0")" && pwd)"
recorder="${FF_RDP_BENCH_TEST_RECORDER:-$root/claude-record.sh}"
scratch="$(mktemp -d -t ff-rdp-recorder-test-XXXXXX)"
echo "Recorder fixture artifacts: $scratch"
wrapper_pid=""
control=""
cleanup_fixture() {
  local file pid command
  [ -z "$wrapper_pid" ] || { kill -KILL "$wrapper_pid" 2>/dev/null || true; wait "$wrapper_pid" 2>/dev/null || true; }
  for file in "$scratch"/*/*.pid; do
    [ -f "$file" ] || continue
    pid="$(cat "$file")"
    command="$(ps -p "$pid" -o command= 2>/dev/null)" || continue
    case "$command" in *"$scratch/child-fixture"*) kill -KILL "$pid" 2>/dev/null || true;; esac
  done
  [ -z "$control" ] || { kill "$control" 2>/dev/null || true; wait "$control" 2>/dev/null || true; }
  # Retain exact partial streams, exits and PID records even when a mutation fails.
}
trap cleanup_fixture EXIT
cat > "$scratch/child-fixture" <<'CHILD'
#!/bin/bash
set -eu
if [ "${1:-}" = descendant ]; then
  printf '%s\n' "$$" > "$CASE_ROOT/descendant.pid"
  if [ "$CASE_MODE" = hung ]; then trap '' TERM INT; else trap 'exit 0' TERM INT; fi
  while [ "$CASE_MODE" = orphan ] || [ ! -f "$CASE_ROOT/release" ]; do sleep 0.05; done
  exit 0
fi
printf '%s\n' "$$" > "$CASE_ROOT/child.pid"
if [ "$CASE_MODE" = hung ]; then trap '' TERM INT; else trap 'exit 0' TERM INT; fi
"$0" descendant &
descendant=$!
printf '{"type":"assistant","message":{"content":[]},"fixture":"partial"}\n'
printf 'fixture stderr\n' >&2
while [ ! -f "$CASE_ROOT/release" ]; do sleep 0.05; done
if [ "$CASE_MODE" != orphan ]; then wait "$descendant"; fi
printf '{"type":"result","result":"fixture result","modelUsage":{},"total_cost_usd":0}\n'
if [ "$CASE_MODE" = failure ]; then exit 37; fi
CHILD
chmod +x "$scratch/child-fixture"
/bin/sleep 120 &
control=$!
export FF_RDP_BENCH_CLAUDE="$scratch/child-fixture"
FF_RDP_BENCH_PROMPT_HASH="$(printf fixture | shasum -a 256 | cut -d' ' -f1)"
export FF_RDP_BENCH_PROMPT_HASH

run_case() {
  local mode="$1" signal="$2" format="$3" audit i status expected pid
  export CASE_MODE="$mode" CASE_ROOT="$scratch/$mode-$signal-$format"
  mkdir -p "$CASE_ROOT/output/invocations" "$CASE_ROOT/run/workspace"
  printf '{}\n' > "$CASE_ROOT/output/provenance.json"
  export FF_RDP_BENCH_OUTPUT="$CASE_ROOT/output"
  cd "$CASE_ROOT/run/workspace"
  # Job control prevents an asynchronously launched shell from inheriting
  # ignored SIGINT, matching a synchronous upstream execSync timeout target.
  set -m
  "$recorder" --setting-sources '' -p fixture --model claude-sonnet-4-6 --append-system-prompt fixture --output-format "$format" > "$CASE_ROOT/observed" 2> "$CASE_ROOT/stderr" &
  wrapper_pid=$!
  set +m
  for ((i=0; i<100; i++)); do
    [ -f "$CASE_ROOT/descendant.pid" ] && break
    sleep 0.05
  done
  [ -f "$CASE_ROOT/descendant.pid" ] || { echo 'Fixture child never started' >&2; return 1; }
  for audit in "$CASE_ROOT/output/invocations/"call-*; do break; done
  if [ "$format" = stream-json ]; then
    for ((i=0; i<40; i++)); do
      [ -s "$CASE_ROOT/observed" ] && cmp -s "$CASE_ROOT/observed" "$audit/stdout.jsonl" && break
      sleep 0.05
    done
    [ -s "$CASE_ROOT/observed" ] || { echo 'Missing incremental agent output before child exit' >&2; return 1; }
    kill -0 "$(cat "$CASE_ROOT/child.pid")"
    cmp "$CASE_ROOT/observed" "$audit/stdout.jsonl"
    echo "PASS $mode partial output visible and audited while child alive"
  else
    [ ! -s "$CASE_ROOT/observed" ] || { echo 'Judge leaked raw JSON into text interface' >&2; return 1; }
  fi
  if [ "$signal" = none ]; then
    touch "$CASE_ROOT/release"
    expected=0
    [ "$mode" != failure ] || expected=37
  else
    kill -"$signal" "$wrapper_pid"
    expected=143
    [ "$signal" != INT ] || expected=130
  fi
  for ((i=0; i<100; i++)); do
    kill -0 "$wrapper_pid" 2>/dev/null || break
    sleep 0.1
  done
  if kill -0 "$wrapper_pid" 2>/dev/null; then echo 'Wrapper did not finish within 10 seconds' >&2; return 1; fi
  set +e
  wait "$wrapper_pid"
  status=$?
  set -e
  wrapper_pid=""
  [ "$status" = "$expected" ] || { echo "Wrong wrapper exit: $status expected $expected" >&2; return 1; }
  [ "$(cat "$audit/exit")" = "$expected" ]
  if [ "$signal" != none ]; then [ "$(cat "$audit/interrupted.exit")" = "$expected" ]; fi
  for pid in "$(cat "$CASE_ROOT/child.pid")" "$(cat "$CASE_ROOT/descendant.pid")"; do
    for ((i=0; i<30; i++)); do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
    if kill -0 "$pid" 2>/dev/null; then echo "Owned fixture process survived: $pid" >&2; return 1; fi
  done
  kill -0 "$control"
  if [ "$format" = stream-json ]; then
    cmp "$CASE_ROOT/observed" "$audit/stdout.jsonl"
    [ "$(cat "$audit/stream.exit")" = 0 ]
  elif [ "$signal" = none ]; then
    [ "$(cat "$CASE_ROOT/observed")" = 'fixture result' ]
  fi
  if [ "$mode" = hung ]; then
    rg -q 'KILL group' "$audit/cancellation.log"
    [ "$(cat "$audit/child.exit")" = 137 ]
  fi
  echo "PASS $mode $signal $format exit=$status, owned child/descendant absent, control=$control alive"
}

case "${FF_RDP_BENCH_TEST_CASE:-all}" in
  stream) run_case normal none stream-json;;
  cancel) run_case hung TERM stream-json;;
  all)
    run_case normal none stream-json
    run_case failure none stream-json
    run_case orphan none stream-json
    run_case normal TERM stream-json
    run_case normal INT stream-json
    run_case hung TERM stream-json
    run_case normal none text
    run_case failure none text
    run_case hung TERM text
    ;;
  *) echo 'Unknown recorder test case' >&2; exit 2;;
esac
