---
branch: iter-150/core-longstring-flake
date: 2026-08-12
depends_on: []
dogfood_path: |
  for i in 1 2 3 4 5 6 7 8 9 10; do
    cargo test -p ff-rdp-core -q specs::types::tests::resolve_slot_longstring_grip_fetches_full_value
  done
  # → 10 consecutive runs must pass; today the test fails intermittently and
  #   passes on rerun, which is how it has survived two separate sightings
first_call_sites: []
status: completed
title: "Iteration 150: resolve_slot_longstring_grip_fetches_full_value is intermittently red"
type: iteration
tags:
  - iteration
---

# Iteration 150: `resolve_slot_longstring_grip_fetches_full_value` is intermittently red

Filed from two independent sightings during the 138–146 batches, neither of which was in scope
for the iteration that saw it. No dogfooding session — evidence inline.

## Why this is worth an iteration rather than a shrug

`crates/ff-rdp-core` — `specs::types::tests::resolve_slot_longstring_grip_fetches_full_value`
fails, then passes on rerun. It has now been observed twice by different agents in different
iterations:

- during [[iteration-141-output-hygiene]]'s review pass ("one pre-existing flaky ff-rdp-core
  test … observed once, unrelated to this change, and passed on rerun")
- during [[iteration-146-live-suite-reliability]]'s gate run ("reproduced once then passed on
  rerun, documented in the plan as out-of-scope")

Two sightings across two batches is not noise. Each time it was correctly ruled out of scope for
the iteration in hand, and each time that judgement was right — which is exactly how an
intermittent survives indefinitely: always someone else's problem.

It also matters more than a typical flaky unit test. This is **not** a live test: it is a plain
`cargo test -p ff-rdp-core` unit test, so it runs on every CI job on every PR across three
platforms. An intermittent there is a random red on unrelated work, and the established response
in this repo has become "rerun it" — the habit [[iteration-146-live-suite-reliability]] warned
about, of training readers to ignore red.

## What is not yet known

The mechanism is unidentified. Do not assume it is a timing/threading issue just because it is
intermittent — the run guidance that corrected four plan hypotheses across 138–146 (two of them
in iteration-146, written from a symptom that looked conclusive) applies with full force here.

Candidate directions, none confirmed:

- a longstring grip resolution path with an ordering assumption that holds only sometimes
- shared mutable state or a fixture reused across tests in the same binary, making it
  order-dependent rather than genuinely random (check by running the test alone in a loop versus
  the full `-p ff-rdp-core` suite in a loop — if it only fails in-suite, it is contamination, not
  timing)
- a real bug in `resolve_slot`'s handling of a partially-delivered longstring, in which case the
  test is correctly reporting a product defect and must not be "stabilised"

That last possibility is the reason this plan forbids the obvious cheap fix.

## Themes

### Theme A — reproduce deterministically

Get from "fails sometimes" to "fails on demand" before changing any product code. A loop harness
that runs the test N times, and separately the whole `ff-rdp-core` suite N times, is the minimum;
record the observed failure rate for each in this plan.

### Theme B — root-cause and fix

Fix the actual mechanism. If it turns out to be a product bug in longstring grip resolution, the
fix belongs in `ff-rdp-core` and the test stays as-is. If it is test-local contamination, fix the
test's isolation.

**Explicitly forbidden**: `#[ignore]`, a retry wrapper, a sleep, or loosening the assertion. Any
of those converts a visible intermittent into an invisible one — the same trade
[[iteration-146-live-suite-reliability]] Theme C refused.

## Acceptance Criteria [3/3]

- [x] unit_150_longstring_deterministic_repeat:
      `resolve_slot_longstring_grip_fetches_full_value` passes 50 consecutive runs in isolation
      and 20 consecutive runs of the full `-p ff-rdp-core` suite
- [x] unit_150_mechanism_documented: this plan's Resolution section names the confirmed
      mechanism and the pre-fix failure rate measured in Theme A, not a hypothesis
- [x] unit_150_regression_pinned: if the cause was a product bug, a test exercises the specific
      longstring path deterministically; if it was test contamination, a test or harness change
      makes the isolation failure impossible rather than unlikely

