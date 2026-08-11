#!/usr/bin/env bash
set -euo pipefail

# preflight.sh — Discover iteration plans, check completion status, emit the
# iteration list for ralph.workflow.js.
#
# Usage: preflight.sh [--json] <start> <end>
#
# Default mode: human-readable summary on stdout.
# --json mode:  JSON object on stdout (consumed verbatim as Workflow args.preflight),
#               human-readable summary on stderr.
#
# Exit code: 0 on success, 1 if any plans are missing (output still emitted —
# the caller decides whether to abort or proceed skipping).
#
# Unlike the old ralph-loop preflight, this writes NO state.json: git
# (merge commits on origin/main) is the only durable ledger. The cache dir is
# used only for per-iteration artifacts (claims reports, progress.log).
#
# Plan discovery: `hyalo find --glob '**/<prefix>-<id>-*.md'` if hyalo is on
# PATH, else shell find. (DO NOT use `--property 'title~='` — frontmatter
# titles are "Iteration N: Slug" and never contain the substring "iteration-N".)
#
# Iteration IDs: integer with optional single lower-case letter suffix ("16",
# "16b"). IDs are opaque strings everywhere; zero-padding is preserved.

MODE="human"
if [[ "${1:-}" == "--json" ]]; then
  MODE="json"
  shift
fi

START="${1:?Usage: preflight.sh [--json] <start> <end>}"
END="${2:?Usage: preflight.sh [--json] <start> <end>}"

# Human summary goes to stdout in human mode, stderr in --json mode.
say() {
  if [[ "$MODE" == "json" ]]; then
    echo "$@" >&2
  else
    echo "$@"
  fi
}

if ! [[ "$START" =~ ^[0-9]+[a-z]?$ ]]; then
  echo "ERROR: start '$START' must match ^[0-9]+[a-z]?\$" >&2
  exit 1
fi
if ! [[ "$END" =~ ^[0-9]+[a-z]?$ ]]; then
  echo "ERROR: end '$END' must match ^[0-9]+[a-z]?\$" >&2
  exit 1
fi

# Plan-file / branch naming prefixes. Defaults preserve the historical
# iteration-NN-*.md plan names and iter-NN/<slug> branches; override per run:
#   RALPH_PLAN_PREFIX=migration RALPH_BRANCH_PREFIX=migration preflight.sh --json 01 07
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
#  - Pure integer range ("14".."17"): iterate integers, no letters allowed on either side.
#  - Same integer with letters ("16b".."16g"): iterate the letter range.
#  - "16".."16d": bare 16, then 16a, 16b, 16c, 16d.
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
CACHE_DIR="${RALPH_CACHE_DIR:-$HOME/.cache/new-ralph-loop/$SLUG}"
mkdir -p "$CACHE_DIR"

# --- Working-tree cleanliness check ---------------------------------------
# The workflow's agents operate directly in THIS working tree (branch, commit,
# switch, merge). Uncommitted changes or untracked files can be swept into an
# iteration's commits or block /merge-pr's clean-tree pre-flight. Warn loudly;
# don't fail — the user may have a deliberate reason.
DIRTY=$(git -C "$REPO_ROOT" status --porcelain 2>/dev/null || true)
if [[ -n "$DIRTY" ]]; then
  {
    echo "⚠️  WARNING: working tree is not clean"
    echo ""
    echo "Uncommitted changes / untracked files in $REPO_ROOT:"
    echo "$DIRTY" | sed 's/^/    /'
    echo ""
    echo "The run's agents branch, commit, and merge directly in this tree."
    echo "Commit or stash before launching, or these files may end up in an"
    echo "iteration's commits (or block the merge pre-flight)."
    echo ""
  } >&2
fi

