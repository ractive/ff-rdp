---
title: "Iteration 185: main is red under clippy 1.98 — three one-line lints, none touched by iter-179"
type: iteration
date: 2026-08-22
status: in-review
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

## What was found when this ran (2026-08-23)

**Task A was already done by the time this iteration started, and this branch changes no Rust at
all.** PR #212 merged as `a294724` before iter-185 ran, so iter-179's three one-line fixes reached
`main` with it. Verified by inspection at `main` HEAD, not assumed:

| Site | State on `main` |
|---|---|
| `crates/ff-rdp-cli/tests/common/mod.rs:1325` | `for chunk in buf.as_chunks::<3>().0` |
| `crates/ff-rdp-cli/build.rs:88` | `.is_ok_and(\|out\| out.status.success() && !out.stdout.is_empty())` |
| `crates/ff-rdp-cli/src/commands/profiles.rs:228` | `now.duration_since(newest).is_ok_and(\|age\| age >= threshold)` |

The plan's own instruction — re-check the toolchain rather than assume 1.98 is still current — was
followed: local stable is now `rustc 1.98.0 (88d9e12ae 2026-08-18)` / `clippy 0.1.98`, the same
release CI's `dtolnay/rust-toolchain@…  # stable` resolves to. `cargo clippy --workspace
--all-targets -- -D warnings` exits 0 on that toolchain, and the workspace scan turned up **no**
further latent 1.98 lints beyond the three (the "Out of scope" note left this open either way).

So the whole deliverable is Task B, exactly as the plan predicted. The decisions and their
reasoning are in [[decision-log]] DEC-044; summarised:

1. **No `rust-toolchain.toml`** — rejected on two grounds. It detects nothing (it converts "nobody
   noticed `main` went red" into "nobody noticed the pin went stale", deferring the whole lint
   delta onto whoever bumps it), and it would silently defeat the `msrv` job:
   `dtolnay/rust-toolchain` activates via `rustup default`, and a repo-root `rust-toolchain.toml`
   overrides `rustup default` — verified locally, a file pinning 1.97.1 wins over a `stable`
   (1.98.0) default, and only `+toolchain`/`RUSTUP_TOOLCHAIN` beats the file. `msrv` runs a bare
   `cargo build` after `toolchain: "1.95"`, so it would compile on the pin and stop being a gate.
2. **Yes to a scheduled lint of `main`** — `.github/workflows/toolchain-watch.yml`, `fmt` +
   `clippy` weekly (Mondays 04:00 UTC, an hour after `live.yml`'s canary) plus
   `workflow_dispatch`. It is the only trigger that fires when the input that changed was the
   toolchain and not the code; even `push: [main]` would not have caught this, because the
   breaking event had zero commits. Cost recorded rather than glossed: it can go red on untouched
   source, which is noise — accepted because the red is genuine, lands where one commit fixes it,
   and blocks nothing.
3. **`CLAUDE.md` and `CONTRIBUTING.md` corrected** — both stated the three gates as though local
   green implied CI green. Both now name the toolchain boundary, tell you to `rustup update
   stable` before quoting a clippy result, and point at `gh pr checks` as the authority.

## How this was verified, and what was deliberately not run

- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace -q` — all exit 0 on `rustc 1.98.0 (88d9e12ae 2026-08-18)`.
- `actionlint` — exit 0 on `.github/workflows/toolchain-watch.yml` and on the workflow set as a
  whole. This is as far as the canary can be validated before it merges: **scheduled workflows
  only run from the default branch**, so the cron cannot fire from this branch. Syntax, action
  refs and expressions are checked; the trigger itself is not, and that gap is a carry-over row.
- All six xtask `check-*` gates exit 0 (`check-dogfood-script` needs `FF_RDP_LIVE_TESTS=1` even
  when the plan has no `dogfood_script`, else it fails closed on an `iter-*` branch; with the gate
  set it reports SKIP).
- **No live sweep was run, on purpose.** The diff is one workflow file and five markdown/doc files
  (`CLAUDE.md`, `CONTRIBUTING.md`, `README.md`, `kb/decision-log.md`, this plan) —
  no `.rs`, no `Cargo.toml`, no product behaviour. `iteration-close`'s standing sweep policy is
  scoped to iterations touching product source, and a 40-minute sweep here could only report on
  code identical to `main`'s while risking contention with other Firefox work in flight. Stated
  rather than silently skipped, and carried as a row.
- The `dogfood_path`'s `cargo +1.95 build` step was not run locally (the 1.95 toolchain is not
  installed on this machine). Since the diff contains no Rust and no manifest change, the MSRV
  surface is byte-identical to `main`; the `msrv` CI job on this PR is the evidence quoted.

## Tasks

### A. Land the fix [2/2]
- [x] All three sites fixed on `main`, `cargo clippy --workspace --all-targets -- -D warnings`
      exits 0 on the toolchain CI currently runs — arrived via the #212 merge (`a294724`) before
      this branch existed; re-verified at `main` HEAD on clippy 0.1.98, exit 0. **This branch
      contains no Rust change**, which is the honest outcome, not a miss
- [x] MSRV gate (`msrv` CI job, or local equivalent) still passes after the change — the diff
      touches no `.rs` and no `Cargo.toml`, so the MSRV surface is untouched; confirmed by the
      `msrv` job on this PR rather than by a local 1.95 install

### B. Close the detection gap — the actual point of this iteration [3/3]
- [x] Decide whether `rust-toolchain.toml` … is the right answer, and either add one or record
      explicitly why not — **decided: no**, recorded in DEC-044 with the `msrv`-job collision
      verified empirically rather than asserted
- [x] Decide whether CI should run clippy on a **scheduled** job … Note the cost honestly —
      **decided: yes**, `.github/workflows/toolchain-watch.yml`; the cost paragraph is in the
      workflow header and in DEC-044, in both cases stating that a cron failing on unchanged code
      is real noise that is being accepted, not denied
- [x] If CLAUDE.md's "run these three in order" section still implies local green means CI green,
      correct it — it did; corrected in `CLAUDE.md` and, at more length, in `CONTRIBUTING.md`

## Acceptance Criteria [2/2]

- [x] `cargo clippy --workspace --all-targets -- -D warnings` exits 0 on `main` at HEAD, on the
      toolchain version CI is running at merge time — exit 0 locally on `rustc 1.98.0
      (88d9e12ae 2026-08-18)`, and green in the PR's own `clippy` job, which is the same
      `stable` CI resolves. `gh pr checks` is the evidence quoted, not the local run
- [x] Each of Task B's three decisions is recorded with its reasoning, whichever way it went —
      DEC-044, one entry covering all three, including the two things deliberately **not** done
      (no `rust-toolchain.toml`; no issue-opening/alerting bot on canary failure) and the known
      erosion path (GitHub disables cron in repos idle 60 days, noted in the workflow header)

## Out of scope

- Auditing the rest of the workspace for other latent toolchain-skew lints beyond these three —
  the closing `cargo clippy` run in task A either finds more or it doesn't; this plan does not
  presuppose there are others.
- Any behavior change beyond the three mechanical rewrites above.

## References

- [[iteration-179-live-62-runner-sees-no-network-events]] — where this was found, in PR #212's
  closing CI run
- PR #212 — carry-over row 13 dispositions this to this plan
- [[decision-log]] — DEC-044, which this iteration adds: no toolchain pin, weekly canary instead
- `.github/workflows/toolchain-watch.yml` — the canary itself; its header carries the incident
  timeline and the re-enable instructions for when GitHub disables the cron
