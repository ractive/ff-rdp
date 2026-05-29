#!/usr/bin/env bash
# iter-87 dogfood gate — exercises the linter and the fail-by-default gate.
# Executed by `cargo run -p xtask -- check-dogfood-script <plan>`.
#
# Run manually:
#   FF_RDP_LIVE_TESTS=1 bash kb/iterations/iteration-87-*.dogfood.sh
set -euo pipefail

SENTINEL=/tmp/ff-rdp-iter-87-dogfood-ok
rm -f "$SENTINEL"

REPO_ROOT="$(git rev-parse --show-toplevel)"
LINTER="$REPO_ROOT/tools/lint-dogfood-script.sh"
SELF_SCRIPT="$REPO_ROOT/kb/iterations/iteration-87-gate-hardening-required-checks-and-dogfood-linter.dogfood.sh"

# --- Theme C check 1: linter exits non-zero on a known-bad fixture ---
# Use the checked-in fixture rather than an inline heredoc (avoids lint false-positives
# on this script's own content).
BAD="$REPO_ROOT/tools/tests/lint-dogfood-script/unanchored-grep-bad.sh"
if [ ! -f "$BAD" ]; then
  echo "FAIL Theme C: fixture not found: $BAD" >&2
  exit 1
fi
if "$LINTER" "$BAD" >/dev/null 2>&1; then
  echo "FAIL Theme C: linter accepted a known-bad fixture ($BAD)" >&2
  exit 1
fi

# --- Theme C check 2: linter exits 0 on iter-87's own script ---
"$LINTER" "$SELF_SCRIPT" || { echo "FAIL Theme C: linter rejected iter-87's own script" >&2; exit 1; }

# --- Theme B check: check-dogfood-script FAILs on iter-* branch when FF_RDP_LIVE_TESTS unset ---
# Use BRANCH_NAME override (the implementation should honor it for testability).
PLAN="$REPO_ROOT/kb/iterations/iteration-87-gate-hardening-required-checks-and-dogfood-linter.md"
set +e
BRANCH_NAME=iter-99/fake-test \
  env -u FF_RDP_LIVE_TESTS \
  cargo run -q -p xtask -- check-dogfood-script "$PLAN" >/tmp/iter87-gate.out 2>&1
GATE_EC=$?
set -e
test "$GATE_EC" -ne 0 || { echo "FAIL Theme B: fail-by-default gate exited 0 on iter-* branch w/o FF_RDP_LIVE_TESTS" >&2; cat /tmp/iter87-gate.out >&2; exit 1; }
grep -qi 'FF_RDP_LIVE_TESTS' /tmp/iter87-gate.out || { echo "FAIL Theme B: diagnostic missing FF_RDP_LIVE_TESTS hint" >&2; exit 1; }

# --- Theme A check: branch-protection script asserts live-tests required ---
# Uses a fake gh shim so the test is network-free.
PROT_SCRIPT="$REPO_ROOT/tools/branch-protection.sh"
FIXTURE_DIR="$REPO_ROOT/tools/tests/branch-protection"
if [ -x "$PROT_SCRIPT" ]; then
  # Build a temporary fake gh binary that cats a fixture JSON file.
  TMP_GH_DIR=$(mktemp -d -t iter87-gh-XXXX)
  GH_SHIM="$TMP_GH_DIR/gh"

  # --- Pass case: fixture contains live-tests ---
  printf '#!/usr/bin/env bash\nif [[ "$*" == *"nameWithOwner"* ]]; then echo '"'"'{"nameWithOwner":"ractive/ff-rdp"}'"'"'; exit 0; fi\ncat "%s/has-live-tests.json"\n' \
    "$FIXTURE_DIR" > "$GH_SHIM"
  chmod +x "$GH_SHIM"
  if ! GH_BIN="$GH_SHIM" PATH="$TMP_GH_DIR:$PATH" "$PROT_SCRIPT" ractive/ff-rdp >/dev/null 2>&1; then
    echo "FAIL Theme A: branch-protection.sh rejected has-live-tests fixture" >&2
    rm -rf "$TMP_GH_DIR"
    exit 1
  fi

  # --- Fail case: fixture missing live-tests ---
  printf '#!/usr/bin/env bash\nif [[ "$*" == *"nameWithOwner"* ]]; then echo '"'"'{"nameWithOwner":"ractive/ff-rdp"}'"'"'; exit 0; fi\ncat "%s/missing-live-tests.json"\n' \
    "$FIXTURE_DIR" > "$GH_SHIM"
  chmod +x "$GH_SHIM"
  if GH_BIN="$GH_SHIM" PATH="$TMP_GH_DIR:$PATH" "$PROT_SCRIPT" ractive/ff-rdp >/dev/null 2>&1; then
    echo "FAIL Theme A: branch-protection.sh accepted missing-live-tests fixture (expected exit 1)" >&2
    rm -rf "$TMP_GH_DIR"
    exit 1
  fi

  rm -rf "$TMP_GH_DIR"
fi

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
echo "iter-87 dogfood: gate hardening verified — $SENTINEL"
