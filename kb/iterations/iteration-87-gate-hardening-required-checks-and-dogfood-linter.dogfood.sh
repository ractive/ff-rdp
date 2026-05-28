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
BAD=$(mktemp -t iter87-bad-XXXX.sh)
cat > "$BAD" <<'EOF'
#!/usr/bin/env bash
# Missing set -euo pipefail, missing sentinel, has unanchored grep + bool-flag-positional.
ff-rdp perf audit --jq-strict '.results.nope'
NOTE=$(ff-rdp perf audit --jq '.results.vitals.lcp_note // ""')
echo "$NOTE" | grep -qi 'headless' && exit 1
EOF
if "$LINTER" "$BAD" >/dev/null 2>&1; then
  echo "FAIL Theme C: linter accepted a known-bad script" >&2
  rm -f "$BAD"
  exit 1
fi
rm -f "$BAD"

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
# Uses a mock payload fixture so the test is network-free.
PROT_SCRIPT="$REPO_ROOT/tools/branch-protection.sh"
if [ -x "$PROT_SCRIPT" ]; then
  # The script supports --mock-payload <path> for local verification (per AC).
  GOOD=$(mktemp -t iter87-prot-good-XXXX.json)
  cat > "$GOOD" <<'EOF'
{"required_status_checks":{"contexts":["live-tests","fmt","clippy"]}}
EOF
  "$PROT_SCRIPT" --mock-payload "$GOOD" || { echo "FAIL Theme A: branch-protection.sh rejected a valid payload" >&2; rm -f "$GOOD"; exit 1; }
  rm -f "$GOOD"

  BAD_P=$(mktemp -t iter87-prot-bad-XXXX.json)
  cat > "$BAD_P" <<'EOF'
{"required_status_checks":{"contexts":["fmt","clippy"]}}
EOF
  if "$PROT_SCRIPT" --mock-payload "$BAD_P" >/dev/null 2>&1; then
    echo "FAIL Theme A: branch-protection.sh accepted a payload missing live-tests" >&2
    rm -f "$BAD_P"
    exit 1
  fi
  rm -f "$BAD_P"
fi

date -u +%Y-%m-%dT%H:%M:%SZ > "$SENTINEL"
echo "iter-87 dogfood: gate hardening verified — $SENTINEL"
