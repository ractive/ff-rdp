#!/bin/bash
# Transparent agent stream recorder; judge JSON is converted back to its expected text.
set -eu
umask 077
recorder_root="$(cd "$(dirname "$0")" && pwd -P)"
# run.sh installs this through a symlink; resolve helpers from the explicit export.
recorder_root="${FF_RDP_BENCH_HARNESS:-$recorder_root}"
audit="$(mktemp -d "$FF_RDP_BENCH_OUTPUT/invocations/call-XXXXXX")"
printf '%s\n' "$audit" >&2
# Preserve pre-launch failures as well as paid-child failures.
trap 'printf "%s\n" "$?" > "$audit/exit"' EXIT
jq -n --args '$ARGS.positional' -- "$@" > "$audit/argv.json"
model="$(jq -r 'index("--model") as $i | if $i == null then "" else .[$i+1] end' "$audit/argv.json")"
[ "$model" = claude-sonnet-4-6 ] || { echo 'Refusing model drift' >&2; exit 2; }
jq -e 'index("--setting-sources") as $i | $i != null and .[$i+1] == ""' "$audit/argv.json" >/dev/null
jq -e '[.[] | select(. == "--setting-sources")] | length == 1' "$audit/argv.json" >/dev/null
jq -e 'all(.[]; . != "--settings" and (startswith("--settings=") | not))' "$audit/argv.json" >/dev/null
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
treatment="${FF_RDP_BENCH_TREATMENT:-baseline}"
case "$treatment" in baseline|session-start) ;; *) echo 'Unknown treatment' >&2; exit 2;; esac
delivery=disabled
if [ "$judge" = 0 ] && [ "$treatment" = session-start ]; then
  delivery=private-session-start
  cp "$FF_RDP_BENCH_OUTPUT/ambient/installed-settings.json" "$audit/installed-settings.json"
  hook_command="$(jq -nr --arg script "$recorder_root/session-start.sh" \
    --arg binary "${FF_RDP_BENCH_BINARY:?}" --arg audit "$audit" \
    '["/bin/bash",$script,$binary,$audit] | @sh')"
  jq --arg command "$hook_command" '.hooks.SessionStart[0].hooks[0].command = $command' \
    "$audit/installed-settings.json" > "$audit/settings.json"
  shasum -a 256 "$audit/installed-settings.json" "$audit/settings.json" \
    "$recorder_root/session-start.sh" > "$audit/settings.sha256"
  args+=(--settings "$audit/settings.json")
fi
if [ "$judge" = 0 ]; then
  # The appended paragraph is identical in BOTH conditions; hook stdout is separate.
  cp "$audit/condition-prompt.txt" "$audit/delivered-condition-prompt.txt"
  shasum -a 256 "$audit/delivered-condition-prompt.txt" > "$audit/delivered-condition-prompt.sha256"
fi
jq -n --arg treatment "$treatment" --arg delivery "$delivery" --argjson judge "$judge" \
  '{$treatment,$delivery,role:(if $judge == 1 then "judge" else "agent" end)}' > "$audit/delivery.json"
jq -n --args '$ARGS.positional' -- "${args[@]}" > "$audit/delivered-argv.json"
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
  if [ "$judge" = 0 ]; then
    jq -s -f "$recorder_root/first-tool.jq" "$audit/stdout.jsonl" > "$audit/first-tool.json" || status=1
  fi
  if [ "$delivery" = private-session-start ]; then
    # A CLI may silently ignore hooks and still produce a successful answer.
    # That is a failed treatment, even when upstream reports a passing task.
    if [ ! -f "$audit/hook/exit" ] || [ "$(cat "$audit/hook/exit")" != 0 ] || \
      [ ! -s "$audit/hook/stdout.txt" ]; then
      printf 'SessionStart delivery missing or failed\n' >> "$audit/stderr.txt"
      [ "$status" != 0 ] || status=65
    fi
    jq -s '[.[] | select(.type == "system" and (.subtype // "" | startswith("hook")))]' \
      "$audit/stdout.jsonl" > "$audit/hook-events.json" || status=1
    if [ -f "$audit/hook/stdout.txt" ]; then
      jq -e --rawfile output "$audit/hook/stdout.txt" \
        '[.[] | select(.subtype == "hook_response" and .hook_event == "SessionStart")] |
          length == 1 and .[0].exit_code == 0 and .[0].stdout == $output' \
        "$audit/hook-events.json" >/dev/null || {
          printf 'SessionStart runtime response does not confirm recorded output\n' >> "$audit/stderr.txt"
          [ "$status" != 0 ] || status=65
        }
    fi
  fi
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
