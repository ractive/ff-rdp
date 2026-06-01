#!/usr/bin/env bash
set -euo pipefail

# run-iteration.sh — Launch interactive claude sessions in a cmux pane for one iteration.
# Split into two phases with fresh context each:
#   Phase 1: Implement (heavy — consumes most context)
#   Phase 2: Create PR, review, fix, adapt next iteration, merge (lightweight)
#
# Usage: run-iteration.sh <iteration-number> <plan-file-path> [review]
#   If "review" is passed as 3rd arg, skip Phase 1 and go straight to Phase 2.
#   If first arg is `--replay <iter-id>`, skip both phases and run the
#   discipline replay (claims-vs-code + ac-fidelity) against the merged branch
#   for <iter-id>. Output is written to $RALPH_CACHE_DIR/replay-<iter-id>.txt
#   (or a temp dir if RALPH_CACHE_DIR is unset). Exit 0 if both checks pass,
#   exit 1 if either fails.
# Exit code: 0 on success, 1 on failure, 2 on throttle

# --- Replay mode (no Phase 1/2; just run discipline checks against a merged branch).
if [[ "${1:-}" == "--replay" ]]; then
  REPLAY_ITER="${2:?Usage: run-iteration.sh --replay <iter-id>}"
  # Accept both `61v` and `iter-61v` so docs that quote either form work.
  REPLAY_ITER="${REPLAY_ITER#iter-}"
  SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
  # Anchor to the repo root so `git log`/`git ls-tree` work regardless of
  # which directory the caller invoked the script from.
  REPO_ROOT_REPLAY=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
  cd "$REPO_ROOT_REPLAY"
  CACHE_DIR="${RALPH_CACHE_DIR:-$(mktemp -d -t ralph-replay.XXXXXX)}"
  mkdir -p "$CACHE_DIR"
  OUT="$CACHE_DIR/replay-${REPLAY_ITER}.txt"

  # Find the merge commit for this iter on main. On CI the local `main`
  # branch may not exist (detached HEAD on the PR commit) — fall back to
  # `origin/main`. Use `-1` instead of `| head -1` so SIGPIPE under
  # `set -o pipefail` can't kill the assignment.
  MAIN_REF=main
  if ! git rev-parse --verify --quiet main >/dev/null 2>&1; then
    if git rev-parse --verify --quiet origin/main >/dev/null 2>&1; then
      MAIN_REF=origin/main
    fi
  fi
  # Match `iter-NN/` (branch slug) or `iter-NN)` (commit subject) so e.g.
  # `iter-61v` doesn't substring-match `iter-61va`. Suffix the grep with a
  # non-alphanumeric boundary by alternating both forms.
  MERGE_COMMIT=$(git log -1 --merges \
    --grep="iter-${REPLAY_ITER}/" \
    --format=%H "$MAIN_REF" 2>/dev/null || true)
  if [[ -z "$MERGE_COMMIT" ]]; then
    # Fall back to the looser match for legacy commit subjects that don't
    # include the slash.
    MERGE_COMMIT=$(git log -1 --merges \
      --grep="iter-${REPLAY_ITER}" \
      --format=%H "$MAIN_REF" 2>/dev/null || true)
  fi
  if [[ -z "$MERGE_COMMIT" ]]; then
    echo "ERROR: no merge commit found on $MAIN_REF matching iter-${REPLAY_ITER}" >&2
    exit 1
  fi
  RANGE="${MERGE_COMMIT}^1..${MERGE_COMMIT}^2"

  # Find the plan file as of the merge commit. The plan name follows
  # `kb/iterations/iteration-<id>-<slug>.md`.
  PLAN_REL=$(git ls-tree -r "${MERGE_COMMIT}^2" --name-only -- kb/iterations 2>/dev/null \
    | grep -E "iteration-${REPLAY_ITER}-[^/]+\.md$" | head -1 || true)
  if [[ -z "$PLAN_REL" ]]; then
    echo "WARN: no plan file found on iter-${REPLAY_ITER} branch — ac-fidelity will be skipped" >&2
    PLAN_PATH_REPLAY=""
  else
    PLAN_PATH_REPLAY="$(mktemp -t replay-plan.XXXXXX).md"
    git show "${MERGE_COMMIT}^2:${PLAN_REL}" > "$PLAN_PATH_REPLAY" 2>/dev/null || PLAN_PATH_REPLAY=""
  fi

  {
    echo "# Replay report for iter-${REPLAY_ITER}"
    echo "merge_commit: $MERGE_COMMIT"
    echo "range: $RANGE"
    echo "plan: ${PLAN_REL:-<none>}"
    echo
  } > "$OUT"

  echo "[replay] running claims-vs-code on $RANGE..."
  CVC_EXIT=0
  "$SCRIPT_DIR/claims-vs-code.sh" --range "$RANGE" >> "$OUT" 2>&1 || CVC_EXIT=$?
  echo >> "$OUT"

  ACF_EXIT=0
  if [[ -n "$PLAN_PATH_REPLAY" && -f "$PLAN_PATH_REPLAY" ]]; then
    echo "[replay] running ac-fidelity-check on plan..."
    "$SCRIPT_DIR/ac-fidelity-check.sh" --plan "$PLAN_PATH_REPLAY" --range "$RANGE" >> "$OUT" 2>&1 || ACF_EXIT=$?
    rm -f "$PLAN_PATH_REPLAY"
  fi

  cat "$OUT"
  echo
  echo "[replay] output written to $OUT"

  if [[ $CVC_EXIT -ne 0 || $ACF_EXIT -ne 0 ]]; then
    echo "[replay] iter-${REPLAY_ITER}: FAIL (claims=$CVC_EXIT, ac-fidelity=$ACF_EXIT)"
    exit 1
  fi
  echo "[replay] iter-${REPLAY_ITER}: PASS"
  exit 0
