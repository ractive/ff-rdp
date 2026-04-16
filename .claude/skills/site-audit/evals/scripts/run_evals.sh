#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
EVALS_DIR="$(dirname "$SCRIPT_DIR")"
EVALS_FILE="$EVALS_DIR/evals.json"
RESULTS_DIR="$EVALS_DIR"
TIMESTAMP=$(date +%Y%m%d-%H%M%S)
RESULTS_FILE="$RESULTS_DIR/results-${TIMESTAMP}.json"

# Parse arguments
EVAL_NAME="${1:-}"
COMPARE_MODE=false

if [ "$EVAL_NAME" = "--compare-chrome-mcp" ]; then
  echo "Chrome MCP comparison mode: Not implemented"
  echo "This is a stub for future iteration. Run without --compare-chrome-mcp for ff-rdp evals."
  exit 0
fi

# Check dependencies
command -v jq >/dev/null 2>&1 || { echo "ERROR: jq is required but not found"; exit 1; }
command -v ff-rdp >/dev/null 2>&1 || { echo "ERROR: ff-rdp is required but not found"; exit 1; }

if [ ! -f "$EVALS_FILE" ]; then
  echo "ERROR: evals.json not found at $EVALS_FILE"
  exit 1
fi

# Read eval names
EVAL_NAMES=$(jq -r '.evals[].name' "$EVALS_FILE")

# Filter to specific eval if requested
if [ -n "$EVAL_NAME" ]; then
  if ! echo "$EVAL_NAMES" | grep -q "^${EVAL_NAME}$"; then
    echo "ERROR: eval '$EVAL_NAME' not found. Available:"
    echo "$EVAL_NAMES" | sed 's/^/  /'
    exit 1
  fi
  EVAL_NAMES="$EVAL_NAME"
fi

TOTAL_PASS=0
TOTAL_FAIL=0
TOTAL_ASSERTIONS=0
START_TIME=$(date +%s)

# JSON results accumulator
RESULTS_JSON='{"timestamp":"'"$TIMESTAMP"'","evals":[]}'

echo "=================================="
echo "  ff-rdp Site Audit Eval Suite"
echo "=================================="
echo ""

for EVAL in $EVAL_NAMES; do
  echo "--- $EVAL ---"

  EVAL_INDEX=$(jq -r --arg name "$EVAL" '.evals | to_entries[] | select(.value.name == $name) | .key' "$EVALS_FILE")
  EVAL_URL=$(jq -r --arg name "$EVAL" '.evals[] | select(.name == $name) | .url' "$EVALS_FILE")
  ASSERTION_COUNT=$(jq -r --arg name "$EVAL" '.evals[] | select(.name == $name) | .assertions | length' "$EVALS_FILE")

  echo "URL: $EVAL_URL"
  echo "Assertions: $ASSERTION_COUNT"
  echo ""

  EVAL_PASS=0
  EVAL_FAIL=0
  EVAL_RESULTS='[]'

  for i in $(seq 0 $((ASSERTION_COUNT - 1))); do
    DESC=$(jq -r --arg name "$EVAL" --argjson i "$i" '.evals[] | select(.name == $name) | .assertions[$i].desc' "$EVALS_FILE")
    COMMAND=$(jq -r --arg name "$EVAL" --argjson i "$i" '.evals[] | select(.name == $name) | .assertions[$i].command' "$EVALS_FILE")
    EXPECTED=$(jq -r --arg name "$EVAL" --argjson i "$i" '.evals[] | select(.name == $name) | .assertions[$i].expected' "$EVALS_FILE")
    CATEGORY=$(jq -r --arg name "$EVAL" --argjson i "$i" '.evals[] | select(.name == $name) | .assertions[$i].category' "$EVALS_FILE")
    CHECK=$(jq -r --arg name "$EVAL" --argjson i "$i" '.evals[] | select(.name == $name) | .assertions[$i].check' "$EVALS_FILE")

    CMD_START=$(date +%s%N 2>/dev/null || date +%s)
    OUTPUT=$(eval "$COMMAND" 2>&1) || true
    CMD_END=$(date +%s%N 2>/dev/null || date +%s)

    # Calculate duration in ms (fall back to seconds if %N not supported)
    if [[ "$CMD_START" =~ ^[0-9]{10,}$ ]]; then
      DURATION_MS=$(( (CMD_END - CMD_START) / 1000000 ))
    else
      DURATION_MS=$(( (CMD_END - CMD_START) * 1000 ))
    fi

    # Check output against expected regex
    if echo "$OUTPUT" | grep -qE "$EXPECTED"; then
      STATUS="PASS"
      EVAL_PASS=$((EVAL_PASS + 1))
      TOTAL_PASS=$((TOTAL_PASS + 1))
    else
      STATUS="FAIL"
      EVAL_FAIL=$((EVAL_FAIL + 1))
      TOTAL_FAIL=$((TOTAL_FAIL + 1))
    fi

    TOTAL_ASSERTIONS=$((TOTAL_ASSERTIONS + 1))

    printf "  %-4s [%s] %s (%dms)\n" "$STATUS" "$CATEGORY" "$DESC" "$DURATION_MS"

    # Accumulate assertion result
    ASSERTION_RESULT=$(jq -n \
      --arg check "$CHECK" \
      --arg category "$CATEGORY" \
      --arg desc "$DESC" \
      --arg status "$STATUS" \
      --argjson duration_ms "$DURATION_MS" \
      --arg output "$OUTPUT" \
      '{check: $check, category: $category, desc: $desc, status: $status, duration_ms: $duration_ms, output: ($output | if length > 200 then .[:200] + "..." else . end)}')
    EVAL_RESULTS=$(echo "$EVAL_RESULTS" | jq --argjson r "$ASSERTION_RESULT" '. + [$r]')
  done

  echo ""
  echo "  Result: $EVAL_PASS/$ASSERTION_COUNT passed"
  echo ""

  # Add eval result to JSON
  EVAL_RESULT=$(jq -n \
    --arg name "$EVAL" \
    --arg url "$EVAL_URL" \
    --argjson pass "$EVAL_PASS" \
    --argjson fail "$EVAL_FAIL" \
    --argjson assertions "$EVAL_RESULTS" \
    '{name: $name, url: $url, passed: $pass, failed: $fail, assertions: $assertions}')
  RESULTS_JSON=$(echo "$RESULTS_JSON" | jq --argjson e "$EVAL_RESULT" '.evals += [$e]')
done

END_TIME=$(date +%s)
TOTAL_SECONDS=$((END_TIME - START_TIME))

echo "=================================="
echo "  Summary"
echo "=================================="
echo "  $TOTAL_PASS/$TOTAL_ASSERTIONS passed in ${TOTAL_SECONDS}s"

if [ "$TOTAL_FAIL" -gt 0 ]; then
  echo "  $TOTAL_FAIL FAILED"
fi

echo "=================================="

# Write results file
RESULTS_JSON=$(echo "$RESULTS_JSON" | jq \
  --argjson total_pass "$TOTAL_PASS" \
  --argjson total_fail "$TOTAL_FAIL" \
  --argjson total_assertions "$TOTAL_ASSERTIONS" \
  --argjson duration_s "$TOTAL_SECONDS" \
  '. + {total_passed: $total_pass, total_failed: $total_fail, total_assertions: $total_assertions, duration_s: $duration_s}')

echo "$RESULTS_JSON" | jq . > "$RESULTS_FILE"
echo ""
echo "Results written to: $RESULTS_FILE"

# Exit with error if any failures
[ "$TOTAL_FAIL" -eq 0 ] || exit 1
