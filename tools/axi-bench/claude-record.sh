#!/bin/bash
# Transparent agent stream recorder; judge JSON is converted back to its expected text.
set -eu
audit="$(mktemp -d "$FF_RDP_BENCH_OUTPUT/invocations/call-XXXXXX")"
printf '%s\n' "$audit" >&2
jq -n --args '$ARGS.positional' -- "$@" > "$audit/argv.json"
model="$(jq -r 'index("--model") as $i | if $i == null then "" else .[$i+1] end' "$audit/argv.json")"
[ "$model" = claude-sonnet-4-6 ] || { echo 'Refusing model drift' >&2; exit 2; }
jq -e 'index("--setting-sources") as $i | $i != null and .[$i+1] == ""' "$audit/argv.json" >/dev/null
jq -j 'index("-p") as $i | .[$i+1]' "$audit/argv.json" > "$audit/task-prompt.txt"
shasum -a 256 "$audit/task-prompt.txt" > "$audit/task-prompt.sha256"
cp "$FF_RDP_BENCH_OUTPUT/provenance.json" "$audit/provenance.json"
args=()
judge=0
while [ "$#" -gt 0 ]; do
  if [ "$1" = --output-format ] && [ "${2:-}" = text ]; then
    args+=(--output-format json); judge=1; shift 2
  else args+=("$1"); shift; fi
done
if [ "$judge" = 0 ]; then
  jq -j 'index("--append-system-prompt") as $i | .[$i+1]' "$audit/argv.json" > "$audit/condition-prompt.txt"
  [ "$(shasum -a 256 "$audit/condition-prompt.txt" | cut -d' ' -f1)" = "$FF_RDP_BENCH_PROMPT_HASH" ] || { echo 'Refusing condition prompt drift' >&2; exit 2; }
  cp "$audit/provenance.json" ../provenance.json
fi
# Job control gives this invocation its own process group, including ordinary
# descendants. Never signal the caller's group or discover processes by name.
interrupted=0
child=""
stream_pid=""
# EXIT also covers local recorder failures after launch (for example a full
# audit filesystem). A failed audit write must not strand the paid child.
# shellcheck disable=SC2329 # Invoked by the EXIT trap with its original status.
finish() {
  local status="$1" child_status stream_status i
  trap - EXIT
  trap '' TERM INT
  set +e
  if [ -n "$child" ]; then
    # Even a normally exited child can leave descendants holding stdout open.
    if kill -0 -- "-$child" 2>/dev/null; then
      printf 'TERM group %s\n' "$child" >> "$audit/cancellation.log"
      kill -TERM -- "-$child" 2>/dev/null
      for ((i=0; i<20; i++)); do
        kill -0 -- "-$child" 2>/dev/null || break
        sleep 0.1
      done
      if kill -0 -- "-$child" 2>/dev/null; then
        printf 'KILL group %s\n' "$child" >> "$audit/cancellation.log"
        kill -KILL -- "-$child" 2>/dev/null
      fi
    fi
    wait "$child"
    child_status=$?
    printf '%s\n' "$child_status" > "$audit/child.exit" || status=1
  fi
  if [ -n "$stream_pid" ]; then
    # Normally EOF drains every byte. Bound this too: an escaped descendant
    # holding the pipe must not prevent the wrapper from reporting cancellation.
    for ((i=0; i<20; i++)); do
      kill -0 "$stream_pid" 2>/dev/null || break
      sleep 0.1
    done
    kill -TERM "$stream_pid" 2>/dev/null
    wait "$stream_pid"
    stream_status=$?
    printf '%s\n' "$stream_status" > "$audit/stream.exit" || status=1
    if [ "$status" = 0 ] && [ "$stream_status" != 0 ]; then status="$stream_status"; fi
    rm "$audit/stream.pipe"
  fi
  printf '%s\n' "$interrupted" > "$audit/interrupted.exit" || status=1
  [ "$interrupted" = 0 ] || status="$interrupted"
  jq -s '[.[] | select(.type == "result") | {modelUsage,total_cost_usd,is_error,subtype,result}]' "$audit/stdout.jsonl" > "$audit/usage.json" || true
  cat "$audit/stderr.txt" >&2
  if [ "$judge" = 1 ]; then
    jq -r '.result // empty' "$audit/stdout.jsonl" || { [ "$status" != 0 ] || status=1; }
  fi
  printf '%s\n' "$status" > "$audit/exit" || status=1
  exit "$status"
}
trap 'finish "$?"' EXIT
trap 'interrupted=143' TERM
trap 'interrupted=130' INT
set -m
if [ "$judge" = 0 ]; then
  mkfifo "$audit/stream.pipe"
  tee "$audit/stdout.jsonl" < "$audit/stream.pipe" &
  stream_pid=$!
  env -u ANTHROPIC_API_KEY "$FF_RDP_BENCH_CLAUDE" "${args[@]}" > "$audit/stream.pipe" 2> "$audit/stderr.txt" &
else
  env -u ANTHROPIC_API_KEY "$FF_RDP_BENCH_CLAUDE" "${args[@]}" > "$audit/stdout.jsonl" 2> "$audit/stderr.txt" &
fi
child=$!
set +m
printf '%s\n' "$child" > "$audit/child.pgid"
set +e
if [ "$interrupted" = 0 ]; then wait "$child"; status=$?; else status="$interrupted"; fi
exit "$status"