fi

ITER_NUM="${1:?Usage: run-iteration.sh <iteration-number> <plan-file-path> [review]}"
PLAN_PATH="${2:?Usage: run-iteration.sh <iteration-number> <plan-file-path> [review]}"
SKIP_TO_REVIEW="${3:-}"

# Sentinel file writer for the orchestrator (Monitor + until pattern).
# If RALPH_CACHE_DIR is set, write iter-N-{done,failed,throttled} on exit so
# a watching orchestrator gets one notification rather than polling.
on_exit() {
  local code=$?
  if [[ -n "${RALPH_CACHE_DIR:-}" ]]; then
    local kind
    case "$code" in
      0) kind="done" ;;
      2) kind="throttled" ;;
      *) kind="failed" ;;
    esac
    mkdir -p "$RALPH_CACHE_DIR"
    {
      date -u +"%Y-%m-%dT%H:%M:%SZ"
      echo "exit_code=$code"
      if [[ -n "${BRANCH_NAME:-}" ]]; then
        echo "branch=$BRANCH_NAME"
      fi
    } > "$RALPH_CACHE_DIR/iter-${ITER_NUM}-${kind}"
  fi
  # Must return 0: bash propagates a trap's last exit status to the shell's
  # exit code, overriding `exit 0`. A bare `[[ ]] && cmd` whose left side is
  # false returns 1 and silently turned every successful run into exit 1.
  return 0
}
trap on_exit EXIT

if [[ ! -f "$PLAN_PATH" ]]; then
  echo "ERROR: Plan file not found: $PLAN_PATH" >&2
  exit 1
fi

STATUS_KEY="iter-${ITER_NUM}"

# --- cmux workspace targeting ---
# RALPH_CMUX_WORKSPACE is set by the orchestrator from state.json.cmux_workspace,
# which preflight.sh captured via `cmux current-workspace` in the orchestrator's
# session. Passing it explicitly to workspace-scoped cmux commands ensures the
# pane / status / log lands in the right workspace even when this script runs
# detached (nohup/disown sever the calling-process context cmux uses to infer
# "current workspace"). If unset, fall back to letting cmux infer.
WS_FLAG=()
if [[ -n "${RALPH_CMUX_WORKSPACE:-}" ]]; then
  WS_FLAG=(--workspace "$RALPH_CMUX_WORKSPACE")