## Resolution

**Confirmed mechanism** (test-local contamination, not a product bug — candidate direction #2
from "What is not yet known" above): `resolve_slot_longstring_grip_fetches_full_value` performs a
real 20 KB `recv_from` round-trip over a loopback TCP socket, which reads the process-global
`transport::max_frame_bytes()` cap (backed by the `MAX_FRAME_BYTES_CELL` atomic). Five tests in
`transport::tests` (`max_frame_mb_knob_works`, `bulk_frame_rejects_oversized_announcement`,
`bulk_frame_cap_send_side`, `recv_bulk_with_handler_oversized_rejected`,
`bulk_recv_caps_drain_length`) transiently shrink that cap to 1024 (or 100) bytes to test the
cap-rejection path, restoring it afterward via a panic-safe RAII guard. The restore is correct —
this is not the DEC-022 leaked-cap class of bug — but the *five* tests only serialized against
*each other* via a `Mutex` private to `transport::tests`; the longstring test, in a different
module, took no guard at all. `cargo test -p ff-rdp-core` runs its ~530 unit tests on a shared
default-parallelism thread pool (not `--test-threads=1` — that constraint is specific to the live
suite, see DEC-022), so whenever the longstring test's TCP round-trip happened to overlap with one
of those five tests' shrink window, `recv_from` observed the shrunk cap and rejected the ~20 KB
substring response with `ProtocolError::FrameTooLarge`.

**Verified on the wire, not assumed**: per the run guidance, the mechanism was confirmed by forcing
the interleave deterministically before writing any fix — a throwaway two-thread harness (one
thread looping the exact `resolve_long_string_slot` round-trip from the failing test, one thread
hammering `set_max_frame_bytes` between `1024` and the default with no synchronization) reproduced
`FrameTooLarge { declared: 20043, max: 1024 }` on the very first iteration of 500. Re-running the
*unmodified* pre-fix code as `cargo test -p ff-rdp-core`'s compiled unit-test binary in a tight
loop (20 consecutive full-suite runs, default parallelism, no artificial hammering) reproduced the
same `FrameTooLarge` failure twice — **measured pre-fix failure rate: 2/20 (10%) under real
`cargo test` parallelism**, consistent with "genuinely rare but not vanishingly rare" — enough to
explain two independent sightings across the 138–146 batches without appearing on every PR.

**Fix**: `transport::FRAME_CAP_LOCK` (previously a private `Mutex<()>` used only by the five
cap-mutating tests) is now a crate-visible (`pub(crate)`) `RwLock<()>`. The five cap-mutating tests
take `.write()` (unchanged exclusivity). The longstring test now takes `.read()` for the duration
of its network round-trip. This makes the interleaving structurally impossible rather than merely
serializing this one pairing: a writer excludes every reader for as long as the cap is shrunk, and
readers don't serialize against each other. See DEC-029 for the full write-up and rationale versus
DEC-022's related-but-distinct leaked-cap bug.

**Regression proof**: `frame_cap_lock_read_guard_excludes_writers` (new, in `transport::tests`)
deterministically proves the `RwLock` contract — no timing dependency, since `try_write`/`try_read`
either succeed or fail immediately based on lock state, not scheduling luck. Post-fix, the same
20-consecutive-full-suite-run measurement that reproduced 2/20 failures pre-fix now shows 0/20 (and
a separate 50-consecutive isolated-run check also passed 50/50, though isolation alone can't
exercise the race since the filtered `cargo test <name>` invocation excludes every other test from
the binary).

## Notes

- If Theme A cannot reproduce the failure at all after a substantial run budget, say so and
  re-defer rather than guessing at a fix — an unreproducible intermittent that resists a
  deliberate hunt is better left visible and documented than "fixed" speculatively. Compare
  [[iteration-144-session-hygiene-followup]] Theme D, which shipped a mitigation for an
  unreproduced symptom; prefer the [[iteration-147-console-locale-repro]] treatment (honest re-defer)
  when the symptom will not appear.
- Cheap first step before anything else: check whether the two recorded sightings share a
  platform, a parallelism setting, or a neighbouring test.

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
