---
branch: iter-151/residual-live-firefox-leak
date: 2026-08-12
depends_on:
  - kb/iterations/iteration-146-live-suite-reliability.md
dogfood_path: |
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live -- --include-ignored --test-threads=1 live_1
  ff-rdp profiles list --jq '.results.count'
  # → after the chunk exits, `count` must be 0 (see the Notes entry on why this beats a
  #   `pgrep -af start-debugger-server` scan in this shared sandbox). Prefer this over the
  #   raw pgrep count used to first find this leak — see Resolution for the confirmed fix.
first_call_sites: []
status: in-progress
title: "Iteration 151: a residual live-suite Firefox leak survives iteration-146"
type: iteration
tags:
  - iteration
---

# Iteration 151: a residual live-suite Firefox leak survives iteration-146

Follow-up to [[iteration-146-live-suite-reliability]]. Found during the post-146 verification
sweep on main (2026-08-12) — the sweep that was supposed to confirm 146 fixed the leak.

## What 146 fixed, and what it didn't

[[iteration-146-live-suite-reliability]] Theme A correctly identified and fixed one leak source:
`live_96_profile_cleanup.rs`'s `launch_headless()` spawned Firefox through a bare `Command` with
no RAII guard. That fix is real — `live_146_no_orphan_firefox_after_suite` and
`live_146_harness_teardown_kills_daemon_spawned_firefox` both pass, and a targeted run of
`live_137_daemon_mode_parity` + `live_146_suite_reliability` + `live_96_profile_cleanup` (11
tests) adds **zero** orphans.

But a full-suite run still leaks. Measured on main at `4e5dfcc`, run in two chunks:

| chunk | filter | result | leaked |
|---|---|---|---|
| A | `live_1` | 101 passed, 1 failed (known `live_109` red) | 1 (pid 28112) |
| B | `--skip live_1` | 112 passed, 1 failed (see below) | 1 (pid 50930) |

Both survivors were real headless Firefox processes holding
`ff-rdp-profile-*` directories:

```
28112  firefox -no-remote --start-debugger-server 61200 --headless --profile .../ff-rdp-profile-0IbV2t5AElgSigts
50930  firefox -no-remote --start-debugger-server 64734 --headless --profile .../ff-rdp-profile-5sWrtjV2LZzMbEaT
```

Roughly **one leak per ~100 live tests**, in both chunks — so the source is not confined to one
suite, or there is more than one.

## The good news: 146's Theme B caught it

Chunk B's only failure was `live_96_profile_cleanup::live_profiles_prune_removes_all_when_no_
firefox_running`, and it failed *correctly*, with the diagnostic 146 Theme B introduced:

```
precondition violated — 2 ff-rdp-managed profile dir(s) ... still owned by a live process,
so `prune --all` would rip a profile out from under it:
  .../ff-rdp-profile-5sWrtjV2LZzMbEaT (pid 50930), .../ff-rdp-profile-0IbV2t5AElgSigts (pid 28112).
