#!/usr/bin/env bash
set -euo pipefail

# ac-fidelity-check.sh — Verify each ticked Acceptance Criteria checkbox in an
# iteration plan is backed by evidence in the branch diff (a test function,
# a referenced symbol, or an explicit deferral annotation).
#
# Usage:
#   ac-fidelity-check.sh --plan <path> [--branch <branch>] [--base <base>] [--range <A..B>]
#
# Exit 0 if every ticked AC has evidence (or is annotated as deferred), 1
# otherwise.

PLAN=""
BRANCH=""
BASE="main"
RANGE=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --plan)   PLAN="$2"; shift 2 ;;
    --branch) BRANCH="$2"; shift 2 ;;
    --base)   BASE="$2"; shift 2 ;;
    --range)  RANGE="$2"; shift 2 ;;
    -h|--help) sed -n '3,12p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

if [[ -z "$PLAN" ]]; then
  echo "ERROR: --plan <path> is required" >&2
  exit 2
fi
if [[ ! -f "$PLAN" ]]; then
  echo "ERROR: plan file not found: $PLAN" >&2
  exit 2
fi

# Resolve diff range.
if [[ -z "$RANGE" ]]; then
  if [[ -z "$BRANCH" ]]; then BRANCH=$(git rev-parse --abbrev-ref HEAD); fi
  if ! git rev-parse --verify --quiet "$BASE" >/dev/null; then
    if git rev-parse --verify --quiet "origin/$BASE" >/dev/null; then
      BASE="origin/$BASE"
    else
      echo "ERROR: base ref '$BASE' not found" >&2
      exit 2
    fi
  fi
  RANGE="$BASE..$BRANCH"
fi

DIFF_FILE=$(mktemp -t ac-fidelity.XXXXXX)
trap 'rm -f "$DIFF_FILE"' EXIT
# Exclude kb/ and *.md so tokens that appear in the plan itself (which is in
# the diff) don't produce false ✅ evidence for code-bearing ACs.
#
# We do NOT fall back to the full diff when this is empty — that would let
# a ticked AC be "backed" by text in its own plan (CodeRabbit caught this
# in PR #91). A docs-only branch will fail this check unless every ticked
# AC is marked `[deferred — new plan: <path>]` or removed.
git diff "$RANGE" -- \
  ':(exclude)kb/' ':(exclude)*.md' ':(exclude)CHANGELOG*' \
  > "$DIFF_FILE" 2>/dev/null || true

