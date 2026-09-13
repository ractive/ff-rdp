#!/bin/bash
# Synthetic shell/process fixtures only; never contacts an authenticated model.
set -eu
root="$(cd "$(dirname "$0")" && pwd -P)"
recorder="${FF_RDP_BENCH_TEST_RECORDER:-$root/claude-record.sh}"
classifier="${FF_RDP_BENCH_TEST_CLASSIFIER:-$root/first-tool.jq}"
scratch="$(mktemp -d -t ff-rdp-ambient-fixtures-XXXXXX)"
echo "Ambient fixture evidence: $scratch"
# Quotes, dollar signs and shell syntax must survive as literal argv bytes.
mkdir "$scratch/bin space'\$;literal"
export FF_RDP_BENCH_BINARY="$scratch/bin space'\$;literal/ff-rdp"
cat > "$FF_RDP_BENCH_BINARY" <<'BINARY'
#!/bin/bash
set -eu
[ "$1" = home ] && [ "$2" = --hook ]
printf 'LIVE_HOOK_FIXTURE\n'
exit "${HOOK_STATUS:-0}"
BINARY
chmod +x "$FF_RDP_BENCH_BINARY"
PATH="$(dirname "$FF_RDP_BENCH_BINARY"):$PATH"
export PATH
cat > "$scratch/claude-fixture" <<'CHILD'
#!/bin/bash
set -eu
jq -n --args '$ARGS.positional' -- "$@" > "$CASE_ROOT/child-argv.json"
settings="$(jq -r 'index("--settings") as $i | if $i == null then "" else .[$i+1] end' "$CASE_ROOT/child-argv.json")"
if [ -n "$settings" ] && [ "${HOOK_MODE:-}" != missing ]; then
  printf '{"hook_event_name":"SessionStart"}\n' | \
    /bin/bash -c "$(jq -r '.hooks.SessionStart[0].hooks[0].command' "$settings")" > "$CASE_ROOT/hook-observed.txt" || true
  if [ "${HOOK_MODE:-}" != no-response ]; then
    jq -cn --rawfile stdout "$CASE_ROOT/hook-observed.txt" \
      '{type:"system",subtype:"hook_response",hook_event:"SessionStart",exit_code:0,$stdout}'
  fi
