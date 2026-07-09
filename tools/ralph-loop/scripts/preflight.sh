#!/usr/bin/env bash
set -euo pipefail

# preflight.sh — Discover iteration plans, check completion status, seed state.json.
#
# Usage: preflight.sh <start> <end>
#
# Emits human-readable summary on stdout. Writes seeded state.json to
# $RALPH_CACHE_DIR (or ~/.cache/ralph-loop/<repo-slug>/).
#
# Exit code: 0 on success (state seeded), 1 on missing plans / errors.
#
# Plan discovery: uses `hyalo find --glob '**/iteration-N-*.md'` if hyalo
# is on PATH, otherwise falls back to shell glob across the repo.
# (DO NOT use `--property 'title~='` — iteration files' frontmatter title is
# typically "Iteration N: Slug" which the substring "iteration-N" never matches.)
#
# Iteration IDs are an integer with an optional single lower-case letter
# suffix: "16", "16b". Letter-suffixed iterations are sub-iterations slotted
# between bare integer ones (iter-15b/15c convention). The state.json stores
# `n`, `range[0]`, `range[1]`, and `current` as strings throughout so that
# letter IDs round-trip cleanly. Pure-integer ranges still work unchanged.

START="${1:?Usage: preflight.sh <start> <end>}"
END="${2:?Usage: preflight.sh <start> <end>}"

if ! [[ "$START" =~ ^[0-9]+[a-z]?$ ]]; then
  echo "ERROR: start '$START' must match ^[0-9]+[a-z]?\$" >&2
  exit 1
fi
if ! [[ "$END" =~ ^[0-9]+[a-z]?$ ]]; then
  echo "ERROR: end '$END' must match ^[0-9]+[a-z]?\$" >&2
  exit 1
fi

# Plan-file / branch naming prefixes. Defaults preserve the historical
# iteration-NN-*.md plan names and iter-NN/<slug> branches; override per run
# for repos whose waves use a different prefix, e.g.
#   RALPH_PLAN_PREFIX=migration RALPH_BRANCH_PREFIX=migration preflight.sh 01 07
# for migration-01-enablers.md + migration-01/<slug> branches.
PLAN_PREFIX="${RALPH_PLAN_PREFIX:-iteration}"
BRANCH_PREFIX="${RALPH_BRANCH_PREFIX:-iter}"

# Split each ID into integer base and optional letter suffix.
START_NUM="${START%%[a-z]*}"; START_LETTER="${START#"$START_NUM"}"
END_NUM="${END%%[a-z]*}";     END_LETTER="${END#"$END_NUM"}"

