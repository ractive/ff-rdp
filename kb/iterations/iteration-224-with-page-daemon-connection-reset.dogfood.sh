#!/usr/bin/env bash
# iter-224 dogfood gate — a connection that dies mid-collection must not cost
# the caller the action.
#
# Drives the exact hop the defect was found on: `navigate --with-page` to pick
# up a ref, then `click --ref … --with-page` to follow it, N times over the
# daemon route. On `5a0071d` this returned
#   {"error":"recv failed: Connection reset by peer (os error 54)",
#    "error_type":"Transport"}   exit 6, in ~0.45 s
# roughly once in fifteen hops against `en.wikipedia.org`. Every hop must now
# return the destination view.
#
# The remote page is the point — the flake never reproduced against a local
# fixture (0 in 90 during development), which is why the live suite's
# `live_224_*` covers the contract and this script covers the original hop.
# It therefore needs FF_RDP_LIVE_NETWORK_TESTS=1 as well, and skips (exit 0,
# sentinel written) when that is not set: a gate that cannot reach the network
# must not fail the run, and must not silently claim to have proved anything.
#
# Run manually:
#   FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
#     bash kb/iterations/iteration-224-*.dogfood.sh
set -euo pipefail

# shellcheck source=kb/iterations/dogfood-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/dogfood-lib.sh"

SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?set by check-dogfood-script; run this script via: cargo run -p xtask -- check-dogfood-script <plan.md>}"
rm -f "$SENTINEL"

URL='https://en.wikipedia.org/wiki/Python_(programming_language)'
LINK='Python Software Foundation'
HOPS="${FF_RDP_DOGFOOD_HOPS:-15}"

if [ "${FF_RDP_LIVE_NETWORK_TESTS:-0}" != "1" ]; then
  echo "iter-224 dogfood: SKIPPED — needs FF_RDP_LIVE_NETWORK_TESTS=1 (the hop is against ${URL})" >&2
  date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
  exit 0
fi

dogfood_init
PORT="$(dogfood_free_port)"

dogfood_launch "$PORT"
sleep 2

FAILED=0
RECONNECTS=0

for i in $(seq 1 "$HOPS"); do
  # `--jq` on a missing path stays silent by default, so an empty REF means the
  # navigate itself failed — reported rather than swallowed.
  REF=$(ffrdp --port "$PORT" navigate "$URL" --with-page \
    --jq "[.results.page.interactive[] | select(.name == \"$LINK\")][0].ref" | tr -d '"')
  if [ -z "$REF" ] || [ "$REF" = "null" ]; then
    echo "FAIL hop $i: navigate --with-page returned no ref for '$LINK'" >&2
    FAILED=$((FAILED + 1))
    continue
  fi

  # `click` exits non-zero on the defect, so capture both halves before
  # `set -e` can end the loop.
  set +e
  OUT=$(ffrdp --port "$PORT" click --ref "$REF" --with-page \
    --jq '{h: .results.page.headings[0].text, et: .error_type, rc: .meta.page_reconnects}' 2>&1)
  RC=$?
  set -e

  case "$OUT" in
    *'"Transport"'* | *'"RemoteClosed"'*)
      echo "FAIL hop $i: the connection died mid-collection — this is the iter-224 defect: $OUT" >&2
      FAILED=$((FAILED + 1))
      continue
      ;;
  esac
  if [ "$RC" != "0" ]; then
    echo "FAIL hop $i: click --ref --with-page exited $RC: $OUT" >&2
    FAILED=$((FAILED + 1))
    continue
  fi
  case "$OUT" in
    *"$LINK"*) ;;
    *)
      echo "FAIL hop $i: click --with-page did not report the destination heading: $OUT" >&2
      FAILED=$((FAILED + 1))
      continue
      ;;
  esac
  case "$OUT" in
    *'"rc":0'*) ;;
    *'"rc":'*) RECONNECTS=$((RECONNECTS + 1)) ;;
    *)
      echo "FAIL hop $i: meta.page_reconnects missing — the cost of the view must always be reported: $OUT" >&2
      FAILED=$((FAILED + 1))
      ;;
  esac
done

test "$FAILED" = "0" || { echo "FAIL: $FAILED of $HOPS hops failed" >&2; exit 1; }

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
echo "iter-224 dogfood: $HOPS/$HOPS hops returned the destination view ($RECONNECTS absorbed a reconnect) — $SENTINEL"
