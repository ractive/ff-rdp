#!/usr/bin/env bash
# iter-252 dogfood gate — does any *other* direct-route watcher starve on
# content-process resources the way iteration 174's navigation waits did?
#
# The audit leaves exactly one candidate: `console --follow`'s direct path,
# which subscribes to `console-message` + `error-message` — both members of
# `FrameTargetResources` in `devtools/server/actors/resources/index.js`, i.e.
# emitted only from the content process, per frame target. Every other
# remaining `get_watcher` call site subscribes to `network-event` or `cookies`,
# which live in `ParentProcessResources` and are therefore immune.
#
# This is the measurement harness iteration 174 lacked. 174 tried the same
# comparison, saw empty stdout on *both* routes inside an 8 s window and
# concluded nothing. Two things it was missing, both of which turned out to be
# defects rather than harness problems:
#
#   1. the direct route never received a single `console-message` resource
#      (no `isServerTargetSwitchingEnabled`, no `watchTargets("frame")`);
#   2. neither route could *print* one, because the parser understood only the
#      nested `consoleAPICall` shape and a `console-message` resource is flat.
#
# Both routes are measured against the same page, the same probe source and the
# same window, so a difference between them is the defect and not the harness —
# and a *daemon* route that goes silent fails the run outright, because a
# harness that measures nothing must never read as a clean result.
#
# The probe is page-driven (a `setInterval` armed once, up front) rather than a
# per-route `eval`: a daemon-routed `console --follow` holds the daemon's single
# RPC-writer slot for its whole lifetime, so a second daemon command fired
# underneath it would time out with `daemon_busy` and the daemon leg would look
# starved for a reason that has nothing to do with this audit.
#
# Run manually:
#   FF_RDP_LIVE_TESTS=1 cargo run -p xtask -- check-dogfood-script \
#     kb/iterations/iteration-252-content-process-resources-on-the-direct-route.md
set -euo pipefail

# shellcheck source=kb/iterations/dogfood-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/dogfood-lib.sh"

SENTINEL="${FF_RDP_DOGFOOD_SENTINEL:?set by check-dogfood-script; run this script via: cargo run -p xtask -- check-dogfood-script <plan.md>}"
rm -f "$SENTINEL"

dogfood_init

# The page logs once a second; 8 s of observation leaves a wide margin over the
# ~1 s a healthy route needs while staying quick.
OBSERVE_SECONDS="${ITER252_OBSERVE_SECONDS:-8}"
PROBE="iter252tick"

PORT="$(dogfood_free_port)"
WORK="$(dogfood_workdir)"

dogfood_launch "$PORT"
sleep 2

# `about:blank` is enough: it is a content-process document like any other, and
# it keeps this gate free of both the network and the `--allow-file-urls` opt-in.
ffrdp --port "$PORT" --no-daemon navigate about:blank >"$WORK/navigate.json"

ffrdp --port "$PORT" --no-daemon \
  eval "setInterval(function(){console.log('$PROBE')},1000); 'armed'" \
  >"$WORK/arm.json"

# Stream `console --follow` over one route for the observation window and print
# how many probe lines it produced. $1 is the label; the remaining arguments are
# the global flags that select the route (none for the daemon route).
#
# Backgrounded rather than run under `timeout`, because `ffrdp` is a shell
# function (the dogfood-lib helper that guarantees the binary under test) and
# `timeout` can only exec a real program.
measure_route() {
  local label="$1"
  shift
  local out="$WORK/follow-$label.out"
  local follow_pid seen

  : >"$out"
  : >"$WORK/follow-$label.err"

  ffrdp --port "$PORT" "$@" console --follow --pattern "$PROBE" \
    >"$out" 2>"$WORK/follow-$label.err" &
  follow_pid=$!

  sleep "$OBSERVE_SECONDS"

  kill "$follow_pid" 2>/dev/null || true
  wait "$follow_pid" 2>/dev/null || true

  seen=$(grep -c "$PROBE" "$out" || true)
  printf '%s\n' "${seen:-0}"
}

DIRECT_LINES="$(measure_route direct --no-daemon)"
DAEMON_LINES="$(measure_route daemon)"

echo "iter-252: window=${OBSERVE_SECONDS}s (page logs at 1 Hz) direct=$DIRECT_LINES daemon=$DAEMON_LINES"

# The daemon leg is the control. If it is silent the harness is not measuring
# anything and the direct result is not evidence either way — the exact trap
# iteration 174 fell into, which is why this is checked first and separately.
test "$DAEMON_LINES" -gt 0 || {
  echo "FAIL: the daemon route delivered no console-message resources — the harness" >&2
  echo "      measured nothing, so the direct result below is not evidence." >&2
  head -c 300 "$WORK/follow-daemon.err" >&2
  exit 1
}

test "$DIRECT_LINES" -gt 0 || {
  echo "FAIL: the direct route delivered no console-message resources. This is the" >&2
  echo "      iteration-174 shape: console --follow subscribes to content-process" >&2
  echo "      resources, so its watcher needs isServerTargetSwitchingEnabled:true" >&2
  echo "      and a watchTargets(\"frame\") before watchResources, or the" >&2
  echo "      subscription is acked into silence." >&2
  head -c 300 "$WORK/follow-direct.err" >&2
  exit 1
}

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
echo "iter-252 dogfood: console --follow delivers content-process resources on both routes — $SENTINEL"