# Extract the ## Acceptance Criteria block. We start at the heading and stop
# at the next H2 (or EOF). Tolerate trailing "[N/M]" counters on the heading.
AC_BLOCK=$(awk '
  /^## Acceptance Criteria/ { capture=1; next }
  capture && /^## / { exit }
  capture { print }
' "$PLAN")

if [[ -z "$AC_BLOCK" ]]; then
  echo "ac-fidelity: no '## Acceptance Criteria' section in $PLAN — nothing to check."
  exit 0
fi

# For each ticked checkbox, look for evidence.
TOTAL=0
FAILED=0
FAILED_LINES=()

while IFS= read -r line; do
  # Match `- [x] <text>` (lowercase x; uppercase X also tolerated).
  if [[ ! "$line" =~ ^[[:space:]]*-[[:space:]]*\[[xX]\][[:space:]]+(.+)$ ]]; then
    continue
  fi
  text="${BASH_REMATCH[1]}"
  TOTAL=$((TOTAL + 1))

  # Deferred annotation forms (em dash or `--`):
  #   `[deferred — new plan: <path>]`        — work moved to a follow-up plan
  #   `[deferred — not applicable: <reason>]` — AC made moot by an in-iteration
  #     design choice (e.g. a different task removed the surface entirely).
  #     Reason must be substantive (≥10 chars after the marker).
  if [[ "$text" == *"[deferred"* ]]; then
    plan_ref=$(printf '%s' "$text" | grep -oE 'new plan:[[:space:]]*[^]]+' \
      | sed -E 's/new plan:[[:space:]]*//' | head -1 || true)
    if [[ -n "$plan_ref" ]]; then
      # Normalise: strip surrounding whitespace, leading "kb/" prefix variants.
      plan_ref=$(echo "$plan_ref" | sed -E 's/^[[:space:]]+|[[:space:]]+$//g')
      # Accept either repo-rooted or kb/-relative path.
      if [[ -f "$plan_ref" ]] || [[ -f "kb/$plan_ref" ]] || [[ -f "$(dirname "$PLAN")/$plan_ref" ]]; then
        continue
      fi
      echo "❌ ticked AC marked deferred but referenced plan not found: $plan_ref"
      FAILED=$((FAILED + 1))
      FAILED_LINES+=("$line")
      continue
    fi
    # "[deferred — not applicable: <reason>]" form.
    na_reason=$(printf '%s' "$text" | grep -oiE 'not[[:space:]]+applicable:[[:space:]]*[^]]+' \
      | sed -E 's/[Nn]ot[[:space:]]+[Aa]pplicable:[[:space:]]*//' | head -1 || true)
    if [[ -n "$na_reason" ]]; then
      na_reason=$(echo "$na_reason" | sed -E 's/^[[:space:]]+|[[:space:]]+$//g')
      if [[ ${#na_reason} -ge 10 ]]; then
        continue
      fi
      echo "❌ ticked AC marked [deferred — not applicable] but reason is too short (need ≥10 chars): $na_reason"
      FAILED=$((FAILED + 1))
      FAILED_LINES+=("$line")
      continue
    fi
  fi

  evidence_found=0

  # Heuristic 0: build/CI process ACs (`cargo fmt ... clean`, "CI green",
  # "all checks pass") don't leave a token in the diff. Accept them as
  # process-status ACs the implementing agent is responsible for running.
  if [[ "$text" =~ cargo[[:space:]]+(fmt|clippy|test|build|check) ]] \
     || [[ "$text" =~ (CI|ci)[[:space:]]+(passes|green|clean) ]] \
     || [[ "$text" =~ all[[:space:]]checks[[:space:]]pass ]]; then
    continue
  fi

  # Heuristic 1: test-function slug (live_* or test_*).
  for slug in $(printf '%s' "$text" | grep -oE '\b(live|test|bench)_[a-z0-9_]+' || true); do
    if grep -qE "fn[[:space:]]+${slug}\b" "$DIFF_FILE"; then
      evidence_found=1
      break
    fi
  done

  # Heuristic 2: backtick-quoted symbol(s) — strip the backticks and look in
  # the diff. Allow trailing punctuation in the captured group.
  if [[ $evidence_found -eq 0 ]]; then
    while IFS= read -r sym; do
      [[ -z "$sym" ]] && continue
      # Skip noise tokens.
      case "$sym" in iter-*|README|CLAUDE|kb/*) continue ;; esac
      if grep -qF "$sym" "$DIFF_FILE"; then
        evidence_found=1
        break
      fi
      # Try last :: component.
      last=${sym##*::}
      if [[ "$last" != "$sym" && -n "$last" ]]; then
        if grep -qE "[^A-Za-z0-9_]${last}([^A-Za-z0-9_]|$)" "$DIFF_FILE"; then
          evidence_found=1
          break
        fi
      fi
    done < <(printf '%s' "$text" | grep -oE '`[^`]+`' | sed -E 's/^`|`$//g' || true)
  fi

  # Heuristic 3: ::-qualified or SCREAMING_SNAKE token in plain text.
  if [[ $evidence_found -eq 0 ]]; then
    for sym in $(printf '%s' "$text" | grep -oE '[A-Z][A-Za-z0-9_]+(::[A-Za-z_][A-Za-z0-9_]*)+|\b[A-Z][A-Z0-9_]{4,}\b' || true); do
      if grep -qF "$sym" "$DIFF_FILE"; then
        evidence_found=1
        break
      fi
    done
  fi

  if [[ $evidence_found -eq 0 ]]; then
    echo "❌ ticked AC with no evidence in diff: ${text}"
    FAILED=$((FAILED + 1))
    FAILED_LINES+=("$line")
  fi
done <<< "$AC_BLOCK"

echo
if [[ $FAILED -eq 0 ]]; then
  echo "ac-fidelity: all $TOTAL ticked AC(s) backed by diff evidence."
  exit 0
fi

echo "ac-fidelity: $FAILED/$TOTAL ticked AC(s) lack evidence in the diff."
echo "Add a test, reference the symbol in the diff, soften the AC text, or"
echo "annotate the line with \`[deferred — new plan: <path>]\` and file the"
echo "follow-up plan before merging."
exit 1