fi

DONE_FILE="/tmp/ralph-loop-done-${ITER_NUM}.$$"
USAGE_FILE="/tmp/ralph-loop-usage-${ITER_NUM}.$$"
USAGE_THRESHOLD="${RALPH_USAGE_THRESHOLD:-90}"
POLL_INTERVAL="${RALPH_POLL_INTERVAL:-10}"
# Soft budget per phase (default 4h). When elapsed exceeds this, we check
# is_child_alive: a still-working pane gets the budget extended and a warning
# logged; a dead pane is declared a true timeout. Bumped from 2h after two
# review-phase incidents where claude was making genuine progress past 2h
# (iter-92 first attempt, mid-2026 ralph-loop session-59 follow-up).
PHASE_TIMEOUT="${RALPH_PHASE_TIMEOUT:-14400}"
DONE_SENTINEL="When ALL steps above are complete and successful, run this exact command: echo 0 > ${DONE_FILE} — if any step fails, run: echo 1 > ${DONE_FILE}"

PROMPT_IMPLEMENT="You are implementing iteration ${ITER_NUM} of this project. Steps: 1) Create a new branch for this iteration (e.g. iter-${ITER_NUM}/short-description) from main 2) Read the iteration plan from ${PLAN_PATH} 3) Implement everything in the plan (code, tests, error handling) using agents whenever possible 4) Ensure all docs, help texts, and project documentation are updated to reflect changes 5) Run the project quality gates (read CLAUDE.md for the specific commands) 6) Run \`cargo run -p xtask -- check-iteration-ready --plan ${PLAN_PATH} --base origin/main\` and fix every reported failure — do not proceed until this exits 0 7) /create-pr 8) Immediately after the PR exists, kick off the GitHub Copilot review by running: gh pr edit <PR-number> --add-reviewer @copilot — fire-and-forget, do NOT wait for the review to come back. This lets Copilot run in parallel while phase 2 starts. ${DONE_SENTINEL}"

# Compute the *likely* next iteration ID for the Phase 2 plan-adaptation step.
# Pure-integer ITER_NUM increments (16 → 17). Letter-suffixed ITER_NUM (16b →
# 16c). The Phase 2 prompt only uses NEXT_ITER for a hyalo-find lookup; if no
# plan exists at that ID, the prompt does nothing — so an over-eager guess is
# harmless. (When running the last letter in a same-base range, e.g. 16g, the
# computed NEXT_ITER=16h won't exist; the lookup misses cleanly. The actual
# next iteration after a letter range is the next bare integer, but the
# orchestrator drives that via state.json, not this prompt.)
#
# IMPORTANT: capture BASH_REMATCH groups into local variables immediately
# after the [[ =~ ]] match. Command substitutions and intervening shell
# operations can clobber BASH_REMATCH before it's read.
if [[ "$ITER_NUM" =~ ^([0-9]+)([a-z])$ ]]; then
  _iter_base="${BASH_REMATCH[1]}"
  _iter_letter="${BASH_REMATCH[2]}"
  _next_letter=$(echo "$_iter_letter" | tr 'a-y' 'b-z')
  NEXT_ITER="${_iter_base}${_next_letter}"
elif [[ "$ITER_NUM" =~ ^[0-9]+$ ]]; then
  NEXT_ITER=$((ITER_NUM + 1))
else
  # Unparseable ID — produce a stable string so the prompt doesn't break.
  NEXT_ITER="${ITER_NUM}-next"
