#!/usr/bin/env bash
set -euo pipefail

# ac-fidelity-check.sh — Check that each ticked Acceptance Criteria checkbox in
# an iteration plan *references* evidence that resolves in the branch diff: a
# test function, a referenced symbol, or an explicit deferral annotation.
#
# SCOPE — read this before trusting a green result. This script only ever sees
# a plan file and a diff. It CANNOT and DOES NOT verify that any test was
# executed, or that it passed. A ticked AC that names a real test function
# proves the function exists, not that anyone ran it (iter-154). Two guards
# narrow that gap without closing it: a ticked AC whose own text admits
# non-execution fails, and a ticked AC naming a `live_*` test (never run in CI —
# they are `#[ignore]`-gated) must carry a `[verified: <YYYY-MM-DD>, <measured
# result>]` annotation that a human had to paste.
#
# Usage:
#   ac-fidelity-check.sh --plan <path> [--branch <branch>] [--base <base>] [--range <A..B>]
#
# Exit 0 if every ticked AC references resolvable evidence (or is annotated as
# deferred), 1 otherwise.

PLAN=""
BRANCH=""
BASE="main"
RANGE=""
SKIP_TEST_EXISTENCE="${AC_FIDELITY_SKIP_TEST_EXISTENCE:-0}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --plan)   PLAN="$2"; shift 2 ;;
    --branch) BRANCH="$2"; shift 2 ;;
    --base)   BASE="$2"; shift 2 ;;
    --range)  RANGE="$2"; shift 2 ;;
    --skip-test-existence) SKIP_TEST_EXISTENCE=1; shift ;;
    -h|--help) sed -n '4,21p' "$0"; exit 0 ;;
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

# iter-154: ACs routinely wrap over several indented continuation lines, and the
# per-line loop below never saw them. That is how iteration-151's two chunk ACs
# passed while their own continuation text read "not exercised end-to-end in
# this session's time budget" (verified by replaying 6d07c8c).
#
# Fold each checkbox into ONE record: `<first line>\x1f<whole AC, whitespace
# collapsed>`. The evidence heuristics keep reading only the FIRST line —
# widening their input would hand them more tokens to match and could turn a
# should-fail plan green, which is exactly what the pinned 61v=FAIL replay
# baseline exists to prevent. Three consumers read the folded text: the two new
# negative checks (which can only ever add failures) and the `[deferred — …]`
# accept, which can only ever remove them — see the anchoring note there.
#
# Continuation rules, which follow Markdown's own list-item semantics:
#   * an indented non-blank line continues the AC, even across a blank line
#     (a second paragraph of the same list item, PR #193 finding 4);
#   * an unindented non-blank line continues it only as a LAZY continuation,
#     i.e. when no blank line intervened (PR #193 finding 5a) — after a blank
#     line, column-0 prose is a new block and belongs to no AC;
#   * a nested `- [ ]` sub-checkbox continues its parent AND opens its own
#     record, so a confession parked under an unticked sub-bullet is seen by
#     the parent instead of vanishing (PR #193 finding 5b).
#
# Pass 1 emits top-level records (nested checkboxes fold into the parent); pass
# 2 emits the nested ones. Two passes rather than one nested state machine —
# `fold_records` is identical in both, differing only in what starts a record.
fold_records() {
  # $1: awk regex matching a line that STARTS a record. Passed through the
  # environment, NOT `-v`: awk applies escape processing to a `-v` value, which
  # silently rewrites `\[` to `[` and turns the pattern into a bracket
  # expression that matches nothing.
  AC_START_RE="$1" awk '
    BEGIN { SEP = sprintf("%c", 31); start_re = ENVIRON["AC_START_RE"] }
    function flush() { if (first != "") print first SEP full; first = ""; full = "" }
    # A top-level checkbox always terminates the record in progress, in both
    # passes: it is never a continuation of anything.
    /^-[[:space:]]*\[[ xX]\]/ && $0 !~ start_re { flush(); pending_blank = 0; next }
    $0 ~ start_re { flush(); first = $0; full = $0; pending_blank = 0; next }
    {
      if (first == "") next
      if ($0 ~ /^[[:space:]]*$/) { pending_blank = 1; next }
      if ($0 !~ /^[[:space:]]/ && pending_blank) { flush(); pending_blank = 0; next }
      pending_blank = 0
      line = $0; sub(/^[[:space:]]+/, "", line); full = full " " line
    }
    END { flush() }
  '
}

