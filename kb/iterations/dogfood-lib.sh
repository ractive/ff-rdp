# shellcheck shell=bash
# kb/iterations/dogfood-lib.sh — shared helpers for the checked-in dogfood gates.
#
# Sourced by every `kb/iterations/*.dogfood.sh`. It exists to keep two
# invariants that iteration 193 had to retrofit onto sixteen scripts:
#
#   1. **Run the binary under test.** `ffrdp` invokes a binary freshly built
#      from *this* working tree (`cargo build -p ff-rdp-cli`), never whatever
#      `ff-rdp` happens to be first on `$PATH`. A stale PATH binary makes a
#      dogfood gate certify a build that is not the one on the branch — the
#      same false-PASS shape iteration 184 fixed one layer down.
#
#   2. **Own only your own browser.** `dogfood_launch` records the port and pid
#      of the Firefox *this run* started; `dogfood_teardown` — wired to an EXIT
#      trap by `dogfood_init` — stops exactly those and nothing else. Until
#      iteration 193 every script opened with
#      `pkill -f 'firefox.*ff-rdp-profile'`, which on a machine where several
#      agents share one working tree (the normal case in this project's loop)
#      terminates browsers the script does not own. That is why iteration 184
#      could not execute a single migrated script to prove its own migration.
#
# `dogfood_init` also points `$FF_RDP_HOME` at a private per-run directory, so
# the profile root, the daemon registry and the connection records this run
# creates are invisible to — and untouchable by — every other run on the
# machine. That is what makes `profiles prune --all` (iterations 96 and 97)
# safe to execute on a shared box: it can only reach this run's own profiles.
#
# Everything here is bash-3.2 compatible (macOS still ships 3.2), which is why
# the bookkeeping uses space-separated strings rather than arrays: under
# `set -u` bash 3.2 errors on `"${arr[@]}"` for an empty array.

# --- state -----------------------------------------------------------------

FF_RDP_DOGFOOD_REPO_ROOT=""
FF_RDP_DOGFOOD_BIN=""
FF_RDP_DOGFOOD_PORTS=""
FF_RDP_DOGFOOD_PIDS=""
FF_RDP_DOGFOOD_HOOKS=""
FF_RDP_DOGFOOD_PRIVATE_HOME=""
FF_RDP_DOGFOOD_TORN_DOWN=0

# --- helpers ---------------------------------------------------------------

dogfood_log() {
  printf '[dogfood-lib] %s\n' "$*" >&2
}

dogfood_die() {
  printf '[dogfood-lib] FAIL: %s\n' "$*" >&2
  exit 1
}

# Resolve the repository root from this file's own location, so a script works
# from any cwd and without shelling out to git.
dogfood_repo_root() {
  if [ -z "$FF_RDP_DOGFOOD_REPO_ROOT" ]; then
    FF_RDP_DOGFOOD_REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
  fi
  printf '%s\n' "$FF_RDP_DOGFOOD_REPO_ROOT"
}

# Build the CLI from this tree and remember where the binary landed.
#
# `cargo build` rather than a `cargo run` per call: a dogfood script makes
# dozens of CLI invocations and several of them are inside timing assertions
# (iteration 85 Theme C budgets a navigate at under 3000 ms), so paying cargo's
# freshness check on every one would measure cargo, not ff-rdp. Building once
# up front and then executing the produced binary is the "freshly built binary
# from the tree under test" form — it is exactly as branch-faithful as
# `cargo run -p ff-rdp-cli --`, and unlike a PATH lookup it cannot resolve to
# something else.
dogfood_build_cli() {
  local root target
  root="$(dogfood_repo_root)"
  target="${CARGO_TARGET_DIR:-$root/target}"

  cargo build --quiet --manifest-path "$root/Cargo.toml" -p ff-rdp-cli --bin ff-rdp \
    || dogfood_die "cargo build -p ff-rdp-cli failed — cannot run the binary under test"

  FF_RDP_DOGFOOD_BIN="$target/debug/ff-rdp"
  [ -x "$FF_RDP_DOGFOOD_BIN" ] \
    || dogfood_die "built binary not found at $FF_RDP_DOGFOOD_BIN"
  dogfood_log "binary under test: $FF_RDP_DOGFOOD_BIN"
}

# Run the binary under test. Every dogfood script calls the CLI through this.
ffrdp() {
  [ -n "$FF_RDP_DOGFOOD_BIN" ] || dogfood_die "ffrdp called before dogfood_init"
  "$FF_RDP_DOGFOOD_BIN" "$@"
}

