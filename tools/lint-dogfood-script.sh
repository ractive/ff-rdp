#!/usr/bin/env bash
# tools/lint-dogfood-script.sh
#
# ff-rdp-specific linter for .dogfood.sh files.
# Catches common authoring mistakes that no generic linter would flag.
#
# Usage:
#   bash tools/lint-dogfood-script.sh <script.sh> [<script2.sh> ...]
#
# Exit 0  — no lint errors
# Exit 1  — one or more lint errors found
# Exit 2  — usage/invocation error
#
# Rules:
#   unanchored-grep         grep -qi '<token>' for deny-listed tokens (false positives)
#   bool-flag-positional    boolean flag followed by a quoted value instead of --jq <expr>
#   missing-set-euo-pipefail  script must have 'set -euo pipefail' after shebang/comments
#   missing-sentinel-pattern  script must have SENTINEL=, rm -f, and date > "$SENTINEL"
#   shellcheck-clean        runs shellcheck if available; surfaces SC2086/SC2046/SC2155
set -euo pipefail

# Tokens that trigger unanchored-grep warnings when used bare in `grep -qi '<token>'`.
GREP_DENY_LIST="headless error warning firefox"

# Boolean flags that must NOT be followed by a positional value.
BOOL_FLAGS="--jq-strict --headless --replace --force --debug-raw --debug-trace"

errors=0

lint_file() {
  local file="$1"
  local file_errors=0

  if [ ! -f "$file" ]; then
    echo "[lint-dogfood-script] ERROR: file not found: $file" >&2
    errors=$((errors + 1))
    return
  fi

  # Use awk for portable multi-rule scanning — avoids macOS grep ERE limitations.
  # Build the deny list and bool flag list as awk variables.
  local deny_list_awk
  deny_list_awk=$(printf '%s|' $GREP_DENY_LIST | sed 's/|$//')

  local bool_flags_awk
  bool_flags_awk=$(printf '%s|' $BOOL_FLAGS | sed 's/|$//')

  # Run awk to check per-line rules, outputting "FILE:LINE: [rule] message" on stderr.
  local awk_errors
  awk_errors=$(awk -v file="$file" \
    -v deny_list="$deny_list_awk" \
    -v bool_flags="$bool_flags_awk" \
    '
    BEGIN {
      n_deny = split(deny_list, deny_arr, "|")
      n_bool = split(bool_flags, bool_arr, "|")
    }
    {
      line = $0
      lineno = NR

      # Rule: unanchored-grep
      # Detect: grep -qi '"'"'token'"'"' or grep -qi "token" where token is in deny list
      # Matches: grep with both -q and -i flags (any order, possibly combined), then a bare quoted token
      for (i = 1; i <= n_deny; i++) {
        token = deny_arr[i]
        # Pattern: grep ...-qi... or ...-iq... followed by quoted token (single or double quotes)
        sq_pat = "grep[[:space:]]+-[a-zA-Z]*[qi][a-zA-Z]*[qi][a-zA-Z]*[[:space:]]+\x27" token "\x27"
        dq_pat = "grep[[:space:]]+-[a-zA-Z]*[qi][a-zA-Z]*[qi][a-zA-Z]*[[:space:]]+\"" token "\""
        if (line ~ sq_pat || line ~ dq_pat) {
          print file ":" lineno ": [unanchored-grep] bare grep -qi/iq " token " is a false-positive trap — token appears as substring in unrelated strings. Use anchored form, e.g. grep -qiE \"(^|[^a-z])" token "\"" > "/dev/stderr"
          nerrors++
        }
      }

      # Rule: bool-flag-positional
      # Detect: --<bool-flag> followed by a quoted string value (not another --flag)
      for (j = 1; j <= n_bool; j++) {
        flag = bool_arr[j]
        # Pattern: flag then whitespace then single or double quoted value
        sq_pat2 = flag "[[:space:]]+\x27[^\x27]*\x27"
        dq_pat2 = flag "[[:space:]]+\"[^\"]*\""
        if (line ~ sq_pat2 || line ~ dq_pat2) {
          print file ":" lineno ": [bool-flag-positional] " flag " is a boolean flag but is followed by a quoted value. Did you mean --jq <expr>? Example: ff-rdp perf audit " flag " --jq '.results.field'" > "/dev/stderr"
          nerrors++
        }
      }
    }
    END { exit (nerrors > 0) ? 1 : 0 }
    ' "$file" 2>&1 >/dev/null) || true

  if [ -n "$awk_errors" ]; then
    while IFS= read -r awk_line; do
      echo "$awk_line" >&2
    done <<< "$awk_errors"
    file_errors=$(echo "$awk_errors" | wc -l | tr -d ' ')
  fi

  # --- Rule: missing-set-euo-pipefail ---
  if ! grep -qE '^set -[a-zA-Z]*e[a-zA-Z]*u[a-zA-Z]*o[a-zA-Z]* pipefail' "$file"; then
    echo "${file}: [missing-set-euo-pipefail] script must contain 'set -euo pipefail' after the shebang/opening comments" >&2
    file_errors=$((file_errors + 1))
  fi

  # --- Rule: missing-sentinel-pattern ---
  if ! grep -qE '^SENTINEL=/tmp/ff-rdp-iter-[0-9]+-dogfood-ok' "$file"; then
    echo "${file}: [missing-sentinel-pattern] missing SENTINEL=/tmp/ff-rdp-iter-<N>-dogfood-ok assignment" >&2
    file_errors=$((file_errors + 1))
  fi
  if ! grep -qF 'rm -f "$SENTINEL"' "$file"; then
    echo "${file}: [missing-sentinel-pattern] missing 'rm -f \"\$SENTINEL\"' near top of script" >&2
    file_errors=$((file_errors + 1))
  fi
  if ! grep -qE 'date .*> "\$SENTINEL"' "$file"; then
    echo "${file}: [missing-sentinel-pattern] missing final 'date -u ... > \"\$SENTINEL\"' line" >&2
    file_errors=$((file_errors + 1))
  fi

  # --- Rule: shellcheck-clean ---
  if command -v shellcheck >/dev/null 2>&1; then
    # Only surface SC2086 (unquoted), SC2046 (word splitting), SC2155 (masked return value)
    local sc_out
    sc_out=$(shellcheck --severity=warning --include=SC2086,SC2046,SC2155 "$file" 2>&1) || true
    if [ -n "$sc_out" ]; then
      while IFS= read -r sc_line; do
        echo "${file}: [shellcheck-clean] $sc_line" >&2
      done <<< "$sc_out"
      file_errors=$((file_errors + 1))
    fi
  else
    echo "${file}: [shellcheck-clean] SKIP — shellcheck not installed (install with: brew install shellcheck)" >&2
  fi

  errors=$((errors + file_errors))

  if [ "$file_errors" -eq 0 ]; then
    echo "[lint-dogfood-script] OK: $file"
  else
    echo "[lint-dogfood-script] FAIL: $file — ${file_errors} lint error(s)" >&2
  fi
}

if [ $# -eq 0 ]; then
  echo "Usage: $0 <script.sh> [<script2.sh> ...]" >&2
  exit 2
fi

for script in "$@"; do
  lint_file "$script"
done

if [ "$errors" -gt 0 ]; then
  echo "[lint-dogfood-script] ${errors} total error(s)" >&2
  exit 1
fi
exit 0