fi
PROMPT_REVIEW="You are reviewing iteration ${ITER_NUM} of this project. A PR has already been created and the GitHub Copilot review was kicked off at the end of phase 1, so it should already be in progress or complete by now. Steps: 0) If a discipline report exists at \${RALPH_CACHE_DIR:-.ralph-cache}/iter-${ITER_NUM}-discipline.log, read it. For each FAIL or ❌ line, either implement the missing test/code so the AC text matches the diff, soften the AC text to match what shipped, or annotate the AC with \`[deferred — new plan: iteration-NN-slug.md]\` and create the follow-up plan. Do not proceed to step 1 until \`cargo run -p xtask -- check-iteration-ready --plan ${PLAN_PATH} --base origin/main\` exits 0. 1) If a claims-vs-code report exists at \${RALPH_CACHE_DIR:-.ralph-cache}/iter-${ITER_NUM}-claims.md, append its contents to the PR body via: gh pr edit <PR-number> --body \"\$(gh pr view <PR-number> --json body -q .body)\$(printf '\\n\\n')\$(cat \${RALPH_CACHE_DIR:-.ralph-cache}/iter-${ITER_NUM}-claims.md)\" — this surfaces any ❌ rows to the reviewer. 2) /review-pr and fix all review issues (the Copilot trigger inside /review-pr is idempotent — safe to run again if needed) 3) Update the iteration ${ITER_NUM} plan at ${PLAN_PATH}: tick every \`- [ ]\` scope checkbox whose work actually landed in this PR (verify against the merged diff — do NOT tick speculatively), and update each scope-section heading's \`[N/M]\` count to reflect the real state. Leave any genuinely incomplete checkbox unchecked and note why in the section. Commit the plan update onto the PR branch and push. 4) Check if a plan exists for iteration ${NEXT_ITER} — find it with: hyalo find --glob '**/iteration-${NEXT_ITER}-*.md' --format text (do NOT use --property 'title~=...' — frontmatter title fields like 'Iteration ${NEXT_ITER}: Slug' do not contain the substring 'iteration-${NEXT_ITER}'). If found, check whether its scope needs to be adapted based on what you learned this iteration, and update it if so 5) /merge-pr. ${DONE_SENTINEL}"

# --- cmux visual helpers (all soft-fail with || true) ---

status() {
  local label="$1" icon="${2:-gear}" color="${3:-}"
  if [[ -n "$color" ]]; then
    cmux set-status "$STATUS_KEY" "$label" --icon "$icon" --color "$color" "${WS_FLAG[@]}" 2>/dev/null || true
  else
    cmux set-status "$STATUS_KEY" "$label" --icon "$icon" "${WS_FLAG[@]}" 2>/dev/null || true
  fi
}

progress() {
  cmux set-progress "$1" --label "$2" "${WS_FLAG[@]}" 2>/dev/null || true
}

log_info() {
  cmux log "$1" --level info --source "ralph" "${WS_FLAG[@]}" 2>/dev/null || true
}

log_success() {
  cmux log "$1" --level success --source "ralph" "${WS_FLAG[@]}" 2>/dev/null || true
}

log_error() {
  cmux log "$1" --level error --source "ralph" "${WS_FLAG[@]}" 2>/dev/null || true
}

flash() {
  cmux trigger-flash --surface "$SURFACE_ID" 2>/dev/null || true
}

# Check if the claude process is still running in the cmux pane.
# Returns 0 if alive, 1 if dead/idle (shell prompt visible).
is_child_alive() {
  # Check if the pane still exists
  local pane_info
  pane_info=$(cmux list-panels "${WS_FLAG[@]}" 2>/dev/null | grep "$SURFACE_ID" || true)
  if [[ -z "$pane_info" ]]; then
    return 1  # pane gone entirely
  fi

  # Read the pane to look for evidence of an alive claude UI or a returned shell.
  local screen
  screen=$(cmux read-screen --surface "$SURFACE_ID" 2>&1 || true)

  # POSITIVE SIGNALS: if claude's interactive UI is on screen anywhere, it's alive.
  # These markers come from the Claude Code TUI footer / header and are not
  # present in a plain shell. Match before the negative signal to avoid the
  # `❯` false-positive that killed an actively-running iteration before — claude
  # renders `❯` as its own input cursor on a standalone line, which previously
  # matched the same regex used to detect zsh's `❯` prompt character.
  if echo "$screen" | grep -qiE 'claude code v|auto mode on|esc to interrupt|⏵⏵|press ctrl\+c'; then
    return 0  # claude UI visible — alive
  fi

  # NEGATIVE SIGNAL: only treat as dead if the last lines look like a shell
  # prompt AND no claude markers were found above. Tail 3 lines so a transient
  # shell render between launcher exit and bash teardown still trips it.
  local last_lines
  last_lines=$(echo "$screen" | tail -3)
  if echo "$last_lines" | grep -qE '^\s*(\$|%|❯|➜)\s*$'; then
    return 1
  fi

  return 0
}

