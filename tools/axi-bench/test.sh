#!/bin/bash
set -eu
root="$(cd "$(dirname "$0")/../.." && pwd)"
wrapper="${FF_RDP_BENCH_TEST_WRAPPER:-$root/tools/axi-bench/ffrdp-bench.sh}"
scratch="$(mktemp -d -t ff-rdp-bench-test-XXXXXX)"
owned=""
intruder=""
cleanup() {
  [ -z "$owned" ] || { kill "$owned" 2>/dev/null || true; wait "$owned" 2>/dev/null || true; }
  [ -z "$intruder" ] || { kill "$intruder" 2>/dev/null || true; wait "$intruder" 2>/dev/null || true; }
  rm -rf "$scratch"
}
trap cleanup EXIT
export FF_RDP_BENCH_SOURCE_ROOT="$root"
export FF_RDP_BENCH_BINARY=/bin/false
export FF_RDP_BENCH_STATE="$scratch/state"
mkdir -p "$FF_RDP_BENCH_STATE/profile"
# A real process, privately owned by this test, with the expected profile path.
/bin/bash -c 'trap "exit 0" TERM; while :; do sleep 0.1; done' "$FF_RDP_BENCH_STATE/profile/owned" &
owned=$!
sleep 0.1
kill -0 "$owned"
printf '%s\n%s\n' "$owned" "$(LC_ALL=C ps -p "$owned" -o lstart= -o command=)" > "$FF_RDP_BENCH_STATE/firefox.owner"
"$wrapper" stop
if kill -0 "$owned" 2>/dev/null; then echo 'Owned process survived' >&2; exit 1; fi
wait "$owned" 2>/dev/null || true
owned=""
echo 'PASS owned process terminated'

/bin/sleep 60 &
intruder=$!
identity="$(LC_ALL=C ps -p "$intruder" -o lstart= -o command=)"
printf '%s\n%s\n' "$intruder" "$identity" > "$FF_RDP_BENCH_STATE/firefox.owner"
if "$wrapper" stop; then echo 'Incorrectly accepted intruder command' >&2; exit 1; fi
kill -0 "$intruder"
echo 'PASS matching PID/start-time but wrong command refused'

# Correct command substring but stale recorded start identity must also refuse.
mkdir -p "$FF_RDP_BENCH_STATE/profile"
/bin/bash -c 'trap "exit 0" TERM; while :; do sleep 0.1; done' "$FF_RDP_BENCH_STATE/profile/second" &
owned=$!
sleep 0.1
kill -0 "$owned"
identity="$(LC_ALL=C ps -p "$owned" -o lstart= -o command=)"
printf '%s\n%s\n' "$owned" "stale $identity" > "$FF_RDP_BENCH_STATE/firefox.owner"
if "$wrapper" stop; then echo 'Incorrectly accepted stale PID identity' >&2; exit 1; fi
kill -0 "$owned"
rm "$FF_RDP_BENCH_STATE/firefox.owner"
echo 'PASS stale PID identity refused'
kill "$owned"
wait "$owned" 2>/dev/null || true
owned=""

if ! lsof -nP -iTCP:6000 -sTCP:LISTEN >/dev/null 2>&1; then
  nc -l 6000 > "$scratch/listener.log" 2>&1 &
  owned=$!
  sleep 0.2
  lsof -a -p "$owned" -iTCP:6000 -sTCP:LISTEN >/dev/null
  if "$wrapper" start; then echo 'Accepted occupied port' >&2; exit 1; fi
  kill -0 "$owned"
  kill "$owned"
  wait "$owned" 2>/dev/null || true
  owned=""
  echo 'PASS preexisting port listener survives refusal'
  export FF_RDP_BENCH_FIREFOX=/usr/bin/false
  if "$wrapper" start; then echo 'Accepted failed browser launch' >&2; exit 1; fi
  "$wrapper" stop
  echo 'PASS failed browser launch is nonzero and cleanup succeeds'
else
  echo 'SKIP occupied-port fixture: external listener already exists'
fi

# The recorder must retain nonzero agent/judge exits, even with parseable output.
mkdir -p "$scratch/output/invocations" "$scratch/run/workspace"
printf '{}\n' > "$scratch/output/provenance.json"
cat > "$scratch/claude-failure" <<'STUB'
#!/bin/bash
printf '{"type":"result","result":"FAILURE_FIXTURE","is_error":true,"modelUsage":{},"total_cost_usd":0}\n'
exit 37
STUB
chmod +x "$scratch/claude-failure"
export FF_RDP_BENCH_OUTPUT="$scratch/output"
export FF_RDP_BENCH_CLAUDE="$scratch/claude-failure"
FF_RDP_BENCH_PROMPT_HASH="$(printf 'fixture' | shasum -a 256 | cut -d' ' -f1)"
export FF_RDP_BENCH_PROMPT_HASH
cd "$scratch/run/workspace"
for format in stream-json text; do
  set +e
  "$root/tools/axi-bench/claude-record.sh" --setting-sources '' -p fixture --model claude-sonnet-4-6 --append-system-prompt fixture --output-format "$format" > "$scratch/$format.out"
  status=$?
  set -e
  [ "$status" = 37 ] || { echo "Lost $format child status: $status" >&2; exit 1; }
done
echo 'PASS agent and judge exit 37 preserved'
set +e
"$root/tools/axi-bench/claude-record.sh" --setting-sources '' -p fixture --model wrong-model > /dev/null
status=$?
set -e
[ "$status" = 2 ]
echo 'PASS model drift refused'

bash "$root/tools/axi-bench/test-recorder.sh"
bash "$root/tools/axi-bench/test-cleanup.sh"
bash "$root/tools/axi-bench/test-ambient.sh"
