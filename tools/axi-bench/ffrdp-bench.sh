#!/bin/bash
# Private lifecycle for the patched upstream harness (bash 3.2, macOS/Linux).
set -eu
# shellcheck source=kb/iterations/dogfood-lib.sh
. "${FF_RDP_BENCH_SOURCE_ROOT:?}/kb/iterations/dogfood-lib.sh"
FF_RDP_DOGFOOD_BIN="${FF_RDP_BENCH_BINARY:?}"
state="${FF_RDP_BENCH_STATE:?}"
export FF_RDP_HOME="$state/home"

fingerprint() { LC_ALL=C ps -p "$1" -o lstart= -o command=; }
matches() {
  local current
  current="$(fingerprint "$1")" || return 1
  [ "$current" = "$2" ] || return 1
  case "$current" in *"$3"*) return 0;; *) return 1;; esac
}
stop_record() {
  local record="$1" expected="$2" pid identity i
  [ -f "$record" ] || return 0
  pid="$(sed -n '1p' "$record")"
  identity="$(sed -n '2p' "$record")"
  case "$pid" in ''|*[!0-9]*) echo "Invalid owner record: $record" >&2; return 1;; esac
  if ! kill -0 "$pid" 2>/dev/null; then rm "$record"; return 0; fi
  if ! matches "$pid" "$identity" "$expected"; then
    echo "Refusing unowned PID $pid ($record)" >&2
    return 1
  fi
  printf 'TERM pid=%s identity=%s\n' "$pid" "$identity" >> "$state/ownership.log"
  kill -TERM "$pid" 2>/dev/null || true
  for i in 1 2 3 4 5 6 7 8 9 10; do
    matches "$pid" "$identity" "$expected" || { rm "$record"; return 0; }
    sleep 0.2
  done
  # Revalidate immediately before escalation; never signal a reused PID.
  if matches "$pid" "$identity" "$expected"; then kill -KILL "$pid" 2>/dev/null || true; fi
  for i in 1 2 3 4 5 6 7 8 9 10; do
    matches "$pid" "$identity" "$expected" || { rm "$record"; return 0; }
    sleep 0.2
  done
  echo "Owned PID $pid survived cleanup" >&2
  return 1
}
record_daemon() {
  local registry="$FF_RDP_HOME/.ff-rdp/daemon.6000.json" pid identity
  [ -f "$registry" ] || return 0
  pid="$(jq -er '.pid | select(type == "number")' "$registry")" || return 1
  identity="$(fingerprint "$pid")" || return 0
  case "$identity" in *"$FF_RDP_DOGFOOD_BIN"*' _daemon '*) ;; *) echo "Unowned daemon $pid: $identity" >&2; return 1;; esac
  printf '%s\n%s\n' "$pid" "$identity" > "$state/daemon.owner"
}
stop() {
  local status=0
  # Discover daemons started by the agent too, using this unique build path and
  # private registry. No daemon-stop RPC can accidentally target an intruder.
  record_daemon || status=1
  stop_record "$state/daemon.owner" "$FF_RDP_DOGFOOD_BIN" || status=1
  stop_record "$state/firefox.owner" "$state/profile" || status=1
  if [ "$status" = 0 ]; then rm -rf "${state:?}/profile" "${state:?}/home"; fi
  return "$status"
}
start() {
  [ ! -e "$state/firefox.owner" ] && [ ! -e "$state/daemon.owner" ] || { echo 'Previous lifecycle not stopped' >&2; return 1; }
  if lsof -nP -iTCP:6000 -sTCP:LISTEN >/dev/null 2>&1; then
    echo 'Refusing occupied port 6000' >&2; return 1
  fi
  mkdir -p "$FF_RDP_HOME" "$state/profile"
  cat > "$state/profile/user.js" <<'PREFS'
user_pref("devtools.debugger.remote-enabled", true);
user_pref("devtools.chrome.enabled", true);
user_pref("devtools.debugger.prompt-connection", false);
PREFS
  "${FF_RDP_BENCH_FIREFOX:?}" -no-remote -profile "$state/profile" --start-debugger-server 6000 --headless > "$state/firefox.log" 2>&1 &
  local pid=$! identity i
  # Wait for exec so the stored identity cannot be the transient launching shell.
  for i in $(seq 1 100); do
    identity="$(fingerprint "$pid")" || { wait "$pid" || true; return 1; }
    case "$identity" in *"$state/profile"*) break;; esac
    sleep 0.01
  done
  printf '%s\n%s\n' "$pid" "$identity" > "$state/firefox.owner"
  printf 'START pid=%s identity=%s\n' "$pid" "$identity" >> "$state/ownership.log"
  i=0
  while [ "$i" -lt 300 ]; do
    i=$((i + 1))
    if lsof -a -p "$pid" -iTCP:6000 -sTCP:LISTEN >/dev/null 2>&1; then
      # Record the post-exec identity, and verify profile before any cleanup.
      identity="$(fingerprint "$pid")"
      printf '%s\n%s\n' "$pid" "$identity" > "$state/firefox.owner"
      if ! ffrdp --port 6000 tabs > "$state/health.json"; then stop; return 1; fi
      record_daemon
      return 0
    fi
    kill -0 "$pid" 2>/dev/null || { stop; return 1; }
    sleep 0.1
  done
  echo 'Owned Firefox did not listen on 6000 in 30 seconds' >&2
  stop
  return 1
}

case "${1:-}" in
  start) start;;
  stop) stop;;
  *) echo 'Usage: ffrdp-bench.sh start|stop' >&2; exit 2;;
esac