# Check session usage via /usage dialog on the child pane (must be idle).
# Writes the percentage to USAGE_FILE. Returns 1 if above threshold.
check_session_usage() {
  # Only check if the child session is still alive
  if ! is_child_alive; then
    log_info "iter-${ITER_NUM}: child exited, skipping usage check"
    return 0
  fi

  cmux send --surface "$SURFACE_ID" "/usage" 2>/dev/null || return 0
  cmux send-key --surface "$SURFACE_ID" enter 2>/dev/null || return 0
  sleep 5

  local screen usage_pct
  screen=$(cmux read-screen --surface "$SURFACE_ID" 2>&1)

  # Dismiss the dialog — send escape multiple times to be safe
  cmux send-key --surface "$SURFACE_ID" escape 2>/dev/null || true
  sleep 1
  cmux send-key --surface "$SURFACE_ID" escape 2>/dev/null || true
  sleep 1

  # Extract "Current session" percentage (first "NN% used" line)
  usage_pct=$(echo "$screen" | grep -oE '[0-9]+% used' | head -1 | grep -oE '[0-9]+')

  if [[ -z "$usage_pct" ]]; then
    log_info "iter-${ITER_NUM}: could not read session usage"
    return 0
  fi

  echo "$usage_pct" > "$USAGE_FILE"
  log_info "iter-${ITER_NUM}: session usage at ${usage_pct}%"

  if [[ "$usage_pct" -ge "$USAGE_THRESHOLD" ]]; then
    log_error "iter-${ITER_NUM}: session usage ${usage_pct}% exceeds threshold ${USAGE_THRESHOLD}%"
    echo "USAGE_THROTTLED" >> "$USAGE_FILE"
    return 1
  fi

  return 0
}

# Send /exit to cleanly close the interactive session (single attempt — pane
# is closed later via cmux close-surface regardless, and is_child_alive may
# not detect the shell prompt reliably, causing retries to send /exit to zsh)
exit_child() {
  if is_child_alive; then
    cmux send --surface "$SURFACE_ID" "/exit" 2>/dev/null || true
    cmux send-key --surface "$SURFACE_ID" enter 2>/dev/null || true
    sleep 3
  fi
}

# --- Create a split pane to the right ---

echo "Opening cmux pane for iteration ${ITER_NUM}..."
PANE_OUTPUT=$(cmux new-pane --direction right "${WS_FLAG[@]}" 2>&1)
SURFACE_ID=$(echo "$PANE_OUTPUT" | grep -oE 'surface:[0-9]+' | head -1)

if [[ -z "$SURFACE_ID" ]]; then
  echo "ERROR: Could not capture cmux surface ID" >&2
  echo "Output was: $PANE_OUTPUT" >&2
  exit 1
fi

echo "Launched in cmux pane ${SURFACE_ID}"
log_info "iter-${ITER_NUM}: started"
progress 0.0 "iter-${ITER_NUM}: starting..."

# --- Helper: run an interactive claude phase in the cmux pane and wait for completion ---

