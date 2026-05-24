#!/usr/bin/env bash
set -euo pipefail

# claims-vs-code.sh — Extract claim-bearing sentences from iteration commit
# messages and verify each claim is supported by the branch diff. Emits a
# markdown "## Claims vs code" section to stdout. Exit 0 if every claim has a
# match in the diff (or is whitelisted with `// allow-claim-miss: <reason>`),
# exit 1 otherwise.
#
# Usage:
#   claims-vs-code.sh [--branch <branch>] [--base <base-ref>]
#
# If --branch is omitted, reads from $RALPH_CACHE_DIR/state.json (key
# `current_branch`), then falls back to the current git HEAD branch. --base
# defaults to `main`.

BRANCH=""
BASE="main"
RANGE=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --branch) BRANCH="$2"; shift 2 ;;
    --base)   BASE="$2"; shift 2 ;;
    --range)  RANGE="$2"; shift 2 ;;
    -h|--help)
      sed -n '3,16p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

# --range A..B overrides --branch/--base. Used by replay mode against an
# already-merged branch: pass <merge-commit>^1..<merge-commit>^2 to get the
# commits that landed via the merge.
if [[ -n "$RANGE" ]]; then
  REV_RANGE="$RANGE"
else
  REV_RANGE=""
fi

if [[ -z "$REV_RANGE" ]]; then
  if [[ -z "$BRANCH" ]]; then
    if [[ -n "${RALPH_CACHE_DIR:-}" && -f "$RALPH_CACHE_DIR/state.json" ]]; then
      BRANCH=$(grep -oE '"current_branch"[[:space:]]*:[[:space:]]*"[^"]+"' "$RALPH_CACHE_DIR/state.json" 2>/dev/null \
        | head -1 | sed -E 's/.*"current_branch"[^"]*"([^"]+)".*/\1/' || true)
    fi
  fi
  if [[ -z "$BRANCH" ]]; then
    BRANCH=$(git rev-parse --abbrev-ref HEAD)
  fi

  # Verify the merge-base exists.
  if ! git rev-parse --verify --quiet "$BASE" >/dev/null; then
    if git rev-parse --verify --quiet "origin/$BASE" >/dev/null; then
      BASE="origin/$BASE"
    else
      echo "ERROR: base ref '$BASE' not found" >&2
      exit 2
    fi
  fi
  REV_RANGE="$BASE..$BRANCH"
fi

# Collect commit messages for the rev range.
COMMITS=$(git log --format='%s%n%b%n--END--' "$REV_RANGE" 2>/dev/null || true)

if [[ -z "$COMMITS" ]]; then
  echo "## Claims vs code"
  echo "<generated $(date -u +%Y-%m-%dT%H:%M:%SZ) by ralph-loop>"
  echo
  echo "_No commits in range \`$REV_RANGE\`._"
  exit 0
fi

# Whole-branch diff. For `A..B` ranges, `git diff A..B` is equivalent to
# `git diff A B` (compares endpoints). For replay against a merge commit, the
# caller passes <merge>^1..<merge>^2 which gives the correct branch-side diff.
# Write the diff to a temp file rather than holding it in a shell variable —
# `printf '%s' "$DIFF" | grep -qF ...` raises SIGPIPE on macOS bash when grep
# exits early, and `set -o pipefail` turns that into a false-negative match.
DIFF_FILE=$(mktemp -t claims-vs-code.XXXXXX)
trap 'rm -f "$DIFF_FILE"' EXIT
# Restrict to actual code (exclude kb/ markdown and the plan itself, where
# claim-tokens appear as documentation and would produce false ✅s).
#
# We do NOT fall back to the full diff when this is empty — falling back
# would re-include kb/ docs and let commit claims match their own plan text
# (CodeRabbit caught this in PR #91). Docs-only branches will report every
# claim as ❌; that's the honest signal. If a docs-only iteration is
# legitimate, soften the commit message or add an `allow-claim-miss`
# annotation.
git diff "$REV_RANGE" -- \
  ':(exclude)kb/' ':(exclude)*.md' ':(exclude)CHANGELOG*' \
  > "$DIFF_FILE" 2>/dev/null || true

# Extract `// allow-claim-miss: <symbol>` whitelist lines from the diff.
WHITELIST=$(grep -oE 'allow-claim-miss:[[:space:]]*[A-Za-z0-9_:#.-]+' "$DIFF_FILE" 2>/dev/null \
  | sed -E 's/.*allow-claim-miss:[[:space:]]*//' | sort -u || true)

