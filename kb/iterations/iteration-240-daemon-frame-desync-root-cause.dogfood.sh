#!/usr/bin/env bash
# iter-240 dogfood gate — the CLI↔daemon frame stream, sustained, against the
# real page.
#
# Drives HOPS rounds of `navigate --with-page` + `click --ref … --with-page`
# against a real Wikipedia article through the daemon, and asserts the two
# things iterations 224/226/227 could not get:
#
#   1. `~/.ff-rdp/daemon.log` gains **no** `abandoning client` line;
#   2. every hop reports `meta.page_reconnects: 0` — a reconnect means the
#      daemon dropped the connection, which is the defect, not the fix;
#   3. the daemon is still healthy at the end, and can say so
#      (`dispatcher.alive`, `dispatcher.in_flight`, `clients_dropped_on_write`).
#
# The live suite (`tests/live/live_240_daemon_frame_desync_and_wedge.rs`) runs
# the same loop against a local fixture. This one exists because the acceptance
# criteria say *the real page*: the desync only ever appeared against a real
# remote origin and a Wikipedia-sized document.
#
# Needs the network (FF_RDP_LIVE_NETWORK_TESTS=1), not just a browser.
#
# Run manually:
#   FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
#     bash kb/iterations/iteration-240-*.dogfood.sh
set -euo pipefail

# shellcheck source=kb/iterations/dogfood-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/dogfood-lib.sh"

SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?set by check-dogfood-script; run this script via: cargo run -p xtask -- check-dogfood-script <plan.md>}"
rm -f "$SENTINEL"

if [ "${FF_RDP_LIVE_NETWORK_TESTS:-0}" != "1" ]; then
  echo "iter-240 dogfood: needs FF_RDP_LIVE_NETWORK_TESTS=1 (it drives a real page) — skipping" >&2
  date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
  exit 0
fi

# 60, the count the Part B acceptance criterion asks for; it also clears Part
# A's 40. Both recorded wedges hit well inside this range (hop 13 and hop 26).
HOPS="${FF_RDP_DOGFOOD_HOPS:-60}"
PAGE='https://en.wikipedia.org/wiki/Python_(programming_language)'
LINK='Python Software Foundation'

dogfood_init
PORT="$(dogfood_free_port)"
dogfood_launch "$PORT"
sleep 2

# `dogfood_init` points FF_RDP_HOME at a private per-run directory, so this is
# *this run's* daemon log and nothing else's.
LOG="$FF_RDP_HOME/.ff-rdp/daemon.log"

RECONNECT_TOTAL=0
FAILED=0
for i in $(seq 1 "$HOPS"); do
  REF="$(ffrdp --port "$PORT" navigate "$PAGE" --with-page \
    --jq "[.results.page.interactive[] | select(.name == \"$LINK\")][0].ref" | tr -d '"')"
  if [ -z "$REF" ] || [ "$REF" = "null" ]; then
    echo "FAIL hop $i: the article carried no ref for '$LINK'" >&2
    FAILED=$((FAILED + 1))
    continue
  fi

  # One invocation, parsed twice: a second click would be a second hop.
  OUT="$(ffrdp --port "$PORT" click --ref "$REF" --with-page)"
  case "$OUT" in
    *'"error_type"'*)
      echo "FAIL hop $i: click returned an error envelope: $OUT" >&2
      FAILED=$((FAILED + 1))
      continue
      ;;
  esac
  RECONNECTS="$(dogfood_json_number "$OUT" page_reconnects)"
  if [ -z "$RECONNECTS" ]; then
    echo "FAIL hop $i: meta.page_reconnects was not reported" >&2
    FAILED=$((FAILED + 1))
    continue
  fi
  RECONNECT_TOTAL=$((RECONNECT_TOTAL + RECONNECTS))
done

test "$FAILED" -eq 0 || { echo "FAIL: $FAILED of $HOPS hops failed" >&2; exit 1; }
test "$RECONNECT_TOTAL" -eq 0 || {
  echo "FAIL: $RECONNECT_TOTAL reconnect(s) across $HOPS hops — the daemon dropped the connection" >&2
  exit 1
}

# The line iteration 240 exists to remove.
if [ -f "$LOG" ] && grep -q 'abandoning client' "$LOG"; then
  echo "FAIL: the daemon abandoned a client:" >&2
  grep 'abandoning client' "$LOG" >&2
  exit 1
fi

# And the daemon can still describe its own health.
ALIVE="$(ffrdp --port "$PORT" daemon status --jq '.results.dispatcher.alive')"
test "$ALIVE" = "true" || { echo "FAIL: dispatcher.alive=$ALIVE after $HOPS hops" >&2; exit 1; }
IN_FLIGHT="$(ffrdp --port "$PORT" daemon status --jq '.results.dispatcher.in_flight')"
test "$IN_FLIGHT" = "0" || { echo "FAIL: dispatcher.in_flight=$IN_FLIGHT — the daemon is wedged" >&2; exit 1; }
DROPPED="$(ffrdp --port "$PORT" daemon status --jq '.results.clients_dropped_on_write')"
test "$DROPPED" = "0" || { echo "FAIL: clients_dropped_on_write=$DROPPED" >&2; exit 1; }

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
echo "iter-240 dogfood: $HOPS hops, 0 reconnects, 0 abandoned clients, dispatcher healthy — $SENTINEL"