run_phase() {
  local phase_name="$1"
  local session_name="$2"
  local prompt="$3"
  local tab_title="$4"

  rm -f "$DONE_FILE"

  # Update tab title and sidebar status
  cmux rename-tab --surface "$SURFACE_ID" "$tab_title" 2>/dev/null || true

  # Write prompt to temp file to avoid quoting issues in cmux send
  local prompt_file="/tmp/ralph-loop-prompt-${ITER_NUM}-${phase_name}.txt"
  echo "$prompt" > "$prompt_file"

  # Launch interactive claude with the prompt read from file.
  #
  # We use `cmux respawn-pane` (tmux-compat) rather than `cmux send` + `send-key
  # enter`, because `cmux send` only delivers keystrokes once the workspace has
  # been focused at least once — that left iterations idling until the user
  # visited the workspace.
  #
  # `cmux respawn-pane --command <X>` exec()s X directly without a shell, so
  # `$(cat ...)` substitution inside <X> is NOT performed. We therefore write
  # a tiny launcher script and pass its path as the command — the script's
  # shebang gives us shell evaluation for the prompt-file substitution.
  local launcher="/tmp/ralph-loop-launch-${ITER_NUM}-${phase_name}.sh"
  # Capture Claude's process exit code as a fallback sentinel. The prompt asks
  # the agent to write 0/1 to DONE_FILE explicitly, but agents sometimes exit
  # cleanly after finishing real work (incl. /create-pr) without running that
  # final echo. Previously that stranded the iteration as "failed" even though
  # Phase 1 had completed. Now: if the agent wrote DONE_FILE, we honor it;
  # otherwise we record claude-teams' exit code so the poller treats a clean
  # session-exit as success and proceeds to Phase 2.
  cat > "$launcher" <<LAUNCHER_EOF
#!/bin/bash
cmux claude-teams --permission-mode auto --name '${session_name}' "\$(cat '${prompt_file}')"
claude_exit=\$?
if [[ ! -f '${DONE_FILE}' ]]; then
  echo "\$claude_exit" > '${DONE_FILE}'
fi
LAUNCHER_EOF
  chmod +x "$launcher"
  cmux respawn-pane --surface "$SURFACE_ID" --command "$launcher"

  # Poll for done file with timeout and dead-process detection
  local elapsed=0
  local dead_checks=0
  while true; do
    sleep "$POLL_INTERVAL"
    elapsed=$((elapsed + POLL_INTERVAL))

    # Check if done file appeared
    if [[ -f "$DONE_FILE" ]]; then
      break
    fi

    # Check if child process died without writing done file
    if ! is_child_alive; then
      dead_checks=$((dead_checks + 1))
      # Wait a few cycles to be sure it's really dead (not just a slow render)
      if [[ $dead_checks -ge 3 ]]; then
        log_error "iter-${ITER_NUM}: child exited without writing done file"
        echo ""
        echo "ERROR: Child claude exited without signaling completion." >&2
        echo "The done file was never written. This usually means context was exhausted" >&2
        echo "or the child crashed before running the echo command." >&2
        # Write failure marker so we don't loop forever
        echo "1" > "$DONE_FILE"
        break
      fi
    else
      dead_checks=0
    fi

    # Soft timeout: extend the budget if claude is still working.
    #
    # Previously this branch wrote "1" to DONE_FILE unconditionally at
    # PHASE_TIMEOUT, racing with the launcher's exit-code capture. iter-92's
    # first attempt hit this: review phase was making progress past 7200s,
    # the watcher wrote "1", and the launcher rewrote "0" microseconds later
    # — but by then the script had already exited 1 and the trap wrote
    # iter-N-failed. The dead-process branch above (3 consecutive
    # is_child_alive=false) already catches truly stuck panes; this branch
    # exists only to bound a pane that is alive-but-stalled (UI rendered,
    # no progress). When still alive, just warn and reset the clock.
    if [[ $elapsed -ge $PHASE_TIMEOUT ]]; then
      if is_child_alive; then
        echo ""
        echo "WARN: phase ${phase_name} past ${PHASE_TIMEOUT}s budget but child still alive — extending another ${PHASE_TIMEOUT}s" >&2
        log_info "iter-${ITER_NUM}: phase ${phase_name} extended past ${PHASE_TIMEOUT}s (child alive)"
        elapsed=0
      else
        log_error "iter-${ITER_NUM}: phase ${phase_name} timed out after ${PHASE_TIMEOUT}s (child not alive)"
        echo ""
        echo "ERROR: Phase ${phase_name} timed out after ${PHASE_TIMEOUT}s." >&2
        echo "1" > "$DONE_FILE"
        break
      fi
    fi

    printf "."
  done
  echo ""

  local exit_code
  exit_code=$(cat "$DONE_FILE")
  rm -f "$DONE_FILE" "$prompt_file" "$launcher"

  # Check session usage while child is idle (before /exit)
  if ! check_session_usage; then
    exit_child
    echo "THROTTLED: session usage above ${USAGE_THRESHOLD}%. Pause and retry later."
    # Exit code 2 = throttled (distinct from 1 = failure)
    exit 2
  fi

  # Send /exit to cleanly close the interactive session
  exit_child

  return "${exit_code:-1}"
}