fi
# Hook/lifecycle-looking events do not count as the agent's first tool call.
printf '{"type":"system","subtype":"hook_response","command":"ff-rdp home --hook"}\n'
printf '{"type":"assistant","parent_tool_use_id":"nested","message":{"content":[{"type":"tool_use","id":"nested-id","name":"Bash","input":{"command":"ff-rdp --help"}}]}}\n'
printf '{"type":"assistant","message":{"content":[{"type":"tool_use","id":"first-actual-id","name":"Read","input":{"file_path":"notes"}},{"type":"tool_use","id":"second-id","name":"Bash","input":{"command":"ff-rdp navigate https://example.com"}}]}}\n'
printf '{"type":"result","result":"fixture result","is_error":false,"modelUsage":{},"total_cost_usd":0}\n'
CHILD
chmod +x "$scratch/claude-fixture"
export FF_RDP_BENCH_CLAUDE="$scratch/claude-fixture"
export FF_RDP_BENCH_HARNESS="$root"
prompt=$'fixture prompt\n'
FF_RDP_BENCH_PROMPT_HASH="$(printf '%s' "$prompt" | shasum -a 256 | cut -d' ' -f1)"
export FF_RDP_BENCH_PROMPT_HASH
run_case() {
  local name="$1" treatment="$2" format="$3" expected="$4" audit status
  export CASE_ROOT="$scratch/$name"
  export FF_RDP_BENCH_OUTPUT="$CASE_ROOT/output"
  export FF_RDP_BENCH_TREATMENT="$treatment"
  mkdir -p "$CASE_ROOT/work/workspace" "$FF_RDP_BENCH_OUTPUT/invocations" "$FF_RDP_BENCH_OUTPUT/ambient"
  printf '{}\n' > "$FF_RDP_BENCH_OUTPUT/provenance.json"
  printf '{"hooks":{"SessionStart":[{"ff_rdp_managed":true,"hooks":[{"type":"command","command":"ff-rdp home --hook"}]}]}}\n' > "$FF_RDP_BENCH_OUTPUT/ambient/installed-settings.json"
  cd "$CASE_ROOT/work/workspace"
  set +e
  # shellcheck disable=SC2016 # Deliberately literal shell syntax is an argv fixture.
  "$recorder" --setting-sources '' -p 'task literal $()' --model claude-sonnet-4-6 \
    --append-system-prompt "$prompt" --output-format "$format" > "$CASE_ROOT/observed" 2> "$CASE_ROOT/stderr"
  status=$?
  set -e
  for audit in "$FF_RDP_BENCH_OUTPUT/invocations/"call-*; do break; done
  [ "$status" = "$expected" ] && [ "$(cat "$audit/exit")" = "$expected" ] || {
    echo "Delivery failure status lost: $name actual=$status expected=$expected" >&2; return 1;
  }
  cmp "$audit/delivered-argv.json" "$CASE_ROOT/child-argv.json"
  jq -e 'index("--setting-sources") as $i | .[$i+1] == ""' "$CASE_ROOT/child-argv.json" >/dev/null
  if [ "$format" = text ] || [ "$treatment" = baseline ]; then
    jq -e 'index("--settings") == null' "$CASE_ROOT/child-argv.json" >/dev/null || {
      echo 'Baseline/judge isolation violated' >&2; return 1;
    }
    [ ! -e "$audit/hook" ]
  else
    jq -e 'index("--settings") != null' "$CASE_ROOT/child-argv.json" >/dev/null || {
      echo 'Treatment settings not delivered' >&2; return 1;
    }
    if [ "$expected" = 0 ]; then
      [ "$(cat "$audit/hook/exit")" = 0 ]
      cmp "$CASE_ROOT/hook-observed.txt" "$audit/hook/stdout.txt"
      jq -e --arg binary "$FF_RDP_BENCH_BINARY" '.binary == $binary' "$audit/hook/identity.json" >/dev/null
    else
      rg -q 'SessionStart delivery missing or failed|SessionStart runtime response does not confirm' "$CASE_ROOT/stderr"
    fi
  fi
  if [ "$format" = stream-json ]; then
    cmp "$audit/condition-prompt.txt" "$audit/delivered-condition-prompt.txt"
    [ "$(shasum -a 256 "$audit/delivered-condition-prompt.txt" | cut -d' ' -f1)" = "$FF_RDP_BENCH_PROMPT_HASH" ]
    jq -e '.id == "first-actual-id" and .category == "other"' "$audit/first-tool.json" >/dev/null || {
      echo 'First actual tool identity misclassified' >&2; return 1;
    }
  else
    [ "$(cat "$CASE_ROOT/observed")" = 'fixture result' ]
  fi
  echo "PASS $name argv, delivery, exact prompt, first actual tool and exit=$status"
}
run_case baseline baseline stream-json 0
run_case treatment session-start stream-json 0
run_case judge session-start text 0
export HOOK_MODE=missing
run_case missing session-start stream-json 65
unset HOOK_MODE
export HOOK_STATUS=37
run_case failed-hook session-start stream-json 65
unset HOOK_STATUS
export HOOK_MODE=no-response
run_case no-response session-start stream-json 65
unset HOOK_MODE

# Classifier partitions: an unrelated first call stays other even when a later
# browser command succeeds; missing/error remain explicit independent dimensions.
for category in '--help' 'browser command' 'bare ff-rdp' other missing; do
  case "$category" in
    --help) command='ff-rdp --help';;
    'browser command') command='ff-rdp navigate https://example.com';;
    'bare ff-rdp') command='ff-rdp';;
    other) command='echo ff-rdp --help';;
    missing) command='';;
  esac
  jq -n --arg command "$command" 'if $command == "" then [] else
    [{type:"assistant",message:{content:[{type:"tool_use",id:"actual",name:"Bash",input:{$command}}]}},
     {type:"result",is_error:true}] end' | jq -f "$classifier" > "$scratch/category.json"
  jq -e --arg category "$category" '.category == $category and
    (if $category == "missing" then .terminal_result == false else .result_error == true end)' "$scratch/category.json" >/dev/null
