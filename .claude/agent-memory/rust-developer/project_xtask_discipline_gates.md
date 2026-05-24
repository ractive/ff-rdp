---
name: project-xtask-discipline-gates
description: xtask discipline gate commands and the check-iteration-ready aggregator added in iter-75b
metadata:
  type: project
---

# xtask discipline gate aggregator (iter-75b)

Added `check-iteration-ready` and `find-iteration-plan` xtask subcommands in iter-75b.

**check-iteration-ready** (`crates/xtask/src/check_iteration_ready.rs`): runs all 6 discipline gates in sequence, collects failures, reports all before exiting. Use with `--plan <path> --base origin/main`.

**find-iteration-plan** (`crates/xtask/src/find_iteration_plan.rs`): resolves an `iter-N/slug` branch name to the absolute path of its kb plan file.

**Why:** Subprocess invocations (not direct function calls) are used to capture each sub-check's stdout/stderr cleanly. Direct calls would require refactoring all sub-checks to accept `&mut dyn Write`.

**Where used:**
- `~/.claude/skills/create-pr/SKILL.md` — pre-PR discipline gate step
- `tools/ralph-loop/scripts/run-iteration.sh` — Phase 1 prompt step 6
- `CLAUDE.md` — "Iteration discipline" section
- `CONTRIBUTING.md` — "One-shot pre-PR discipline gate" section

**Watch out for:** If two plans share the same iteration ID (e.g. `iteration-75b-*.md`), `find-iteration-plan` correctly errors with "multiple plans found". This is the expected behavior — disambiguate by removing duplicates.
