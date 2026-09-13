#!/usr/bin/env bash
set -euo pipefail
# shellcheck source=kb/iterations/dogfood-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/dogfood-lib.sh"
SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?run via check-dogfood-script}"
rm -f "$SENTINEL"
[ "${FF_RDP_LIVE_NETWORK_TESTS:-}" = 1 ] || dogfood_die "requires FF_RDP_LIVE_NETWORK_TESTS=1"
dogfood_init
PORT="$(dogfood_free_port)"
dogfood_launch "$PORT"
WORK="$(dogfood_workdir)"
ffrdp --port "$PORT" navigate 'https://en.wikipedia.org/wiki/Python_(programming_language)' \
  --with-page --query Developer > "$WORK/python.json"
cat "$WORK/python.json"
REF="$(jq -er '.results.page.facts[] | select(.key == "Developer") | .links[] | select(.href | contains("/wiki/Python_Software_Foundation")) | .ref' "$WORK/python.json" | head -1)"
[ -n "$REF" ] || dogfood_die "Developer fact has no PSF ref"
ffrdp --port "$PORT" click --ref "$REF" --with-page --query 'founded formed' > "$WORK/psf.json"
cat "$WORK/psf.json"
jq -e '.results.page.facts | any(.key == "Formation")' "$WORK/psf.json" >/dev/null
ffrdp --port "$PORT" eval location.href > "$WORK/url.json"
cat "$WORK/url.json"
jq -e '.. | strings | select(. == "https://en.wikipedia.org/wiki/Python_Software_Foundation")' "$WORK/url.json" >/dev/null
date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
