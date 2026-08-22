---
title: "Iteration 185: main is red under clippy 1.98 — three one-line lints, none touched by iter-179"
type: iteration
date: 2026-08-22
status: planned
branch: iter-185/main-red-under-clippy-1-98
depends_on: []
first_call_sites: []
dogfood_path: |
  # Lint-only fix, no product behavior changes. Verification is the gate
  # itself, run on the exact toolchain CI uses.
  rustup update stable
  cargo clippy --version   # confirm 1.98.x, matching CI at the time this was filed
  cargo clippy --workspace --all-targets -- -D warnings
  # Expect exit 0. Then confirm MSRV is unaffected:
  cargo +1.95 build --workspace 2>&1 | tail -5   # or: cargo msrv verify, if configured
tags: [iteration, tooling, clippy, ci, maintenance]
---

# Iteration 185: main is red under clippy 1.98

## Why this exists

[[iteration-179-live-62-runner-sees-no-network-events]]'s closing CI run hit `clippy` failing on
PR #212 — a branch that never touched the three offending lines. Provenance was checked with
`git log -S`/`git blame` against `main`, not assumed:

- `crates/ff-rdp-cli/tests/common/mod.rs`: `buf.chunks_exact(3)` (clippy: `chunks_exact_to_as_chunks`,
  suggests `as_chunks::<3>().0`) — introduced by `49a3d88` (2026-08-12), well before iter-179's
  `baa861c` branch point.
- `crates/ff-rdp-cli/build.rs`: `.output().ok().is_some_and(...)` in `git_is_dirty` (clippy:
  `manual_is_variant_and`, suggests `.output().is_ok_and(...)`).
- `crates/ff-rdp-cli/src/commands/profiles.rs:228`: the same `.ok().is_some_and(...)` shape in the
  stale-profile-pruning closure.

All three are pre-existing on `main`. iter-179 fixed all three **on its own branch** (commit
`4d9e83c`, confirmed clean against clippy 1.98 with `rustup update stable` after CI failed with a
skewed local toolchain at 1.97), because it needed a green `clippy` check to merge — but that fix
does not reach `main` until #212 merges, and does nothing for any other open branch in the
meantime. Every PR based on an older `main` will trip the same three lints until `main` itself is
fixed directly.

**Lesson this iteration is also filed to record**: a local `cargo clippy` pass is not evidence CI's
clippy passes. CLAUDE.md's three gates (`cargo fmt`, `cargo clippy`, `cargo test`) are written as
though local green implies CI green; a toolchain skew (stable moved 1.97 → 1.98 between when this
machine last updated and when CI ran) breaks that silently, with no local signal.

## The durable item is the detection gap, not the three lints

Stated plainly, because it is easy to file this as "fix three lints" and miss the point: by the
time anyone reads this plan the three lints are already fixed (on iter-179's branch, and on `main`
once #212 merges). **The item worth an iteration is that clippy in CI tracks stable, so a
toolchain release can red-line `main` with no code change at all, and nothing in this repo notices
until the next PR happens to run.**

The timeline is the argument. Stable released 1.98.0 on 2026-08-18. No PR ran CI between then and
2026-08-22. `main` was red that entire window and nobody could have known. The first PR to run
(#212, an `assert_network` iteration with no relationship to any of it) absorbed the whole cost:
one failed CI run, and an evening's diagnosis that concluded the branch was innocent.

Compounding it: the local gate **disagreed with CI on identical code**. `cargo clippy --workspace
--all-targets -- -D warnings` exited 0 on the contributor machine (1.97) while CI failed (1.98).
CLAUDE.md presents its three gates as though local green implies CI green. That is not true across
a toolchain boundary, and there is currently no local signal that a boundary was crossed.

## Scope

Land the same three one-line fixes directly on `main`, verified against clippy 1.98 (or whatever
CI is running at merge time — re-check, do not assume 1.98 is still current):

1. `tests/common/mod.rs`: `buf.chunks_exact(3)` → `buf.as_chunks::<3>().0`
2. `build.rs`: `.output().ok().is_some_and(...)` → `.output().is_ok_and(...)`
3. `commands/profiles.rs`: same `.ok().is_some_and(...)` → `.is_ok_and(...)`

Both replacements are clippy's own suggested fix, applied verbatim — no behavior change, only the
`Result`/`Option` conversion is elided. MSRV is 1.95 (`Cargo.toml`); `as_chunks` stabilized in
1.88, `is_ok_and` in 1.70, so neither replacement needs an MSRV bump. Confirm with the dogfood step
above rather than trusting this note by the time this plan runs.

## Tasks

### A. Land the fix [0/2]
- [ ] All three sites fixed on `main`, `cargo clippy --workspace --all-targets -- -D warnings`
      exits 0 on the toolchain CI currently runs
- [ ] MSRV gate (`msrv` CI job, or local equivalent) still passes after the change

### B. Close the detection gap — the actual point of this iteration [0/3]
- [ ] Decide whether `rust-toolchain.toml` (pinning the exact stable version this repo builds
      with) is the right answer, and either add one or record explicitly why not. A pin trades
      "always current" for "always reproducible locally" — a real tradeoff, not a free fix. It
      would have made local and CI agree, which is the failure that cost the most time here
- [ ] Decide whether CI should run clippy on a **scheduled** job (e.g. weekly) as well as per-PR,
      so a toolchain release that red-lines `main` is discovered by the schedule rather than by
      whichever unlucky PR runs first. Note the cost honestly: a cron job that fails on green
      code is its own kind of noise
- [ ] If CLAUDE.md's "run these three in order" section still implies local green means CI green,
      correct it — one sentence noting the toolchain boundary is enough

## Acceptance Criteria [0/2]

- [ ] `cargo clippy --workspace --all-targets -- -D warnings` exits 0 on `main` at HEAD, on the
      toolchain version CI is running at merge time
- [ ] Each of Task B's three decisions is recorded with its reasoning, whichever way it went — the
      plan is not done because the lints are green; it is done when the *next* toolchain bump has a
      named path to being noticed by something other than an unrelated PR

## Out of scope

- Auditing the rest of the workspace for other latent toolchain-skew lints beyond these three —
  the closing `cargo clippy` run in task A either finds more or it doesn't; this plan does not
  presuppose there are others.
- Any behavior change beyond the three mechanical rewrites above.

## References

- [[iteration-179-live-62-runner-sees-no-network-events]] — where this was found, in PR #212's
  closing CI run
- PR #212 — carry-over row 13 dispositions this to this plan
