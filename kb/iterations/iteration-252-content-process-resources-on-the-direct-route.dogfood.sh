#!/usr/bin/env bash
# iteration-252 — does any *other* direct-route watcher starve on
# content-process resources the way iteration 174's navigation waits did?
#
# The audit (see the plan) leaves exactly one candidate: `console --follow`'s
# direct path, which subscribes to `console-message` + `error-message` — both
# members of `FrameTargetResources` in
# `devtools/server/actors/resources/index.js`, i.e. emitted only from the
# content process, per frame target. Every other remaining `get_watcher` call
# site subscribes to `network-event` or `cookies`, which live in
# `ParentProcessResources` and are therefore immune.
#
# This script is the measurement harness iteration 174 lacked. 174 tried the
# same comparison, saw empty stdout on *both* routes inside an 8 s window and
# concluded nothing. The two things it was missing:
#
#   1. a page that actually runs in the content process (`about:blank` and a
#      freshly-launched browser can leave you on a parent-process document);
#   2. enough settling time between arming the subscription and emitting the
#      probe — `watchResources` is asynchronous on the server and a probe fired
#      immediately after the ack races the subscription.
#
# Both routes are measured with the same code path, the same page and the same
# timings, so a difference between them is the defect and not the harness.
set -euo pipefail

# shellcheck source=kb/iterations/dogfood-lib.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/dogfood-lib.sh"
dogfood_init

ARM_SECONDS="${ITER252_ARM_SECONDS:-4}"
OBSERVE_SECONDS="${ITER252_OBSERVE_SECONDS:-6}"

PORT="$(dogfood_free_port)"
WORK="$(dogfood_workdir)"

dogfood_log "launching headless Firefox on port $PORT"
dogfood_launch "$PORT" || dogfood_die "launch failed"

PAGE="$WORK/iter252-probe.html"
cat >"$PAGE" <<'HTML'
<!doctype html>
<meta charset="utf-8">
<title>iter-252 console-follow probe</title>
<body>iter-252 probe page</body>
HTML
PAGE_URL="file://$PAGE"

# `file://` is off by default (it makes local files exfiltratable through
# `eval`/`page-text`); the page here is one this script just wrote into its own
# private workdir, so opting in is bounded by the run.
dogfood_log "navigating to $PAGE_URL"
ffrdp --port "$PORT" --allow-file-urls navigate "$PAGE_URL" >"$WORK/navigate.json" 2>&1 \
  || dogfood_die "navigate failed: $(cat "$WORK/navigate.json")"

# Measure one route. $1 is the label used in the probe string and the output
# file names; the remaining arguments are the global flags that select the
# route (empty for the daemon route).
measure_route() {
  local label="$1"
  shift
  local out="$WORK/follow-$label.out"
  local err="$WORK/follow-$label.err"
  local probe="iter252-$label-probe"
  local follow_pid

  : >"$out"
  : >"$err"

  ffrdp --port "$PORT" "$@" console --follow --pattern iter252 >"$out" 2>"$err" &
  follow_pid=$!

  sleep "$ARM_SECONDS"

  if ! kill -0 "$follow_pid" 2>/dev/null; then
    dogfood_log "$label: console --follow exited before the probe was emitted"
    dogfood_log "$label: stderr: $(head -c 400 "$err")"
    printf 'FOLLOW-DIED\n'
    return 0
  fi

  ffrdp --port "$PORT" "$@" eval "console.log('$probe')" \
    >"$WORK/eval-$label.json" 2>&1 \
    || dogfood_log "$label: eval reported failure: $(head -c 300 "$WORK/eval-$label.json")"

  sleep "$OBSERVE_SECONDS"

  kill "$follow_pid" 2>/dev/null || true
  wait "$follow_pid" 2>/dev/null || true

  if grep -q "$probe" "$out"; then
    printf 'OBSERVED\n'
  else
    printf 'SILENT\n'
  fi
}

dogfood_log "measuring the daemon route"
DAEMON_RESULT="$(measure_route daemon)"
dogfood_log "daemon route: $DAEMON_RESULT"

dogfood_log "measuring the direct route (--no-daemon)"
DIRECT_RESULT="$(measure_route direct --no-daemon)"
dogfood_log "direct route: $DIRECT_RESULT"

printf '\n=== iteration-252 console --follow route comparison ===\n'
printf 'daemon route : %s\n' "$DAEMON_RESULT"
printf 'direct route : %s\n' "$DIRECT_RESULT"

FAILED=0

if [ "$DAEMON_RESULT" != "OBSERVED" ]; then
  printf 'FAIL: the daemon route did not deliver its own console probe — the\n'
  printf '      harness is not measuring anything and neither result is evidence.\n'
  FAILED=1
fi

if [ "$DIRECT_RESULT" != "OBSERVED" ]; then
  printf 'FAIL: the direct route did not deliver its own console probe. This is\n'
  printf '      the iteration-174 defect in a second place: console --follow\n'
  printf '      subscribes to content-process resources through a watcher\n'
  printf '      obtained without isServerTargetSwitchingEnabled.\n'
  FAILED=1
fi

if [ "$FAILED" -eq 0 ]; then
  printf 'PASS: both routes deliver console-message resources.\n'
fi

exit "$FAILED"
