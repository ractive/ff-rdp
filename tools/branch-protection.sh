#!/usr/bin/env bash
# tools/branch-protection.sh
#
# Assert that `live-tests` is a required status check on the `main` branch.
#
# Usage:
#   bash tools/branch-protection.sh [OWNER/REPO]
#
# If OWNER/REPO is omitted it is derived from `gh repo view --json nameWithOwner`.
#
# Exit 0  — live-tests is present in required_status_checks.contexts
# Exit 1  — live-tests is missing; prints remediation command
# Exit 2  — gh CLI not found or API call failed
#
# To apply the protection rule for iter-* branches, run:
#   gh api repos/OWNER/REPO/branches/main/protection \
#     --method PUT \
#     --field required_status_checks='{"strict":false,"contexts":["live-tests"]}' \
#     --field enforce_admins=false \
#     --field required_pull_request_reviews=null \
#     --field restrictions=null
set -euo pipefail

REQUIRED_CHECK="live-tests"

# Allow overriding the `gh` binary for testing.
GH="${GH_BIN:-gh}"

if ! command -v "$GH" >/dev/null 2>&1; then
  echo "branch-protection: ERROR — gh CLI not found (searched PATH for: $GH)" >&2
  echo "Install from https://cli.github.com/" >&2
  exit 2
fi

# Resolve repo.
if [ $# -ge 1 ]; then
  REPO="$1"
else
  REPO=$("$GH" repo view --json nameWithOwner --jq '.nameWithOwner' 2>/dev/null) || {
    echo "branch-protection: ERROR — could not determine repo from 'gh repo view'" >&2
    exit 2
  }
fi

# Fetch branch protection for main.
PROTECTION_JSON=$("$GH" api "repos/${REPO}/branches/main/protection" 2>/dev/null) || {
  echo "branch-protection: ERROR — failed to fetch branch protection for ${REPO}/main" >&2
  echo "Ensure the repo has branch protection configured and you have admin read access." >&2
  exit 2
}

# Extract required status check contexts.
CONTEXTS=$(echo "$PROTECTION_JSON" | python3 -c "
import json, sys
data = json.load(sys.stdin)
contexts = (
    data.get('required_status_checks', {}) or {}
).get('contexts', [])
for c in contexts:
    print(c)
" 2>/dev/null) || CONTEXTS=""

# Also try the 'checks' array (newer GitHub API format).
if [ -z "$CONTEXTS" ]; then
  CONTEXTS=$(echo "$PROTECTION_JSON" \
    | python3 -c "
import json, sys
data = json.load(sys.stdin)
rsc = (data.get('required_status_checks', {}) or {})
# Newer API uses 'checks' list of {context, app_id}
checks = rsc.get('checks', [])
for c in checks:
    print(c.get('context', ''))
contexts = rsc.get('contexts', [])
for c in contexts:
    print(c)
" 2>/dev/null) || CONTEXTS=""
fi

if echo "$CONTEXTS" | grep -qxF "$REQUIRED_CHECK"; then
  echo "branch-protection: OK — '${REQUIRED_CHECK}' is a required status check on ${REPO}/main"
  exit 0
else
  echo "branch-protection: FAIL — '${REQUIRED_CHECK}' is NOT in required_status_checks for ${REPO}/main" >&2
  echo "" >&2
  echo "Current required checks:" >&2
  if [ -n "$CONTEXTS" ]; then
    echo "$CONTEXTS" | sed 's/^/  /' >&2
  else
    echo "  (none)" >&2
  fi
  echo "" >&2
  echo "Remediation — PUT replaces required_status_checks.contexts wholesale," >&2
  echo "so include ALL existing required checks plus 'live-tests' in the array:" >&2
  echo "  gh api repos/${REPO}/branches/main/protection \\" >&2
  echo "    --method PUT \\" >&2
  echo "    --field required_status_checks='{\"strict\":false,\"contexts\":[\"live-tests\", <existing checks above>]}' \\" >&2
  echo "    --field enforce_admins=false \\" >&2
  echo "    --field required_pull_request_reviews=null \\" >&2
  echo "    --field restrictions=null" >&2
  exit 1
fi