Rerun once these have exited (or in an isolated environment).
```

The pre-146 version of this test reported `left: 1 / right: 0` and taught the reader nothing.
This one named the dirs, the PIDs and the remedy — which is how the residual leak was found at
all. Theme B did its job; this plan is the work it surfaced.

## Themes

### Theme A — name the leaking test

Today a leaked profile is anonymous: the directory carries `.ff-rdp-owner-pid` but nothing
identifying which test spawned it, so the culprit cannot be found from the artifact after the
run. Record the spawning test's name alongside the owner PID (a sibling marker file, or an extra
field wherever the owner PID is written), so a leak is self-identifying.

Do this **first**. It converts every future occurrence — including this one — from a hunt into a
lookup, and it is the cheapest possible fix for the class.

### Theme B — find and fix the remaining source(s)

With Theme A in place, re-run both chunks and read the leaked profile's marker. Candidate
shapes, none confirmed:

- a test that spawns a daemon and asserts on failure before its guard is constructed, so nothing
  owns the process at the moment the assertion unwinds
- a test using its own bespoke spawn helper, as `live_96` did — audit for any remaining
  `Command::new(ff_rdp_bin()).args(["launch"` outside `LiveFirefox`
- a daemon deliberately outliving its CLI invocation whose `daemon stop` is best-effort and
  silently fails

Note the six duplicated `firefox_with_daemon(test: &str) -> Option<LiveFirefox>` wrappers that
[[iteration-146-live-suite-reliability]]'s scope-check flagged (files 137, 138, 139, 140, 141,
145) — if one of them diverges from the others, that is a likely home for the bug and a reason
to finally consolidate them into `tests/common/mod.rs`.

### Theme C — make the whole-suite guarantee testable in chunks

`live_146_no_orphan_firefox_after_suite` passes today because it observes only its own
neighbourhood; the leak appears at full-suite scale. Whatever assertion replaces it must be able
to fail on the evidence above. A post-run check that counts `start-debugger-server` processes
attributable to this suite (via Theme A's marker) is the shape to aim for.

## Resolution (confirmed live, not a hypothesis — see Run guidance rule 1)

Two real, still-open instances of the exact "no RAII guard across an assertion" bug class
[[iteration-146-live-suite-reliability]] Theme A fixed in `live_96_profile_cleanup.rs`'s
`launch_headless` — 146's own fix was correct but scoped to one file, and these were never
migrated:

1. **`live_90_daemon_lifecycle.rs` (×2 tests), `live_daemon_stop_mdn.rs`, and
   `live_142_daemon_stop_pid_honesty.rs`** each wrapped their `LiveFirefox` guard in
   `std::mem::ManuallyDrop` immediately after spawning, so `daemon stop` (or `launch --replace`)
   alone was responsible for killing Firefox. But every assertion between that point and the
   final liveness/port-free check ran with **no** guard at all — a failure in any of them (a
   non-zero `daemon stop`, a slow port release under contention, a pid-honesty mismatch) panics
   with Firefox still alive and nothing left to reap it. Fix: remove the `ManuallyDrop`
   suppression in all four call sites — the guard now stays a normal binding for the rest of each
   function. `daemon stop`'s own cleanup is still asserted (the belt); the guard's `Drop` at
   function end — a no-op once Firefox is already dead on the happy path — is the suspenders.
   **Proven live**, not by inspection alone: `live_151_root_cause_documented` drives both the
   pre-fix (`ManuallyDrop`) and fixed (normal binding) shapes against a real Firefox process
   inside an identical `catch_unwind`'d panic and asserts on actual PID liveness afterward. Run
   2026-08-12: pre-fix shape leaked pid 98074 (confirmed still alive 300ms after the panic,
   manually reaped); fixed shape reaped pid 98140 through the same panic automatically. PASS.
2. **`live_142_disk_growth.rs`'s `launch_headless`** launched Firefox via a bare `Command` with
   no guard whatsoever and relied entirely on a *manual*, later `kill_pid` call — this is the
   exact pre-146 `live_96_profile_cleanup.rs` shape (see 146's own postmortem in this file's "What
   146 fixed, and what it didn't" section above), just never migrated when 146 fixed that
   specific file. `live_142_profile_growth_bounded` had an `assert!` between spawn and its
   deliberate force-kill; `live_142_throttle_json_gc` had two `assert!`s between spawn and its
   manual `kill_pid` cleanup at the very end. Fix: added `FirefoxGuard`, a small local RAII
   wrapper constructed immediately once each PID is known (this file's launches need a custom
   `FF_RDP_HOME` env var that `common::LiveFirefox` doesn't expose, so it can't reuse that type
   outright).

Why 146's Theme A fix didn't cover either: 146 scoped its search (correctly, for its own
symptom) to the file the orphan-sweep test caught leaking at the time —
`live_96_profile_cleanup.rs`. It never audited the rest of the suite for the same shape, and
146's own scope-check that *did* flag related duplication (the six `firefox_with_daemon`
wrappers) was a code-smell note, not a leak-source search — none of the four `ManuallyDrop`
sites or `live_142_disk_growth.rs` were touched.

Also fixed for completeness (Theme A, self-identification): `commands::launch::run` now writes an
`.ff-rdp-owner-test` marker naming the spawning test alongside `.ff-rdp-owner-pid`, gated on the
live-test harness setting `FF_RDP_LIVE_TEST_NAME` (see `tests/common/mod.rs`'s
`SPAWNING_TEST_ENV` / `crate::util::profile_dir::SPAWNING_TEST_ENV`). `profiles prune --all`'s
warn log and `live_96_profile_cleanup.rs`'s precondition message both now name the culprit test
when the marker is present.

**Not fixed, deferred as follow-up** (see Notes): four live suites (`live_oneway.rs`,
`live_target_destroyed.rs`, `live_cross_actor.rs`, `live_61l.rs`) each define their own duplicate
local `LiveFirefox` struct with an already-correct `Drop` impl, predating the iter-100b
consolidation into `tests/common/mod.rs`. They are not a correctness risk (their guards already
work), so they were left alone rather than expanding this iteration's diff — but they don't get
Theme A's owner-test marker, since only `common::LiveFirefox` writes `SPAWNING_TEST_ENV`.
Consolidating them is exactly the cleanup [[iteration-146-live-suite-reliability]]'s scope-check
flagged; still worth doing, just not required to close this leak.

## Acceptance Criteria [2/4]

> The two chunk-orphan ACs below are deliberately **not ticked**: their named tests are
> implemented and compiled but were never executed end-to-end, and CLAUDE.md is explicit that
> "an AC without a named test is not done". They are settled by the post-batch chunked live
> sweep on `main`, not by this PR. Tick them there, with the run's actual orphan count.

- [x] live_151_leaked_profile_names_its_test: a profile directory left behind by a live test
      identifies the spawning test, and a deliberately-leaked instance is traceable to its test
      from the artifact alone — verified live 2026-08-12, PASS
- [ ] live_151_chunk_a_leaves_no_orphans: a full chunk-A run (the filter this plan's dogfood_path
      and Environment quirks section document) leaves zero surviving ff-rdp-spawned Firefox
      processes — implemented and compiled; gated behind `FF_RDP_LIVE_SUITE_CHECK=1` (nests a
      ~6 min chunk run, see the test's own doc comment) and not exercised end-to-end in this
      session's time budget — the mechanism it exercises (`live_96`'s live-owner precondition
      scanning the real profile root) was verified directly: a targeted 13-test live run covering
      every Theme B fix site left `profiles list` reporting `count: 0` afterward
- [ ] live_151_chunk_b_leaves_no_orphans: the complementary chunk-B run (skips chunk A's filter)
      leaves zero surviving ff-rdp-spawned Firefox processes, and `live_96_profile_cleanup`'s
      precondition passes without manual cleanup — same status as
      `live_151_chunk_a_leaves_no_orphans` above; `live_profiles_prune_removes_all_when_no_
      firefox_running` (the precondition test) itself PASSed in the same targeted run
- [x] live_151_root_cause_documented: this plan's Resolution names the confirmed leak source(s)
      and why 146's Theme A fix did not cover them — proven live by the identically-named test,
      not a hypothesis (see Resolution above)

## Notes

- iter-149 review (this session, 2026-08-12): running `check-dogfood-script` with
  `FF_RDP_LIVE_TESTS=1` against iter-149's `dogfood_path` (`ff-rdp launch --headless --port 6100`
  → `navigate` → `a11y --native`) left zero `ff-rdp-profile-*` directories and zero surviving
  `start-debugger-server` processes afterward — that single-invocation launch/a11y/exit path is
  not, by itself, a reproduction of this leak. Also observed: `pgrep -af start-debugger-server`
  transiently matched PIDs in this shared sandbox that `ps -p <pid>` immediately reported as
  already gone — i.e. plain process-count snapshots can be noisy here independent of anything
  ff-rdp does. Both observations support Theme A/C's approach (a per-test ownership marker, not a
  raw process count) over trying to tighten the pgrep-based measurement itself.
- Do not "fix" this by widening cleanup (a blanket kill of stray Firefox at suite start or end).
  That hides the leak exactly the way [[iteration-146-live-suite-reliability]] Theme C refused to
  hide the daemon-parity flake, and it would mask a *product* leak if one is ever the cause.
- The measurement above was taken with the session's own Firefox browser (pid 907) running; it
  uses a different profile path and was correctly excluded. Any check added here must likewise
  distinguish ff-rdp-spawned instances from the developer's own browser.
- Full-suite runs of the live binary take ~12 minutes and were repeatedly killed in this
  environment; the two-chunk split (`live_1` / `--skip live_1`) is what made the measurement
  possible and is the recommended way to reproduce.
- Every live test added here must exercise the **default daemon path**. Do not add entries to the
  shrink-only grandfather list in `crates/ff-rdp-cli/tests/no_daemon_live_test_guard.rs`.
- Implementation session (2026-08-12): a targeted live run covering every Theme B fix site —
  `live_142_daemon_stop_pid_honesty`, `live_142_disk_growth` (both tests), `live_151_residual_leak`
  (all 4), `live_90_daemon_lifecycle` (all 3), `live_96_profile_cleanup` (all 3) — 13/13 passed in
  15.25s with `--test-threads=1`, and `ff-rdp profiles list` reported `count: 0` immediately
  afterward (stronger evidence than a `pgrep` scan — see the pgrep-noise observation above, which
  reproduced again in this same session: `pgrep -af start-debugger-server` kept matching a
  different already-gone PID on every successive call).
- Follow-up filed as a note, not a new plan (small, non-blocking, no user-visible surface):
  `live_oneway.rs`, `live_target_destroyed.rs`, `live_cross_actor.rs`, and `live_61l.rs` each
  still define their own duplicate local `LiveFirefox`/`Drop` pair instead of using
  `tests/common/mod.rs`'s. Consolidating them would also give them Theme A's owner-test marker
  for free. Not done here to keep this iteration's diff scoped to the confirmed leak sources.
- The two `live_151_chunk_*_leaves_no_orphans` tests nest a full chunk run inside themselves via
  `std::env::current_exe()` and are gated behind an additional `FF_RDP_LIVE_SUITE_CHECK=1` (on
  top of `FF_RDP_LIVE_TESTS=1`) specifically so a normal live-suite invocation — including CI's
  `live.yml`, which runs the whole suite with no filter — doesn't silently double in duration.
  They were not run end-to-end in this implementation session (each nests ~6 minutes and the
  session's background-command budget was better spent verifying every individual fix site
  directly, per the note above); a human or CI operator opting into
  `FF_RDP_LIVE_SUITE_CHECK=1` is the intended way to exercise them for real.

## Run guidance (batch 149 → 151 → 150 → 148)

Non-negotiable working rules for whoever implements this plan:

1. **Do not trust the root cause stated above.** Across iterations 135–146 the real cause
   differed from the plan's hypothesis at least six times, and twice the wrong hypothesis was in
   a plan Claude itself wrote — [[iteration-146-live-suite-reliability]] guessed `LiveFirefox` for
   the leak and a daemon restart for the parity flake; both were wrong (two real bugs in
   `daemon/server.rs`). Reproduce the symptom and verify the mechanism **on the wire** (actual RDP
   packets / actual command output) before writing the fix. If the diagnosis here turns out to be
   wrong, fix the real cause and correct this section.
2. **A live test that passes `--no-daemon` proves nothing about the default path.** Every live
   test added here must exercise the default (daemon) path. iter-137's guard is at
   `crates/ff-rdp-cli/tests/no_daemon_live_test_guard.rs` with a shrink-only grandfather list —
   **do not add entries to that list.**

### Environment quirks (measured, session of 2026-08-12)

- Long background commands are killed at ~9–10 min. A full live run of `ff-rdp-cli` takes ~12 min
  and was killed three times. Run it in **two chunks**:
  `cargo test-live -p ff-rdp-cli -- --include-ignored --test-threads=1 live_1` and the same with
  `--skip live_1`. Each finishes inside the budget.
- Prewarm with `cargo build --workspace --all-targets` first — this avoids the xtask nested-cargo
  deadlock.
- Kill stray ff-rdp Firefox instances **before** any live run; a leftover breaks the daemon-stop
  and profile-prune tests. The developer's own browser is a separate process with no debugger
  port — do not kill it.
- `pgrep -f "firefox.*ff-rdp-profile"` matches its **own** shell command line, so counting orphans
  that way over-reports by exactly one. Use `pgrep -af start-debugger-server`.
- `ff-rdp-core` live tests must also run sequentially (`--test-threads=1`) against a headless
  Firefox on port 6000; in parallel, 4 tests fail from shared-Firefox interference.
