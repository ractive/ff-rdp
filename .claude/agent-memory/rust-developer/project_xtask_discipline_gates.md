---
name: project-xtask-discipline-gates
description: xtask discipline gate commands, check-iteration-ready aggregator (iter-75b), and check-pre-fix-repro persistent worktree + SHA cache (iter-91)
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

## check-pre-fix-repro (iter-91)

Rewrote `check_pre_fix_repro.rs` to use a persistent main-side git worktree (`~/.cache/ff-rdp/pre-fix-repro/main-tree`) and a SHA-keyed result cache instead of `git stash` / `git checkout`. Key design decisions:

- `RunConfig` struct injected into `run_with_writer()` — avoids env var races in parallel tests
- `resolve_main_worktree_path()` is pure (OnceLock); `ensure_main_worktree()` is side-effecting
- Cache files at `results/<sha>-<crate>-<slug>` contain `PASS\n` or `FAIL\n` + ISO-8601 timestamp
- Only the red-on-main probe runs; there is no second "green on branch" run
- Env var overrides (`FF_RDP_PRE_FIX_REPRO_CACHE_DIR`, `FF_RDP_PRE_FIX_REPRO_SHA_OVERRIDE`) used by dogfood script; `RunConfig` used by unit tests
- `std::env::set_var` is unsafe in Rust 2024 edition — wrap in `unsafe {}` with SAFETY comment when needed in tests
