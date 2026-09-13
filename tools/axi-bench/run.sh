#!/bin/bash
set -eu
umask 077
harness="$(cd "$(dirname "$0")" && pwd)"
source_root="$(cd "$harness/../.." && pwd)"
revision=""
harness_revision=""
output=""
tasks=""
repeat=3
label=baseline
prepare=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --source-root) source_root="$2"; shift 2;;
    --revision) revision="$2"; shift 2;;
    --harness-revision) harness_revision="$2"; shift 2;;
    --output) output="$2"; shift 2;;
    --task) tasks="$2"; shift 2;;
    --repeat) repeat="$2"; shift 2;;
    --label) label="$2"; shift 2;;
    --prepare-only) prepare=1; shift;;
    --help) echo 'run.sh --output NEW_DIRECTORY [--source-root CHECKOUT --revision FULL_SHA] [--harness-revision EXPORT_SHA] [--task ID,ID --repeat N --label LABEL] [--prepare-only]'; exit 0;;
    *) echo "Unknown argument: $1" >&2; exit 2;;
  esac
done
[ -n "$output" ] || { echo '--output is required (must not exist)' >&2; exit 2; }
case "$repeat" in ''|*[!0-9]*|0) echo 'repeat must be positive' >&2; exit 2;; esac
for dep in git cargo jq pnpm node claude lsof shasum; do command -v "$dep" >/dev/null; done
source_root="$(cd -P "$source_root" && pwd)"
head="$(git -C "$source_root" rev-parse HEAD)"
revision="${revision:-$head}"
[ "$head" = "$revision" ] || { echo "Product HEAD $head differs from --revision $revision" >&2; exit 2; }
[ -z "$(git -C "$source_root" status --porcelain --untracked-files=normal)" ] || { echo 'Product source must be clean' >&2; exit 2; }
if lsof -nP -iTCP:6000 -sTCP:LISTEN >/dev/null 2>&1; then echo 'Refusing occupied port 6000' >&2; exit 2; fi
mkdir "$output"
output="$(cd "$output" && pwd)"
export FF_RDP_BENCH_OUTPUT="$output"
export FF_RDP_BENCH_SOURCE_ROOT="$source_root"
FF_RDP_BENCH_CLAUDE="$(command -v claude)"
export FF_RDP_BENCH_CLAUDE
export FF_RDP_BENCH_FIREFOX="${FF_RDP_BENCH_FIREFOX:-/Applications/Firefox.app/Contents/MacOS/firefox}"
if [ ! -x "$FF_RDP_BENCH_FIREFOX" ]; then FF_RDP_BENCH_FIREFOX="$(command -v firefox)"; fi
CARGO_TARGET_DIR="$(mktemp -d -t ff-rdp-bench-target-XXXXXX)"
CARGO_TARGET_DIR="$(cd -P "$CARGO_TARGET_DIR" && pwd)"
export CARGO_TARGET_DIR
work="$(mktemp -d -t ff-rdp-axi-XXXXXX)"
work="$(cd -P "$work" && pwd)"
export FF_RDP_BENCH_STATE="$work/lifecycle"
mkdir -p "$FF_RDP_BENCH_STATE" "$output/invocations" "$work/bin"
# shellcheck source=tools/axi-bench/cleanup.sh
. "$harness/cleanup.sh"
# shellcheck source=kb/iterations/dogfood-lib.sh
. "$source_root/kb/iterations/dogfood-lib.sh"
# shellcheck disable=SC2034 # Consumed by sourced dogfood-lib.
FF_RDP_DOGFOOD_REPO_ROOT="$source_root"
export FF_RDP_BENCH_BINARY="$CARGO_TARGET_DIR/debug/ff-rdp"
dogfood_on_exit cleanup
dogfood_init
export FF_RDP_BENCH_BINARY="$FF_RDP_DOGFOOD_BIN"
export FF_RDP_HOME="$FF_RDP_BENCH_STATE/home"
git clone --quiet --no-checkout https://github.com/kunchenguid/axi.git "$work/axi"
git -C "$work/axi" checkout --quiet --detach d28c5e79aa7ee7a59a386fc34125f8cd1470fbeb
[ "$(shasum -a 256 "$work/axi/bench-browser/src/runner.ts" | cut -d' ' -f1)" = 11659d64e71fa116744f6b837d0b8b246c8623eb0f3cf796a2342a7567a24a8b ]
git -C "$work/axi" apply "$harness/ff-rdp-condition.patch"
ln -s "$harness/ffrdp-bench.sh" "$work/bin/ffrdp-bench.sh"
ln -s "$harness/claude-record.sh" "$work/bin/claude"
export PATH="$CARGO_TARGET_DIR/debug:$work/bin:$PATH"
[ "$(command -v ff-rdp)" = "$FF_RDP_BENCH_BINARY" ]
# This exact YAML block scalar includes its trailing newline.
export FF_RDP_BENCH_PROMPT_HASH=758dfb37452f8099cfb46460ee417ccc73839cf0ee3553f7da0558229be49c8b
harness_revision="${harness_revision:-$(git -C "$harness" rev-parse HEAD)}"
for file in "$harness"/*; do
  shasum -a 256 "$file"
done > "$output/harness.sha256"
jq -n --arg harness_revision "$harness_revision" --arg product_revision "$revision" \
  --arg source_root "$source_root" --arg binary "$FF_RDP_BENCH_BINARY" \
  --arg binary_sha256 "$(shasum -a 256 "$FF_RDP_BENCH_BINARY" | cut -d' ' -f1)" \
  --arg version "$(ffrdp --version)" --arg cli_version "$("$FF_RDP_BENCH_CLAUDE" --version)" \
  --arg firefox_version "$("$FF_RDP_BENCH_FIREFOX" --version)" \
  --arg node_version "$(node --version)" --arg pnpm_version "$(pnpm --version)" \
  --arg prompt_sha256 "$FF_RDP_BENCH_PROMPT_HASH" --arg label "$label" \
  --arg task "$tasks" --argjson repeat "$repeat" \
  '{$harness_revision,$product_revision,$source_root,$binary,$binary_sha256,$version,$cli_version,$firefox_version,$node_version,$pnpm_version,$prompt_sha256,$label,$task,$repeat,model:"claude-sonnet-4-6",judge_model:"claude-sonnet-4-6",historical_cli_version:"2.1.241",upstream_revision:"d28c5e79aa7ee7a59a386fc34125f8cd1470fbeb",credential_source:"existing cached account; ANTHROPIC_API_KEY omitted only for child",hook_delivery:"disabled baseline",judge_usage_instrumentation:"JSON output converted back to original text interface"}' > "$output/provenance.json"
cd "$work/axi"
jq --arg effective_pnpm_version "$(pnpm --version)" '. + {$effective_pnpm_version}' "$output/provenance.json" > "$output/provenance.tmp"
mv "$output/provenance.tmp" "$output/provenance.json"
pnpm install --frozen-lockfile > "$output/pnpm-install.log" 2>&1
cd bench-browser
pnpm exec tsc --noEmit > "$output/typecheck.log" 2>&1
args=(matrix --condition ff-rdp --model claude-sonnet-4-6 --repeat "$repeat")
[ -z "$tasks" ] || args+=(--task "$tasks")
# Validate all task names and the actual denominator before any paid invocation.
pnpm exec tsx -e 'import fs from "node:fs"; import {parse} from "yaml"; const tasks=parse(fs.readFileSync("config/tasks.yaml","utf8")).tasks; const selected=process.argv[1]?process.argv[1].split(","):Object.keys(tasks); if(selected.some(t=>!tasks[t])||new Set(selected).size!==selected.length)throw Error("Unknown/duplicate task"); console.log(JSON.stringify({tasks:selected,task_count:selected.length,repeat:Number(process.argv[2]),runs:selected.length*Number(process.argv[2])}));' "$tasks" "$repeat" > "$output/matrix.json"
[ "$(git -C "$source_root" rev-parse HEAD)" = "$revision" ]
[ -z "$(git -C "$source_root" status --porcelain --untracked-files=normal)" ]
if [ "$prepare" = 1 ]; then printf '0\n' > "$output/prepare.exit"; exit 0; fi
set +e
env -u ANTHROPIC_API_KEY pnpm bench "${args[@]}" > "$output/matrix.log" 2>&1
status=$?
set -e
printf '%s\n' "$status" > "$output/matrix.exit"
exit "$status"