# --- Phase 1: Implement (skip if "review" arg passed) ---

if [[ "$SKIP_TO_REVIEW" != "review" ]]; then
  status "Implementing..." gear
  progress 0.1 "iter-${ITER_NUM}: implementing..."
  log_info "iter-${ITER_NUM}: implementation started"
  echo "Phase 1: Implementing iteration ${ITER_NUM}..."

  if ! run_phase "implement" "iter-${ITER_NUM}-implement" "$PROMPT_IMPLEMENT" "iter-${ITER_NUM}: implement"; then
    status "FAILED" warning "#ff0000"
    progress 0.0 "iter-${ITER_NUM}: failed"
    log_error "iter-${ITER_NUM}: implementation FAILED"
    flash
    echo "Iteration ${ITER_NUM} FAILED during implementation. Pane left open for inspection." >&2
    exit 1
  fi

  log_success "iter-${ITER_NUM}: implementation complete"
  echo "Phase 1 complete. Starting Phase 2..."
fi

# --- Iteration discipline checks (pre-Phase 2) ---
#
# Run in-repo xtask checks before opening the PR. These are the same checks CI
# runs in the `discipline` job. Running them here surfaces failures before the
# PR is created, giving the implementing agent a chance to fix them in Phase 2.
#
# If `cargo run -p xtask` fails (xtask not yet in the repo), log a warning and
# continue — the CI job is the load-bearing gate.
#
# claims-vs-code.sh and ac-fidelity-check.sh (added in iter-61z) run alongside
# the xtask checks. claims-vs-code is advisory (its output goes to the cmux
# log so Phase 2 can attach it to the PR body); ac-fidelity is a hard gate
# that blocks the merge if a ticked AC has no evidence.
check_iteration_discipline() {
  local repo_root="$1"
  local script_dir
  script_dir="$(cd "$(dirname "$0")" && pwd)"

  # Check if xtask is available in this repo.
  if ! (cd "$repo_root" && cargo run -p xtask -- --help > /dev/null 2>&1); then
    log_info "iter-${ITER_NUM}: xtask not found — skipping discipline checks (add crates/xtask to enable)"
    return 0
  fi

  log_info "iter-${ITER_NUM}: running discipline checks..."

  local failed=0

  if ! (cd "$repo_root" && cargo run -p xtask -- check-dead-primitives --since origin/main 2>&1); then
    log_error "iter-${ITER_NUM}: check-dead-primitives FAILED — unwired pub items detected"
    failed=1
  else
    log_info "iter-${ITER_NUM}: check-dead-primitives OK"
  fi

  if ! (cd "$repo_root" && cargo run -p xtask -- check-todo-annotations --since origin/main 2>&1); then
    log_error "iter-${ITER_NUM}: check-todo-annotations FAILED — unannotated TODOs detected"
    failed=1
  else
    log_info "iter-${ITER_NUM}: check-todo-annotations OK"
  fi

  # Claims-vs-code (advisory): emit a markdown section. Phase 2 attaches it to
  # the PR body. Non-zero exit becomes a WARN line rather than a hard fail so
  # iterations whose commit messages don't fit the heuristic don't block.
  local claims_out="${RALPH_CACHE_DIR:-$repo_root/.ralph-cache}/iter-${ITER_NUM}-claims.md"
  mkdir -p "$(dirname "$claims_out")" 2>/dev/null || true
  if (cd "$repo_root" && "$script_dir/claims-vs-code.sh" --branch "${BRANCH_NAME:-HEAD}" --base main > "$claims_out" 2>&1); then
    log_info "iter-${ITER_NUM}: claims-vs-code OK (report at $claims_out)"
  else
    log_info "iter-${ITER_NUM}: claims-vs-code WARN — unmatched claim(s); see $claims_out"
  fi

  # AC fidelity (hard gate): every ticked AC must be backed by diff evidence
  # or marked `[deferred — new plan: <path>]`.
  if [[ -f "$PLAN_PATH" ]]; then
    if ! (cd "$repo_root" && "$script_dir/ac-fidelity-check.sh" --plan "$PLAN_PATH" --branch "${BRANCH_NAME:-HEAD}" --base main 2>&1); then
      log_error "iter-${ITER_NUM}: ac-fidelity-check FAILED — ticked AC lacks evidence in diff"
      failed=1
    else
      log_info "iter-${ITER_NUM}: ac-fidelity-check OK"
    fi
  fi

  if [[ $failed -ne 0 ]]; then
    log_error "iter-${ITER_NUM}: discipline checks FAILED — fix before creating PR"
    return 1
  fi

  log_info "iter-${ITER_NUM}: discipline checks passed"
  return 0
}

