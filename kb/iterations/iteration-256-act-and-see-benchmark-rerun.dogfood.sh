#!/usr/bin/env bash
set -euo pipefail
# shellcheck source=kb/iterations/dogfood-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/dogfood-lib.sh"
SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?run via check-dogfood-script}"
rm -f "$SENTINEL"
[ "${FF_RDP_LIVE_NETWORK_TESTS:-}" = 1 ] || dogfood_die 'requires FF_RDP_LIVE_NETWORK_TESTS=1'
dogfood_init
PORT="$(dogfood_free_port)"
dogfood_launch "$PORT"
WORK="$(dogfood_workdir)"
ffrdp --port "$PORT" navigate https://en.wikipedia.org/wiki/Ada_Lovelace --with-page --query Babbage > "$WORK/ada.json"
cat "$WORK/ada.json"
REF="$(jq -er '[.results.page.interactive[] | select(.name | test("Charles Babbage")) | .ref][0]' "$WORK/ada.json")"
ffrdp --port "$PORT" click --ref "$REF" --with-page > "$WORK/babbage.json"
cat "$WORK/babbage.json"
jq -e '.results.page.headings | any(.text == "Charles Babbage")' "$WORK/babbage.json" >/dev/null
ffrdp --port "$PORT" home --hook > "$WORK/hook.txt"
cat "$WORK/hook.txt"
rg -q 'Charles Babbage' "$WORK/hook.txt"
date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