# Strip CR so CRLF plans fold (and match) identically, and drop any literal
# 0x1f from the source text so it cannot truncate a record at the separator.
AC_CLEAN=$(printf '%s\n' "$AC_BLOCK" | tr -d '\r\037')
AC_FOLDED=$(
  printf '%s\n' "$AC_CLEAN" | fold_records '^-[[:space:]]*\[[ xX]\]'
  printf '%s\n' "$AC_CLEAN" | fold_records '^[[:space:]]+-[[:space:]]*\[[ xX]\]'
)

# Wording that makes a ticked AC self-incriminating: the AC text itself says the
# work was not carried out. Matched case-insensitively against the folded text
# as ERE, with word boundaries. Deliberately short and literal — no sentiment
# analysis.
#
# PR #193 finding 2: these were plain substrings, which fired on ordinary AC
# wording — `not run` inside "not run*ning*" and "can*not run*", and `time
# budget` on any latency AC ("completes within the 200 ms time budget"). Word
# boundaries fix the first two. `time budget` is dropped outright: it earns
# nothing, since the iteration-151 AC that motivated the list is caught by `not
# exercised` in the same sentence (pinned by iter151-prefix-ac.md).
#
# Word boundaries cannot fix the residual class where an AC legitimately
# *describes* product behaviour — "`--dry-run` does not run the command". That
# is what ALLOW_WORDING_RE exists for; rewording until the grep stops firing is
# the one remedy this gate must not encourage.
NON_EXECUTION_REGEXES=(
  '\bnot exercised\b'
  '\bnot run\b'
  '\bnever run\b'
  '\bnot executed\b'
  '\bimplemented and compiled\b'
  '\bnot verified\b'
)

# Escape hatch for an AC whose wording describes behaviour rather than its own
# status. Mirrors the repo's `// allow-spec-drift:` / `// allow-todo:` pattern;
# the reason must be substantive (≥10 chars) so it cannot be a bare marker.
ALLOW_WORDING_RE='\[allow-ac-wording:[[:space:]]*[^]]{10,}\]'

# For each ticked checkbox, look for evidence.
TOTAL=0
FAILED=0
FAILED_LINES=()

