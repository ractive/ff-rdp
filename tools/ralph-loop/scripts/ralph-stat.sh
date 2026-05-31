#!/usr/bin/env bash
set -euo pipefail

# ralph-stat.sh — Render ralph-loop state as a human ledger or JSON.
#
# Usage:
#   ralph-stat.sh              # human-readable ledger to stdout
#   ralph-stat.sh --json       # raw state.json
#   ralph-stat.sh --decision   # one-line summary of next action (for tick logs)
#
# Resolves the cache dir from $RALPH_CACHE_DIR if set, otherwise
# ~/.cache/ralph-loop/$(basename $(git rev-parse --show-toplevel)).

resolve_cache_dir() {
  if [[ -n "${RALPH_CACHE_DIR:-}" ]]; then
    echo "$RALPH_CACHE_DIR"
    return
  fi
  local root
  root=$(git rev-parse --show-toplevel 2>/dev/null) || {
    echo "ERROR: not in a git repo and RALPH_CACHE_DIR not set" >&2
    exit 2
  }
  echo "$HOME/.cache/ralph-loop/$(basename "$root")"
}

CACHE_DIR=$(resolve_cache_dir)
STATE_FILE="$CACHE_DIR/state.json"

if [[ ! -f "$STATE_FILE" ]]; then
  echo "No ralph-loop state at $STATE_FILE"
  echo "(run preflight.sh to seed)"
  exit 1
fi

mode="${1:-ledger}"

if [[ "$mode" == "--json" ]]; then
  cat "$STATE_FILE"
  exit 0
fi

# Pretty status icons — pure ASCII so this works in any terminal.
status_glyph() {
  case "$1" in
    done)      echo "[v]" ;;
    running)   echo "[>]" ;;
    pending)   echo "[ ]" ;;
    failed)    echo "[X]" ;;
    throttled) echo "[~]" ;;
    skipped)   echo "[-]" ;;
    *)         echo "[?]" ;;
  esac
}

human_age() {
  # Convert ISO8601 to "Xm Ys ago"; fall back to raw on failure.
  local ts="$1"
  [[ -z "$ts" || "$ts" == "null" ]] && { echo ""; return; }
  local then now diff
  then=$(date -u -j -f "%Y-%m-%dT%H:%M:%SZ" "$ts" +%s 2>/dev/null) || { echo "$ts"; return; }
  now=$(date -u +%s)
  diff=$((now - then))
  if (( diff < 60 )); then printf "%ds ago" "$diff"
  elif (( diff < 3600 )); then printf "%dm%ds ago" $((diff/60)) $((diff%60))
  else printf "%dh%02dm ago" $((diff/3600)) $(((diff%3600)/60))
  fi
}

if [[ "$mode" == "--decision" ]]; then
  jq -r '"phase=\(.phase) current=\(.current) tick=\(.tick_count)"' "$STATE_FILE"
  exit 0
fi

# --- Human ledger ---

range_start=$(jq -r '.range[0]' "$STATE_FILE")
range_end=$(jq -r '.range[1]' "$STATE_FILE")
phase=$(jq -r '.phase' "$STATE_FILE")
tick=$(jq -r '.tick_count' "$STATE_FILE")
last_tick=$(jq -r '.last_tick_at // ""' "$STATE_FILE")

printf "Ralph loop  iter %s..%s   tick #%s\n" "$range_start" "$range_end" "$tick"
printf "%s\n" "──────────────────────────────────"

iter_count=$(jq '.iterations | length' "$STATE_FILE")
for ((i=0; i<iter_count; i++)); do
  n=$(jq -r ".iterations[$i].n" "$STATE_FILE")
  status=$(jq -r ".iterations[$i].status" "$STATE_FILE")
  branch=$(jq -r ".iterations[$i].branch // \"\"" "$STATE_FILE")
  merge=$(jq -r ".iterations[$i].merge_commit // \"\"" "$STATE_FILE")
  started=$(jq -r ".iterations[$i].started_at // \"\"" "$STATE_FILE")
  ended=$(jq -r ".iterations[$i].ended_at // \"\"" "$STATE_FILE")

  glyph=$(status_glyph "$status")
  detail=""
  case "$status" in
    done)
      if [[ -n "$merge" ]]; then
        detail="merged $(human_age "$ended") ($merge)"
      else
        detail="$(human_age "$ended")"
      fi
      ;;
    running)
      detail="started $(human_age "$started")"
      [[ -n "$branch" ]] && detail="$detail  branch=$branch"
      ;;
    failed)
      detail="failed $(human_age "$ended")"
      ;;
    throttled)
      detail="throttled $(human_age "$ended")"
      ;;
    skipped)
      detail="already merged"
      ;;
  esac

  printf "%s iter-%-3s  %-9s  %s\n" "$glyph" "$n" "$status" "$detail"
done

printf "\nphase: %s" "$phase"
[[ -n "$last_tick" ]] && printf "  last tick: %s" "$(human_age "$last_tick")"
printf "\n"