# Print a TCP port nothing is currently listening on.
#
# A per-run port is half of "own only your own browser": two runs that both
# hardcode 6000 fight over the same Firefox no matter how careful their
# teardown is, and a run that adopts a port it did not open cannot tell its own
# browser from a sibling's.
dogfood_free_port() {
  local port tries=0
  while [ "$tries" -lt 60 ]; do
    port=$(( 20000 + RANDOM % 20000 ))
    if ! nc -z 127.0.0.1 "$port" >/dev/null 2>&1; then
      printf '%s\n' "$port"
      return 0
    fi
    tries=$(( tries + 1 ))
  done
  dogfood_die "could not find a free TCP port after 60 attempts"
}

dogfood_record_port() {
  case " $FF_RDP_DOGFOOD_PORTS " in
    *" $1 "*) ;;
    *) FF_RDP_DOGFOOD_PORTS="$FF_RDP_DOGFOOD_PORTS $1" ;;
  esac
}

dogfood_record_pid() {
  [ -n "${1:-}" ] || return 0
  [ "$1" != "null" ] || return 0
  case " $FF_RDP_DOGFOOD_PIDS " in
    *" $1 "*) ;;
    *) FF_RDP_DOGFOOD_PIDS="$FF_RDP_DOGFOOD_PIDS $1" ;;
  esac
}

# Register an extra cleanup function, run (in registration order) before the
# standard teardown. Use this instead of installing a second EXIT trap, which
# would silently replace `dogfood_teardown`.
dogfood_on_exit() {
  FF_RDP_DOGFOOD_HOOKS="$FF_RDP_DOGFOOD_HOOKS $1"
}

# Launch Firefox on `$1`, recording the port and pid so teardown can reach
# exactly this browser. Extra arguments are passed through to `launch`.
# Prints the full launch JSON on stdout.
dogfood_launch() {
  local port="$1" json pid
  shift
  json="$(ffrdp launch --headless --port "$port" "$@")" || return 1
  dogfood_record_port "$port"
  pid="$(dogfood_json_number "$json" pid)"
  dogfood_record_pid "$pid"
  printf '%s\n' "$json"
}

# Extract a top-level numeric field from `results` in a CLI JSON envelope,
# without depending on jq or python3 (not every dogfood script has either).
# Returns the empty string when the field is absent.
dogfood_json_number() {
  printf '%s' "$1" \
    | tr ',{}' '\n\n\n' \
    | sed -n "s/.*\"$2\"[[:space:]]*:[[:space:]]*\([0-9][0-9]*\).*/\1/p" \
    | head -1
}

# Stop the daemon/Firefox this run opened on `$1`. Best effort and quiet: the
# port may already be free because the script under test stopped it itself.
dogfood_stop_port() {
  ffrdp --port "$1" daemon stop >/dev/null 2>&1 || true
}

# Terminate a pid this run launched, escalating TERM → KILL.
dogfood_stop_pid() {
  local pid="$1" i
  kill -0 "$pid" >/dev/null 2>&1 || return 0
  kill -TERM "$pid" >/dev/null 2>&1 || true
  for i in 1 2 3 4 5 6 7 8 9 10; do
    kill -0 "$pid" >/dev/null 2>&1 || return 0
    sleep 0.5
  done
  kill -KILL "$pid" >/dev/null 2>&1 || true
}

# Stop everything this run started — and nothing else.
dogfood_teardown() {
  local status=$? hook port pid
  [ "$FF_RDP_DOGFOOD_TORN_DOWN" -eq 0 ] || return "$status"
  FF_RDP_DOGFOOD_TORN_DOWN=1

  for hook in $FF_RDP_DOGFOOD_HOOKS; do
    "$hook" || true
  done
  for port in $FF_RDP_DOGFOOD_PORTS; do
    dogfood_stop_port "$port"
  done
  for pid in $FF_RDP_DOGFOOD_PIDS; do
    dogfood_stop_pid "$pid"
  done
  if [ -n "$FF_RDP_DOGFOOD_PRIVATE_HOME" ] && [ -d "$FF_RDP_DOGFOOD_PRIVATE_HOME" ]; then
    rm -rf "$FF_RDP_DOGFOOD_PRIVATE_HOME"
  fi
  return "$status"
}

# Entry point. Call once, immediately after the SENTINEL assignment.
dogfood_init() {
  dogfood_repo_root >/dev/null

  # Private per-user state root: profiles, daemon registry and connection
  # records all follow $FF_RDP_HOME, so nothing this run creates is visible to
  # a sibling run and nothing this run prunes can belong to one.
  FF_RDP_DOGFOOD_PRIVATE_HOME="$(mktemp -d -t ff-rdp-dogfood-home-XXXXXX)"
  export FF_RDP_HOME="$FF_RDP_DOGFOOD_PRIVATE_HOME"

  trap dogfood_teardown EXIT
  dogfood_build_cli
}
