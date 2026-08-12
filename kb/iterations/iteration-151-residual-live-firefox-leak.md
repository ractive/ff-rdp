---
branch: iter-151/residual-live-firefox-leak
date: 2026-08-12
depends_on: [kb/iterations/iteration-146-live-suite-reliability.md]
dogfood_path: |
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live -- --include-ignored --test-threads=1 live_1
  pgrep -af 'start-debugger-server'
  # → after the chunk exits, zero ff-rdp-spawned Firefox processes may survive;
  #   today roughly one leaks per ~100 live tests, from a source not yet named
first_call_sites: []
status: planned
title: "Iteration 151: a residual live-suite Firefox leak survives iteration-146"
type: iteration
tags: [iteration]
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

## Acceptance Criteria [0/4]

- [ ] live_151_leaked_profile_names_its_test: a profile directory left behind by a live test
      identifies the spawning test, and a deliberately-leaked instance is traceable to its test
      from the artifact alone
- [ ] live_151_chunk_a_leaves_no_orphans: a full `live_1` chunk run leaves zero surviving
      ff-rdp-spawned Firefox processes
- [ ] live_151_chunk_b_leaves_no_orphans: a full `--skip live_1` chunk run leaves zero surviving
      ff-rdp-spawned Firefox processes, and `live_96_profile_cleanup`'s precondition passes
      without manual cleanup
- [ ] live_151_root_cause_documented: this plan's Resolution names the confirmed leak source(s)
      and why 146's Theme A fix did not cover them — not a hypothesis

## Notes

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