while IFS= read -r record; do
  line="${record%%$'\x1f'*}"
  folded="${record#*$'\x1f'}"
  # Match `- [x] <text>` (lowercase x; uppercase X also tolerated).
  if [[ ! "$line" =~ ^[[:space:]]*-[[:space:]]*\[[xX]\][[:space:]]+(.+)$ ]]; then
    continue
  fi
  text="${BASH_REMATCH[1]}"
  # Whole-AC text, whitespace collapsed so a phrase split across a wrapped line
  # still matches.
  full_text=$(printf '%s' "$folded" | tr -s '[:space:]' ' ')
  TOTAL=$((TOTAL + 1))

  # Deferred annotation forms (em dash or `--`):
  #   `[deferred — new plan: <path>]`        — work moved to a follow-up plan
  #   `[deferred — not applicable: <reason>]` — AC made moot by an in-iteration
  #     design choice (e.g. a different task removed the surface entirely).
  #     Reason must be substantive (≥10 chars after the marker).
  #
  # iter-154: matched against the folded text, not just the first line — a
  # deferral annotation is usually the last thing on a wrapped AC. This also
  # keeps a legitimate deferral out of the non-execution check below, which
  # would otherwise fire on the very wording a deferral is meant to carry.
  #
  # PR #193 finding 1: the annotation must CLOSE the AC, not merely appear
  # somewhere in it. This accept `continue`s past every later check, so an
  # unanchored substring match let any AC that so much as mentioned `[deferred`
  # in passing launder itself — a plan that failed before iter-154 passed after
  # it, the exact regression the folded-text note above warns about.
  #
  # The annotation must be the LAST thing on the AC — trailing whitespace and a
  # period are tolerated, a closing `)` is NOT. A deferral nested inside a
  # parenthetical is indistinguishable from one that merely mentions a deferral
  # in passing (both read `… [deferred — new plan: x])`), and only one of those
  # may be allowed to skip every remaining check. `iteration-114` line 124 is the
  # sole plan in the repo that closes a parenthetical this way; merged plans are
  # never re-gated, so tightening costs nothing today and the failure message
  # tells a future author exactly where to move the annotation.
  #
  # The mention must also not be inside backticks, so prose *about* deferrals —
  # including this repo's own docs and fixtures — cannot serve as one.
  deferred_anchored=0
  if printf '%s' "$full_text" \
     | grep -qE '\[deferred[^]`]*\][[:space:]]*\.?[[:space:]]*$'; then
    deferred_anchored=1
  fi
  if [[ $deferred_anchored -eq 1 ]]; then
    plan_ref=$(printf '%s' "$full_text" | grep -oE 'new plan:[[:space:]]*[^]]+' \
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
    na_reason=$(printf '%s' "$full_text" | grep -oiE 'not[[:space:]]+applicable:[[:space:]]*[^]]+' \
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

  # iter-154 Theme A: a ticked AC that says, in its own text, that the work was
  # not carried out. PR #188 shipped two such ACs ("implemented and compiled …
  # not exercised end-to-end in this session's time budget") and this gate
  # passed them, because the slugs they named resolved in the diff. Evidence
  # heuristics cannot outvote a confession — check this before any of them.
  #
  # PR #193 finding 11: report the matched wording in its folded context rather
  # than the truncated first line, so the author can see what tripped it.
  matched_phrase=""
  if ! printf '%s' "$full_text" | grep -qiE "$ALLOW_WORDING_RE"; then
    for re in "${NON_EXECUTION_REGEXES[@]}"; do
      hit=$(printf '%s' "$full_text" | grep -oiE "$re" | head -1 || true)
      if [[ -n "$hit" ]]; then
        matched_phrase="$hit"
        break
      fi
    done
  fi
  if [[ -n "$matched_phrase" ]]; then
    echo "❌ ticked AC declares its own non-execution (\"$matched_phrase\"): ${full_text:0:200}"
    echo "   If the work really did not happen: untick the AC, or annotate it"
    echo "   \`[deferred — new plan: <path>]\` (the annotation must close the AC) and file the plan."
    echo "   If the AC merely *describes* behaviour with those words, this check matched a literal"
    echo "   phrase and not your meaning: annotate \`[allow-ac-wording: <reason ≥10 chars>]\`."
    echo "   Do not reword the AC to get past this check."
    FAILED=$((FAILED + 1))
    FAILED_LINES+=("$line")
    continue
  fi

  # iter-154 Theme B: a ticked AC naming a `live_*` test must carry positive run
  # evidence. Live tests are `#[ignore]`-gated and never run in CI, so nothing
  # downstream of this gate will ever execute them — "the function exists in the
  # diff" is the only signal the pipeline would otherwise have. Required form:
  #
  #   [verified: <YYYY-MM-DD>, <measured result>]
  #
  # The date and the trailing measurement are both required; the script does not
  # (and cannot) validate the number. The point is that a human or agent had to
  # paste a real result rather than merely name a function.
  #
  # PR #193 finding 13: the date must be a real calendar-shaped date and must
  # not be in the future, so `[verified: 9999-99-99, 0]` no longer satisfies it.
  # This does not make the annotation unforgeable — DEC-030 accepts that — it
  # just stops the laziest forgery.
  live_slugs=$(printf '%s' "$full_text" | grep -oE '\blive_[a-z0-9_]+' || true)
  needs_run_evidence=0
  for slug in $live_slugs; do
    # Same filename-stem exclusion as Heuristic 1: `tests/live_oneway.rs` names
    # a file, not a test function.
    if printf '%s' "$full_text" | grep -qE "${slug}\.(rs|md|toml|json|bench|txt)"; then
      continue
    fi
    needs_run_evidence=1
    break
  done
  verified_date=""
  if [[ $needs_run_evidence -eq 1 ]]; then
    verified_date=$(printf '%s' "$full_text" \
      | grep -oE '\[verified:[[:space:]]*[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])[^]]*[0-9][^]]*\]' \
      | grep -oE '[0-9]{4}-[0-9]{2}-[0-9]{2}' | head -1 || true)
    # A future date cannot be a record of a run that happened. String compare is
    # correct for zero-padded ISO dates and needs no date-math portability.
    if [[ -n "$verified_date" ]] && [[ "$verified_date" > "$(date +%F)" ]]; then
      echo "❌ ticked AC carries a [verified: …] date in the future ($verified_date): ${text}"
      echo "   Record the date the run actually happened."
      FAILED=$((FAILED + 1))
      FAILED_LINES+=("$line")
      continue
    fi
  fi
  if [[ $needs_run_evidence -eq 1 ]] && [[ -z "$verified_date" ]]; then
    echo "❌ ticked AC names a live_* test but carries no run evidence: ${full_text:0:200}"
    echo "   Live tests are #[ignore]-gated and never run in CI, so nothing downstream of this"
    echo "   check will execute them. Run it, then add \`[verified: <YYYY-MM-DD>, <measured result>]\`"
    echo "   (e.g. \`[verified: 2026-08-12, 109 passed / 0 failed, 0 orphans]\`), untick it,"
    echo "   or annotate \`[deferred — new plan: <path>]\` at the end of the AC."
    FAILED=$((FAILED + 1))
    FAILED_LINES+=("$line")
    continue
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
  #
  # iter-66 strengthening: when an AC names a test slug, the slug MUST resolve
  # to an `fn <slug>` somewhere in the workspace tree — either in the branch
  # diff (newly added/modified) OR pre-existing under crates/. Naming a test
  # that doesn't exist anywhere is the iter-61w failure mode this iteration
  # pays down.  Use --skip-test-existence (or AC_FIDELITY_SKIP_TEST_EXISTENCE=1)
  # to opt out in environments where the source tree isn't available.
  slugs=$(printf '%s' "$text" | grep -oE '\b(live|test|bench)_[a-z0-9_]+' || true)
  if [[ -n "$slugs" ]]; then
    any_resolved=0
    missing_slugs=()
    for slug in $slugs; do
      # Filter out filename stems: if the slug appears in the AC text immediately
      # followed by a file extension (.rs, .md, .toml, etc.), it is a path
      # component (e.g. `tests/live_oneway.rs`) rather than a test function name.
      # iter-74 fix: the regex \b(live|test|bench)_[a-z0-9_]+ also matches the
      # stem of filenames like `live_oneway.rs`, causing false "no fn found" errors.
      if printf '%s' "$text" | grep -qE "${slug}\.(rs|md|toml|json|bench|txt)"; then
        continue
      fi

      # Match added-or-context `fn <slug>` in the diff. Exclude removed lines
      # (those starting with `-` but not `---`) so a deleted test cannot serve
      # as evidence for an AC that names it.
      if grep -E '^[+ ]' "$DIFF_FILE" | grep -qE "fn[[:space:]]+${slug}\b"; then
        any_resolved=1
        continue
      fi
      if [[ "$SKIP_TEST_EXISTENCE" != "1" ]] \
         && [[ -d crates ]] \
         && grep -rqE "fn[[:space:]]+${slug}\b" crates 2>/dev/null; then
        # Pre-existing test in the workspace satisfies this slug.
        any_resolved=1
        continue
      fi
      missing_slugs+=("$slug")
    done
    # iter-66 tightening: EVERY named slug must resolve. A single missing slug
    # fails the AC even if a sibling slug was found — naming a non-existent
    # test is the iter-61w failure mode.
    if [[ ${#missing_slugs[@]} -gt 0 ]] && [[ "$SKIP_TEST_EXISTENCE" != "1" ]]; then
      echo "❌ ticked AC names test(s) [${missing_slugs[*]}] with no matching \`fn\` in the workspace: ${text}"
      FAILED=$((FAILED + 1))
      FAILED_LINES+=("$line")
      continue
    fi
    if [[ $any_resolved -eq 1 ]]; then
      evidence_found=1
    fi
  fi

  # Heuristic 2: backtick-quoted symbol(s) — strip the backticks and look in
  # the diff. Allow trailing punctuation in the captured group.
  if [[ $evidence_found -eq 0 ]]; then
    while IFS= read -r sym; do
      [[ -z "$sym" ]] && continue
      # Skip noise tokens.
      case "$sym" in iter-*|README|CLAUDE|kb/*) continue ;; esac
      # iter-140: `grep -qF -- "$sym"` — without `--`, a backtick-quoted AC
      # symbol that happens to start with `-` (e.g. a CLI flag like
      # `--jq '.results.frame_url'`) is parsed as a grep OPTION, not a
      # pattern, and errors out — silently counted as "no evidence found"
      # rather than crashing the script (this runs inside an `if`, so
      # `set -e` doesn't catch it). `--` ends option parsing so the symbol
      # is always treated as a literal pattern.
      if grep -qF -- "$sym" "$DIFF_FILE"; then
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
      if grep -qF -- "$sym" "$DIFF_FILE"; then
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
done <<< "$AC_FOLDED"

echo
if [[ $FAILED -eq 0 ]]; then
  # iter-154 Theme C: say what was actually checked. The previous wording
  # ("backed by diff evidence") read as an endorsement that the ACs were done,
  # and a review agent trusted it that way.
  echo "ac-fidelity: all $TOTAL ticked AC(s) reference evidence that resolves in the diff,"
  echo "declare no non-execution, and carry run evidence where they name a live_* test."
  echo "This check reads a plan and a diff only — it does NOT verify that any test ran."
  exit 0
fi

echo "ac-fidelity: $FAILED/$TOTAL ticked AC(s) failed (see the reason on each ❌ line)."
echo "Add a test, reference the symbol in the diff, record the live run as"
echo "\`[verified: <YYYY-MM-DD>, <measured result>]\`, untick the AC, or annotate"
echo "it \`[deferred — new plan: <path>]\` and file the follow-up plan before merging."
exit 1