# Extract distinctive code-shaped tokens from commit messages. We catch:
#   1. verb + symbol claims  ("adds Foo::Bar", "implements baz", "closes #42")
#   2. ::-qualified paths    ("RdpError::Navigation", "Foo::bar::Baz")
#   3. SCREAMING_SNAKE_CASE  ("SCREENSHOT_JS_PROGRAM")
#   4. kebab-event tokens    ("dom-interactive", "chrome-context")
#   5. issue closures        ("#42")
# Tokens are reported as bare symbols so the diff search can match the same
# token literally. Per design notes the script is conservative: false ✅ is
# preferable to false ❌, but we still need to surface the obvious misses.

extract_tokens() {
  # grep returns 1 on no-match; pipe through `|| true` to keep set -e happy.
  { printf "%s\n" "$COMMITS" | grep -oEi '(adds|implements|wires|fixes|closes)[[:space:]]+(#?[A-Za-z_][A-Za-z0-9_:.\-]*)' \
      | sed -E 's/^[A-Za-z]+[[:space:]]+//' || true; } # 1. verb + symbol
  printf "%s\n" "$COMMITS" | grep -oE '[A-Z][A-Za-z0-9_]+(::[A-Za-z_][A-Za-z0-9_]*)+' || true
  printf "%s\n" "$COMMITS" | grep -oE '\b[A-Z][A-Z0-9_]{4,}\b' | grep -E '_.*_' || true
  printf "%s\n" "$COMMITS" | grep -oE '\b(dom|will|chrome|net|tab|target|resource|frame)-[a-z][a-z0-9-]+\b' || true
  printf "%s\n" "$COMMITS" | grep -oE '#[0-9]+' || true
}

CLAIMS=()
SEEN_KEYS=$'\n'
while IFS= read -r match; do
  [[ -z "$match" ]] && continue
  # Skip noise tokens that show up everywhere.
  case "$match" in
    iter-*|Co-Authored-By|README|CLAUDE|PR|CI|JS|DOM|API|URL|HTTP) continue ;;  # allow-todo: noise-token list, not a real TODO marker
    TODO|FIXME|XXX) continue ;;  # allow-todo: noise-token list, not a real TODO marker
  esac
  key=$(printf '%s' "$match" | tr '[:upper:]' '[:lower:]')
  if [[ "$SEEN_KEYS" != *$'\n'"$key"$'\n'* ]]; then
    SEEN_KEYS="${SEEN_KEYS}${key}"$'\n'
    CLAIMS+=("$match")
  fi
done < <(extract_tokens)

echo "## Claims vs code"
echo "<generated $(date -u +%Y-%m-%dT%H:%M:%SZ) by ralph-loop>"
echo

if [[ ${#CLAIMS[@]} -eq 0 ]]; then
  echo "_No claims extracted from commit messages._"
  exit 0
fi

MISSES=0
for symbol in "${CLAIMS[@]}"; do
  ok=0
  evidence=""
  if [[ "$symbol" == \#* ]]; then
    if grep -qF "$symbol" "$DIFF_FILE"; then ok=1; fi
  else
    # Try the whole symbol literally (preserves :: and -).
    if grep -qF "$symbol" "$DIFF_FILE"; then
      ok=1
    else
      # Path-qualified: try the last component.
      last=${symbol##*::}
      if [[ "$last" != "$symbol" && -n "$last" ]]; then
        if grep -qE "[^A-Za-z0-9_]${last}([^A-Za-z0-9_]|$)" "$DIFF_FILE"; then
          ok=1
        fi
      fi
    fi
  fi

  # Whitelisted?
  if [[ $ok -eq 0 ]] && [[ -n "$WHITELIST" ]] && printf '%s\n' "$WHITELIST" | grep -qxF "$symbol"; then
    printf -- '- `%s` → ⚠️ no match in diff (whitelisted via `allow-claim-miss`)\n' "$symbol"
    continue
  fi

  if [[ $ok -eq 1 ]]; then
    printf -- '- `%s` → ✅ matched in diff\n' "$symbol"
  else
    printf -- '- `%s` → ❌ no match in diff\n' "$symbol"
    MISSES=$((MISSES + 1))
  fi
done

echo

if [[ $MISSES -gt 0 ]]; then
  echo "_$MISSES claim(s) had no matching evidence. Add the code, soften the_"
  echo "_commit message, or annotate with \`// allow-claim-miss: <symbol>\` and a reason._"
  exit 1
fi

exit 0
