---
title: check-pre-fix-repro alternates PASS/FAIL — worktree reuse check is wrong for linked worktrees
type: reference
date: 2026-07-03
tags: [backlog, issue, xtask, pre-fix-repro, worktree]
status: fixed
fixed_in: iter-96/profile-leak-cleanup
---

# check-pre-fix-repro worktree flap

`cargo run -p xtask -- check-pre-fix-repro` (and therefore
`check-iteration-ready`) alternates between PASS and FAIL on successive
runs. Observed 2026-07-03 during iter-96: run 1 FAIL, prune, run 2 PASS,
run 3 FAIL with:

```
fatal: '/Users/james/.cache/ff-rdp/pre-fix-repro/main-tree' is a missing
but already registered worktree; use 'add -f' to override, or 'prune' or
'remove' to clear
```

## Root cause

`ensure_main_worktree` in `crates/xtask/src/check_pre_fix_repro.rs`
(~lines 207-245) decides whether the cached worktree belongs to the
current repo by comparing `git rev-parse --show-toplevel` inside the
worktree against the current repo root. **In a linked worktree,
`--show-toplevel` always reports the worktree's own path**
(`…/pre-fix-repro/main-tree`), never the parent checkout
(`…/devel/ff-rdp`), so `roots_match` is always false. Every run after a
successful creation:

1. takes the "stale" branch and `std::fs::remove_dir_all`s the tree,
2. never deregisters it (`git worktree remove`/`prune` is not called),
3. then `git worktree add` fails on the leftover registration in
   `.git/worktrees/main-tree`.

## Fix sketch

- Compare `git -C <worktree> rev-parse --git-common-dir` (resolved to an
  absolute path) against the current repo's `.git` dir instead of
  `--show-toplevel`.
- On the recreate path, run `git worktree remove --force <path>` or
  `git worktree prune` before `git worktree add`.
- Regression test: call `ensure_main_worktree` twice; the second call
  must reuse (not recreate) the worktree.

## Second bug found during the same investigation

`refresh_worktree` ran `git -C <worktree> fetch origin --depth=1`. A
linked worktree shares the parent repo's object store and `.git/shallow`
file, so this **re-shallowed the entire repository** on every gate run,
which broke `check-discipline-regression` (its iter-61t replay needs the
deep merge history of `main`). The two sub-checks could never pass in
the same run.

## Resolution

Fixed on `iter-96/profile-leak-cleanup` in
`crates/xtask/src/check_pre_fix_repro.rs`: reuse check now compares
`git rev-parse --path-format=absolute --git-common-dir`, a
`git worktree prune` runs before `git worktree add`, and the refresh
fetch is no longer depth-limited. Verified by two consecutive green
`check-iteration-ready` runs (second run exercises the reuse path).
If the repo was already shallowed by an earlier run, restore once with
`git fetch --unshallow origin`.