# --- Discover plan path for one iteration N. Echoes path or empty. ---
discover_plan() {
  local n="$1"
  local path=""

  if command -v hyalo >/dev/null 2>&1; then
    path=$(cd "$REPO_ROOT" && hyalo find --glob "**/${PLAN_PREFIX}-${n}-*.md" \
             --jq '.results[0].file // empty' 2>/dev/null || true)
    # hyalo returns paths relative to its auto-detected knowledgebase root,
    # which may be a subdirectory of $REPO_ROOT. If the path doesn't resolve
    # under $REPO_ROOT, drop it so shell-find takes over.
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

  if [[ -n "$path" && "$path" != /* ]]; then
    path="$REPO_ROOT/$path"
  fi
  echo "$path"
}

# --- Extract frontmatter title from a plan file. Echoes title or empty. ---
extract_title() {
  local plan="$1"
  local title=""
  if command -v hyalo >/dev/null 2>&1; then
    title=$(hyalo properties "$plan" 2>/dev/null | jq -r '.title // ""' 2>/dev/null || true)
  fi
  if [[ -z "$title" ]]; then
    title=$(grep -m1 -E '^title:' "$plan" 2>/dev/null | sed 's/^title: *//' | tr -d '"' || true)
  fi
  echo "$title"
}

# --- Check completion status. Echoes one of: pending|done|skipped ---
# skipped: a merge commit on origin/main references the iteration's branch
# done:    plan frontmatter status is "completed" or "done"
# pending: otherwise
check_completion() {
  local n="$1" plan="$2"

  # Look for a merge commit whose subject names a branch like "iter-N/<slug>".
  # Slash-anchored so iter-16 doesn't match iter-16b. Restricted to --merges so
  # plan-rewrite commits ("iter-13: defer auth") don't false-positive.
  # Capture into a variable instead of piping to grep -q (SIGPIPE + pipefail).
  local log_out branch_re
  # With the default prefix, match both historical spellings (iter-N/ and
  # iteration-N/). With an explicit override, match ONLY that prefix so legacy
  # iter-N/ merges can't false-positive same-numbered waves of another series.
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

  if [[ -n "$plan" && -f "$plan" ]]; then
    local status
    if command -v hyalo >/dev/null 2>&1; then
      status=$(hyalo properties "$plan" 2>/dev/null | jq -r '.status // ""' 2>/dev/null || true)
    fi
    if [[ -z "${status:-}" ]]; then
      status=$(grep -m1 -E '^status:' "$plan" 2>/dev/null | sed 's/^status: *//' | tr -d '"' || true)
    fi
    case "${status:-}" in
      completed|done) echo "done"; return ;;
    esac
  fi

  echo "pending"
}

# --- Normalize an ID against the plan files' padding convention. -----------
# Users say "4 7"; plan files may be iteration-04-*.md (or vice versa). If the
# raw ID finds no plan, probe padded (04, 004) and stripped (4) variants; the
# first variant WITH a plan becomes the canonical ID for everything downstream
# (branch names, completion matching, artifacts, JSON). Raw ID wins if it has
# a plan, so repos with both conventions stay predictable.
normalize_id() {
  local n="$1" base letter cands=() c
  base="${n%%[a-z]*}"; letter="${n#"$base"}"
  cands=("$n")
  if [[ ${#base} -eq 1 ]]; then cands+=("0${base}${letter}" "00${base}${letter}")
  elif [[ ${#base} -eq 2 ]]; then cands+=("0${base}${letter}")
  fi
  local stripped=$((10#$base))
  [[ "${stripped}${letter}" != "$n" ]] && cands+=("${stripped}${letter}")
  for c in "${cands[@]}"; do
    if [[ -n "$(discover_plan "$c")" ]]; then
      echo "$c"
      return
    fi
  done
  echo "$n"
}

# --- Build the iterations array + clean stale in-range artifacts ---
ITER_JSON="[]"
MISSING=()

for n in "${ITERS[@]}"; do
  n=$(normalize_id "$n")
  # Stale artifacts from a previous run of the same IDs would confuse the
  # review agent (it reads iter-N-claims.md). The glob requires a dash right
  # after the ID, so iter-16-* cannot match iter-16b-*.
  rm -f "$CACHE_DIR/iter-${n}-"* 2>/dev/null || true

  plan=$(discover_plan "$n")
  if [[ -z "$plan" ]]; then
    MISSING+=("$n")
    iter_obj=$(jq -n --arg n "$n" \
               '{n: $n, status: "pending", plan_path: null, title: null, missing: true}')
  else
    completion=$(check_completion "$n" "$plan")
    title=$(extract_title "$plan")
    iter_obj=$(jq -n --arg n "$n" --arg p "$plan" --arg s "$completion" --arg t "$title" \
               '{n: $n, status: $s, plan_path: $p, title: (if $t == "" then null else $t end)}')
  fi
  ITER_JSON=$(jq --argjson o "$iter_obj" '. + [$o]' <<<"$ITER_JSON")
done

# Fresh progress log per preflight (used only when SendMessage fallback is active).
: > "$CACHE_DIR/progress.log"

# --- Human summary ---
say "New-ralph-loop preflight — iter $START..$END"
say "──────────────────────────────────────────"
say "Repo:   $REPO_ROOT"
say "Cache:  $CACHE_DIR"
say ""
while IFS= read -r line; do say "$line"; done < <(
  jq -r --arg bp "$BRANCH_PREFIX" \
    '.[] | "  \($bp)-\(.n)  \(.status)\(if .plan_path then "  " + .plan_path else "  (no plan found)" end)\(if .title then "  — " + .title else "" end)"' \
    <<<"$ITER_JSON")
say ""

if [[ ${#MISSING[@]} -gt 0 ]]; then
  say "WARN: missing plan files for iterations: ${MISSING[*]}"
fi

SKIPPED=$(jq -r '[.[] | select(.status=="skipped" or .status=="done")] | length' <<<"$ITER_JSON")
PENDING=$(jq -r '[.[] | select(.status=="pending")] | length' <<<"$ITER_JSON")
say "Summary: $PENDING pending, $SKIPPED already complete"

# --- JSON output (stdout, --json mode only) ---
if [[ "$MODE" == "json" ]]; then
  jq -n \
    --arg root "$REPO_ROOT" \
    --arg cache "$CACHE_DIR" \
    --arg plan_prefix "$PLAN_PREFIX" \
    --arg branch_prefix "$BRANCH_PREFIX" \
    --arg start "$START" \
    --arg end "$END" \
    --argjson iters "$ITER_JSON" \
    '{
      repo_root: $root,
      cache_dir: $cache,
      plan_prefix: $plan_prefix,
      branch_prefix: $branch_prefix,
      range: [$start, $end],
      iterations: $iters
    }'
fi

# Exit 1 if any missing — caller decides whether to proceed skipping them.
[[ ${#MISSING[@]} -eq 0 ]] || exit 1
exit 0