if (( 10#$START_NUM > 10#$END_NUM )); then
  echo "ERROR: start ($START) must be <= end ($END)" >&2
  exit 1
fi
if [[ "$START_NUM" == "$END_NUM" && -n "$START_LETTER" && -n "$END_LETTER" \
      && "$START_LETTER" > "$END_LETTER" ]]; then
  echo "ERROR: start letter '$START_LETTER' > end letter '$END_LETTER'" >&2
  exit 1
fi

# Build the iteration list as an array of string IDs.
#  - Pure integer range (e.g. "14" .. "17"): iterate integers, no letters allowed on either side.
#  - Same integer with letters (e.g. "16b" .. "16g"): iterate the letter range.
#  - "16" .. "16d": bare 16, then 16a, 16b, 16c, 16d.
ITERS=()
if [[ "$START_NUM" == "$END_NUM" ]]; then
  if [[ -z "$START_LETTER" ]]; then
    ITERS+=("$START_NUM")
  fi
  if [[ -n "$END_LETTER" ]]; then
    for letter in {a..z}; do
      [[ -n "$START_LETTER" && "$letter" < "$START_LETTER" ]] && continue
      ITERS+=("${START_NUM}${letter}")
      [[ "$letter" == "$END_LETTER" ]] && break
    done
  elif [[ -n "$START_LETTER" ]]; then
    # End has no letter but start does — interpret as "just the start letter".
    ITERS+=("${START_NUM}${START_LETTER}")
  fi
else
  if [[ -n "$START_LETTER" || -n "$END_LETTER" ]]; then
    echo "ERROR: cross-integer letter ranges not supported (start=$START, end=$END)" >&2
    exit 1
  fi
  # 10# forces base-10 so "08"/"09" don't trip octal parsing; zero-padded
  # ranges (e.g. "01".."07") keep their width.
  PAD_WIDTH=0
  if [[ "$START_NUM" == 0* && ${#START_NUM} -gt 1 ]]; then PAD_WIDTH=${#START_NUM}; fi
  for ((n=10#$START_NUM; n<=10#$END_NUM; n++)); do
    if (( PAD_WIDTH > 0 )); then
      printf -v _id "%0${PAD_WIDTH}d" "$n"
    else
      _id="$n"
    fi
    ITERS+=("$_id")
  done
fi

REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null) || {
  echo "ERROR: not in a git repo" >&2
  exit 1
}
SLUG=$(basename "$REPO_ROOT")
CACHE_DIR="${RALPH_CACHE_DIR:-$HOME/.cache/ralph-loop/$SLUG}"
mkdir -p "$CACHE_DIR"
STATE_FILE="$CACHE_DIR/state.json"

# --- Working-tree cleanliness check ---------------------------------------
# The cmux child for each iteration does its own git work in a separate
# workspace clone. If the orchestrator's working tree has uncommitted
# modifications or untracked files, the child may stash them ("iter-N
# leftover plans" pattern) and never pop the stash, which breaks downstream
# iterations that need the stashed files (e.g. plan files for iter-N+1).
# Warn loudly so the user commits or stashes before launching.
#
# Don't fail — the user may have a deliberate reason. Just make it visible.
DIRTY=$(git -C "$REPO_ROOT" status --porcelain 2>/dev/null || true)
if [[ -n "$DIRTY" ]]; then
  echo "⚠️  WARNING: working tree is not clean" >&2
  echo "" >&2
  echo "Uncommitted changes / untracked files in $REPO_ROOT:" >&2
  echo "$DIRTY" | sed 's/^/    /' >&2
  echo "" >&2
  echo "Each iteration's cmux child runs git operations in its own workspace clone." >&2
  echo "If the child stashes these to make space for its branch work, the stash" >&2
  echo "may not be popped — breaking downstream iterations that read these files" >&2
  echo "(e.g. plan files for later iterations in the same ralph-loop run)." >&2
  echo "" >&2
  echo "Recommended before proceeding:" >&2
  echo "  - Commit anything you want available to the children (especially plan files)" >&2
  echo "  - Stash + drop anything you don't need" >&2
  echo "  - Then re-run preflight" >&2
  echo "" >&2
fi

# --- Discover plan path for one iteration N. Echoes path or empty. ---
discover_plan() {
  local n="$1"
  local path=""

  if command -v hyalo >/dev/null 2>&1; then
    # Use --jq to extract the file field cleanly. --format text is multi-line
    # per file (path on first line in quotes, then indented properties) — not
    # what we want. --property 'title~=' won't work either: frontmatter title
    # is typically "Iteration N: Slug" which doesn't contain "iteration-N".
    path=$(cd "$REPO_ROOT" && hyalo find --glob "**/${PLAN_PREFIX}-${n}-*.md" \
             --jq '.results[0].file // empty' 2>/dev/null || true)
    # hyalo returns paths relative to its auto-detected knowledgebase root,
    # which may be a subdirectory of $REPO_ROOT (e.g. <repo>/foo-knowledgebase/).
    # If the path doesn't resolve under $REPO_ROOT, drop it so shell-find takes
    # over — shell-find always returns paths relative to $REPO_ROOT.
    if [[ -n "$path" && ! -f "$REPO_ROOT/$path" ]]; then
      path=""
    fi
  fi

  if [[ -z "$path" ]]; then
    # Fallback: shell find. Take first match without piping to head (SIGPIPE
    # interacts badly with set -o pipefail).
    path=$(cd "$REPO_ROOT" && find . -type f -name "${PLAN_PREFIX}-${n}-*.md" 2>/dev/null \
             | sed -n '1p' | sed 's|^\./||' || true)
  fi

  if [[ -n "$path" ]]; then
    # Normalize to absolute path
    if [[ "$path" != /* ]]; then
      path="$REPO_ROOT/$path"
    fi
  fi
  echo "$path"
}

# --- Check completion status. Echoes one of: pending|done|skipped ---
# done: frontmatter status is "completed" or "done"
# skipped: a merge commit on origin/main references the iteration's branch
# pending: otherwise
check_completion() {
  local n="$1" plan="$2"

  # Look for a merge commit whose subject names a branch like "iter-N/<slug>"
  # or "iteration-N/<slug>" (the trailing slash is the branch-name marker).
  # Restricted to --merges so plain commits with similar wording — e.g. plan
  # rewrites like "iter-13: defer auth" or "Rewrite iter-13 plan" — don't
  # false-positive as completed iterations.
  #
  # Capture log output into a variable instead of piping to grep -q: grep
  # closes the pipe early on match, git gets SIGPIPE, and `set -o pipefail`
  # then flags the whole pipeline as failed — which would make the `if` see
  # "no match" even when there was one.
  local log_out branch_re
  # With the default prefix, match both historical spellings (iter-N/ and
  # iteration-N/). With an explicit override, match ONLY that prefix —
  # otherwise legacy iter-N/ merges false-positive same-numbered waves of a
  # different series (e.g. iter-01/ marking migration-01 as complete).
  if [[ "$BRANCH_PREFIX" == "iter" ]]; then
    branch_re="(iter|iteration)-${n}/"
  else
    branch_re="${BRANCH_PREFIX}-${n}/"
  fi
  log_out=$(git -C "$REPO_ROOT" log origin/main --merges --oneline 2>/dev/null || true)
  if printf '%s\n' "$log_out" | grep -qiE "$branch_re"; then
    echo "skipped"
    return
  fi

  # Check frontmatter status
  if [[ -n "$plan" && -f "$plan" ]]; then
    local status
    if command -v hyalo >/dev/null 2>&1; then
      status=$(hyalo properties "$plan" 2>/dev/null | jq -r '.status // ""' 2>/dev/null || true)
    fi
    if [[ -z "${status:-}" ]]; then
      # Crude grep fallback for "status: completed"
      status=$(grep -m1 -E '^status:' "$plan" 2>/dev/null | sed 's/^status: *//' | tr -d '"' || true)
    fi
    case "${status:-}" in
      completed|done) echo "done"; return ;;
    esac
  fi

  echo "pending"
}

# --- Build the iterations array as JSON. `n` is stored as a string so letter
# suffixes round-trip; ralph-stat.sh and run-iteration.sh treat it as a string
# already. ---
ITER_JSON="[]"
MISSING=()

for n in "${ITERS[@]}"; do
  # Extract the numeric prefix for use as a sort key (avoids string "10" < "9" bugs).
  n_int=$((10#${n%%[a-z]*}))
  plan=$(discover_plan "$n")
  if [[ -z "$plan" ]]; then
    MISSING+=("$n")
    iter_obj=$(jq -n --arg n "$n" --argjson n_int "$n_int" \
               '{n: $n, n_int: $n_int, status: "pending", plan_path: null, missing: true}')
  else
    completion=$(check_completion "$n" "$plan")
    iter_obj=$(jq -n --arg n "$n" --argjson n_int "$n_int" --arg p "$plan" --arg s "$completion" \
               '{n: $n, n_int: $n_int, status: $s, plan_path: $p}')
  fi
  ITER_JSON=$(jq --argjson o "$iter_obj" '. + [$o]' <<<"$ITER_JSON")
done

# --- Decide initial "current" — first pending iter's `n` (string). If all
# already done, leave empty; the orchestrator's tick logic detects that as
# "nothing to do". ---
CURRENT=$(jq -r \
  '[.[] | select(.status=="pending")] | if length>0 then .[0].n else "" end' \
  <<<"$ITER_JSON")

NOW=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

# --- Capture the cmux workspace of the orchestrator session, so the child
# script can target the correct workspace regardless of how it's launched
# (nohup/disown sever the calling-process context cmux uses to infer
# "current workspace"). Best-effort — empty if cmux unavailable.
#
# Prefer $CMUX_WORKSPACE_ID — cmux exports this into every pane's env so
# each pane can identify its OWN workspace. `cmux current-workspace` is a
# global "most-recently-focused" lookup, which can return another workspace
# entirely when the user has multiple cmux windows open (e.g. a parallel
# ralph-loop in another repo's workspace). Using the env var keeps the
# orchestrator pinned to its own workspace. ---
CMUX_WORKSPACE=""
if [[ -n "${CMUX_WORKSPACE_ID:-}" ]]; then
  # cmux accepts UUIDs as workspace refs — pass through verbatim.
  CMUX_WORKSPACE="$CMUX_WORKSPACE_ID"
elif command -v cmux >/dev/null 2>&1; then
  CMUX_WORKSPACE=$(cmux current-workspace 2>/dev/null \
    | grep -oE 'workspace:[0-9a-f-]+' | head -1 || true)
fi

# --- Seed state.json (overwrites any prior state). `range[0]`, `range[1]`,
# and `current` are stored as strings so letter-suffixed IDs round-trip.
# Each iteration entry also carries `n_int` (integer) for safe numeric
# comparison by the orchestrator — avoids the "10" < "9" string-sort bug. ---
jq -n \
  --arg root "$REPO_ROOT" \
  --arg now "$NOW" \
  --arg ws "$CMUX_WORKSPACE" \
  --arg start "$START" \
  --arg end "$END" \
  --arg current "$CURRENT" \
  --arg plan_prefix "$PLAN_PREFIX" \
  --arg branch_prefix "$BRANCH_PREFIX" \
  --argjson iters "$ITER_JSON" \
  '{
    version: 1,
    plan_prefix: $plan_prefix,
    branch_prefix: $branch_prefix,
    repo_root: $root,
    started_at: $now,
    range: [$start, $end],
    current: $current,
    phase: "pending",
    last_tick_at: null,
    tick_count: 0,
    cmux_workspace: (if $ws == "" then null else $ws end),
    iterations: $iters
  }' > "$STATE_FILE"

# --- Human summary ---
echo "Ralph loop preflight — iter $START..$END"
echo "──────────────────────────────────────────"
echo "Repo:      $REPO_ROOT"
echo "Cache:     $CACHE_DIR"
if [[ -n "$CMUX_WORKSPACE" ]]; then
  echo "Workspace: $CMUX_WORKSPACE (cmux)"
fi
echo
jq -r --arg bp "$BRANCH_PREFIX" '.iterations[] | "  \($bp)-\(.n)  \(.status)\(if .plan_path then "  " + .plan_path else "  (no plan found)" end)"' "$STATE_FILE"
echo

if [[ ${#MISSING[@]} -gt 0 ]]; then
  echo "WARN: missing plan files for iterations: ${MISSING[*]}" >&2
fi

SKIPPED=$(jq -r '[.iterations[] | select(.status=="skipped" or .status=="done")] | length' "$STATE_FILE")
PENDING=$(jq -r '[.iterations[] | select(.status=="pending")] | length' "$STATE_FILE")
echo "Summary: $PENDING pending, $SKIPPED already complete"
echo "Next:    ${BRANCH_PREFIX}-$CURRENT"

# Defensive sweep: clear any stale `iter-*` sidebar entries / progress bar
# left behind by a previous crashed orchestrator run.
_clear_args=()
[[ -n "$CMUX_WORKSPACE" ]] && _clear_args+=(--workspace "$CMUX_WORKSPACE")
"$(dirname "$0")/clear-cmux-state.sh" "${_clear_args[@]}" 2>/dev/null || true

# Exit 1 if any missing — caller decides whether to proceed
[[ ${#MISSING[@]} -eq 0 ]] || exit 1
exit 0