done
echo 'PASS first-call partitions retain other/missing/error'

# A successful later command/result cannot repair the first decision. Preserve
# the original bytes and ID while separating help, unsupported verbs and syntax
# that needs manual raw-trace adjudication. No fixture invokes the product CLI.
classify_case() {
  local expected="$1" command="$2"
  jq -n --arg command "$command" '[
    {type:"system",subtype:"hook_response",command:"ff-rdp home --hook"},
    {type:"stream_event",event:{type:"content_block_start",content_block:{type:"tool_use",name:"Bash",input:{command:"ff-rdp --help"}}}},
    {type:"assistant",parent_tool_use_id:"nested",message:{content:[{type:"tool_use",id:"nested",name:"Bash",input:{command:"ff-rdp tabs"}}]}},
    {type:"assistant",message:{content:[{type:"tool_use",id:"first",name:"Bash",input:{$command}}]}},
    {type:"assistant",message:{content:[{type:"tool_use",id:"recovery",name:"Bash",input:{command:"ff-rdp tabs"}}]}},
    {type:"result",is_error:false}]' | jq -f "$classifier" > "$scratch/regression.json"
  jq -e --arg category "$expected" --arg command "$command" '
    .id == "first" and .name == "Bash" and .command == $command and
    .category == $category and .terminal_result == true and .result_error == false
  ' "$scratch/regression.json" >/dev/null || {
    printf 'Classifier regression: expected=%s command=%s\n' "$expected" "$command" >&2
    cat "$scratch/regression.json" >&2
    return 1
  }
}
for command in 'ff-rdp --help' 'ff-rdp -h' 'ff-rdp help' \
  'ff-rdp navigate --help' 'ff-rdp navigate -h' 'ff-rdp help navigate' \
  'ff-rdp scroll --help' 'ff-rdp scroll by --help' 'ff-rdp scroll by -h' \
  'ff-rdp help scroll by' 'ff-rdp scroll help by' \
  'ff-rdp a11y summary --help' 'ff-rdp profiles prune --help'; do
  classify_case --help "$command"
done
# shellcheck disable=SC2016 # Literal shell syntax is input to the classifier.
for command in 'ff-rdp fill x' 'ff-rdp press Enter' 'ff-rdp page' \
  'ff-rdp tab' 'ff-rdp hover x' 'ff-rdp select x' 'ff-rdp check x' \
  'ff-rdp scroll nonexistent' 'ff-rdp perf nonexistent' 'ff-rdp a11y nonexistent' \
  'ff-rdp nonexistent --help' 'ff-rdp help nonexistent' 'ff-rdp scroll' \
  'ff-rdp navigate help' 'ff-rdp tabs help' \
  'ff-rdp --port 6000 tabs' 'ff-rdp --version' 'ff-rdp navigate -- --help' \
  'ff-rdp navigate https://example.com --help' \
  'ff-rdp tabs; false' 'ff-rdp tabs && false' 'ff-rdp tabs | head' \
  'ff-rdp tabs > output' 'ff-rdp tabs # comment' \
  'ff-rdp eval "--help"' "ff-rdp eval '--help'" 'ff-rdp eval $(echo --help)' \
  'ff-rdp eval `echo --help`' 'ff-rdp eval document.*' \
  'ff-rdp page-text --query="--help"' $'ff-rdp tabs\nfalse' $'ff-rdp tabs\n' \
  'ff-rdp launch --headless' 'ff-rdp install-hook --claude' 'ff-rdp profiles list'; do
  classify_case other "$command"
done
for command in 'ff-rdp navigate https://example.com' 'ff-rdp tabs' \
  'ff-rdp page-text --full' 'ff-rdp click --ref r1' 'ff-rdp type --ref r1 hello' \
  'ff-rdp scroll by --page-down' 'ff-rdp a11y summary' 'ff-rdp perf vitals' \
  'ff-rdp dom tree' 'ff-rdp reload --with-page' 'ff-rdp home --hook'; do
  classify_case 'browser command' "$command"
done
classify_case 'bare ff-rdp' $' \tff-rdp \t'
echo 'PASS classifier help, real verbs, conservative syntax and later-success regressions'
