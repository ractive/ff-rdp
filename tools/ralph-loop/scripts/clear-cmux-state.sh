#!/usr/bin/env bash
set -euo pipefail

# clear-cmux-state.sh — Sweep ralph-loop's visual state from the cmux sidebar.
#
# Removes every `iter-*` status entry, the progress bar, and (optionally) the
# log feed for the target workspace. Safe to run any time: each cmux call
# soft-fails. Built-in cmux badges ("Needs input", "Claude is waiting…") are
# owned by cmux itself and cannot be cleared from here.
#
# Usage: clear-cmux-state.sh [--workspace <ref>] [--include-log]
#
# Workspace resolution order: --workspace flag, then $RALPH_CMUX_WORKSPACE,
# then cmux's own default ($CMUX_WORKSPACE_ID / current).

WORKSPACE="${RALPH_CMUX_WORKSPACE:-}"
INCLUDE_LOG=0

while (( $# )); do
  case "$1" in
    --workspace)   WORKSPACE="${2:?--workspace needs a value}"; shift 2 ;;
    --include-log) INCLUDE_LOG=1; shift ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

if [[ -n "$WORKSPACE" ]]; then
  WS_FLAG=(--workspace "$WORKSPACE")
else
  WS_FLAG=()
fi

# Clear every iter-* status key the orchestrator may have set.
while IFS= read -r key; do
  [[ -z "$key" ]] && continue
  cmux clear-status "$key" ${WS_FLAG[@]+"${WS_FLAG[@]}"} 2>/dev/null || true
done < <(cmux list-status ${WS_FLAG[@]+"${WS_FLAG[@]}"} 2>/dev/null \
           | awk -F= '/^iter-/ {print $1}')

cmux clear-progress ${WS_FLAG[@]+"${WS_FLAG[@]}"} 2>/dev/null || true

if (( INCLUDE_LOG )); then
  cmux clear-log ${WS_FLAG[@]+"${WS_FLAG[@]}"} 2>/dev/null || true
fi