# Determine the repo root (parent of this script's directory hierarchy, or
# use the working directory if git rev-parse fails).
REPO_ROOT=$(git -C "$(dirname "$0")" rev-parse --show-toplevel 2>/dev/null || pwd)

DISCIPLINE_LOG="${RALPH_CACHE_DIR:-$REPO_ROOT/.ralph-cache}/iter-${ITER_NUM}-discipline.log"
mkdir -p "$(dirname "$DISCIPLINE_LOG")" 2>/dev/null || true
if ! check_iteration_discipline "$REPO_ROOT" 2>&1 | tee "$DISCIPLINE_LOG"; then
  # Discipline failures are NOT fatal here — the implementing agent is still
  # alive in the cmux pane and Phase 2 reads $DISCIPLINE_LOG to fix each
  # issue before /merge-pr. Exiting here (the prior behavior) stranded the
  # iteration: the pane stayed open, /create-pr had already run in Phase 1,
  # and the loop's manual recovery path was needed every time.
  log_error "iter-${ITER_NUM}: discipline checks failed — Phase 2 will address them (details in $DISCIPLINE_LOG)"
  progress 0.5 "iter-${ITER_NUM}: discipline failed, repairing in Phase 2"
else
  # Successful run still leaves the log around so Phase 2 sees "OK" lines if it
  # peeks; keeps the contract uniform.
  log_info "iter-${ITER_NUM}: discipline checks passed (log at $DISCIPLINE_LOG)"
fi

# --- Phase 2: Create PR, review, merge ---

status "Review & Merge" merge
progress 0.5 "iter-${ITER_NUM}: review & merge..."
log_info "iter-${ITER_NUM}: review phase started"

if ! run_phase "review" "iter-${ITER_NUM}-review" "$PROMPT_REVIEW" "iter-${ITER_NUM}: review & merge"; then
  status "FAILED" warning "#ff0000"
  progress 0.5 "iter-${ITER_NUM}: failed"
  log_error "iter-${ITER_NUM}: review phase FAILED"
  flash
  echo "Iteration ${ITER_NUM} FAILED during review phase. Pane left open for inspection." >&2
  exit 1
fi

# --- Success ---

status "Done" checkmark "#00ff00"
progress 1.0 "iter-${ITER_NUM}: complete"
log_success "iter-${ITER_NUM}: merged successfully"
flash

echo "Iteration ${ITER_NUM} completed successfully."

# Tear down the cmux pane and clear visual status. Run synchronously so the
# work is guaranteed to happen before this script exits — the previous
# `( ... ) & disown` form was unreliable: the disowned subshell could be
# killed (or never get scheduled) before close-surface ran, leaving panes
# leaked across multiple iterations.
#
# Trap HUP to no-op so a closed pane can't terminate the script before it
# finishes the cleanup.
trap '' HUP
cmux close-surface --surface "$SURFACE_ID" "${WS_FLAG[@]}" 2>/dev/null || true
cmux clear-progress "${WS_FLAG[@]}" 2>/dev/null || true
cmux clear-status "$STATUS_KEY" "${WS_FLAG[@]}" 2>/dev/null || true

exit 0
