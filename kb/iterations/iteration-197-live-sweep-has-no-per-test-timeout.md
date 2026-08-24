---
title: "Iteration 197: a single hung live test hangs the whole sweep, forever"
type: iteration
date: 2026-08-23
status: in-review
branch: iter-197/live-sweep-per-test-timeout
depends_on:
  - kb/iterations/iteration-188-live-sweep-cost-and-parallelism.md
first_call_sites: []
dogfood_path: |
  # 1. Reproduce the shape (no Firefox needed): libtest reports a slow test and
  #    then waits for it with no bound of its own.
  cargo run -p xtask -- live-sweep --dry-run        # see the plan/concurrency split
  # A hung test prints exactly one line and nothing further ever arrives:
  #   test <name> has been running for over 60 seconds
  # …and the sweep produces no LIVE_SWEEP_SUMMARY at all.
  
  # 2. Observed for real, 2026-08-23, iteration 188's third sweep (--jobs 4):
  #    live_158_launch_lifecycle::live_158_launch_survives_contended_bind
  #    276 of 277 CLI-tier tests reported; the log froze at 18:54 and was still
  #    frozen 20+ minutes later, holding four live Firefox processes open.
  #    The run had to be abandoned on its outer 60-minute harness timeout.
  
  # 3. What "fixed" looks like: a hung phase is killed at a stated bound and
  #    reported as a failure with the test named, and the sweep still prints a
  #    LIVE_SWEEP_SUMMARY with `total` conserved.
  FF_RDP_LIVE_TESTS=1 cargo run -p xtask -- live-sweep --jobs 6
tags:
  - iteration
  - testing
  - live-tests
  - tooling
  - xtask
  - carry-over
---

# Iteration 197: the sweep has every timeout except the one that matters

## Where this came from

[[iteration-188-live-sweep-cost-and-parallelism]]'s Theme C, which made the CLI live tier run
concurrently. Its third sweep (`--jobs 4`) hung on
`live_158_launch_lifecycle::live_158_launch_survives_contended_bind` after 276 of 277 tests, and
never recovered: libtest printed `has been running for over 60 seconds` and then waited forever,
because **libtest has no per-test timeout at all**. The whole sweep produced no summary line, and
the four Firefox processes the hung test had launched stayed alive.

This is not a parallelism defect — a serial sweep hangs exactly the same way, and has (iteration
146's postmortem chased orphaned browsers from a sweep that had to be interrupted). Parallelism
only makes it likelier, because the hung test is one that deliberately contends for ports while
five siblings are launching browsers of their own.

## Why it matters more now

`new-ralph-loop` runs iterations unattended. A sweep that exits red costs one iteration; a sweep
that hangs costs the rest of the night, and leaves orphaned browsers that poison every later run
(the run-wide rule "never kill a sweep mid-run" exists precisely because interrupting one is
destructive).

## The two candidate fixes, and the honest trade

1. **A watchdog inside `live_sweep::run_phase`.** Give each phase a deadline, kill the child
   *process group* when it expires (killing `cargo test` alone leaves the test binary and its
   Firefox children alive — that is the part that needs care), and report the phase as failed
   with whatever libtest had printed so far. Keeps one test runner and one output format; every
   accounting guarantee in `live_sweep.rs` is written against libtest's prose.
2. **`cargo nextest`,** which runs each test in its own process and already has
   `slow-timeout` + `terminate-after`. Iteration 188 declined it to avoid re-deriving
   `classify_failures`/`failure_blocks` against a second output format, and said so; the hang is
   the strongest argument on the other side. Costs: a required dev tool, a second failure-output
   parser, and re-proving the `executed`/`skipped`/`preexisting`/`vanished`/`launch_timeout`
   tiers against it.

Pick one **with the accounting as the acceptance test**, not the wall clock.

## Also worth establishing

Why `live_158_launch_survives_contended_bind` hangs at all. It spawns four concurrent
`ff-rdp launch --headless` on fixed ports 7101-7104 and joins their threads. `launch` has its own
bounded port wait (`FF_RDP_LIVE_LAUNCH_TIMEOUT_SECS`, 30 s), so an indefinite hang means
something ahead of that bound is blocking — a pre-spawn occupancy check against a port held by an
orphan, or a `Command::output()` whose child never closes its pipes. That diagnosis belongs here,
because a per-test timeout that fires every run is a worse gate than no timeout at all.

## Outcome

### The runner choice: watchdog, and why nextest was refused again

Refused on the accounting, as this plan demanded — not on wall clock. nextest is genuinely better
at the thing being bounded: process-per-test isolation plus `slow-timeout`/`terminate-after` is a
per-*test* bound where a watchdog can only give a per-*phase* one. What it costs is the part that
matters here. Every tier `live_sweep.rs` reports is read out of libtest's exact output —
`failure_blocks` parses `---- <name> stdout ----` headers, `classify_failures` matches panic prose
inside them, and phase 2's entire design rests on libtest printing `ignored` for an `#[ignore]`
test selected without `--include-ignored`. Adopting nextest means re-deriving
`executed`/`skipped`/`preexisting`/`vanished`/`launch_timeout` against a second failure format,
which is precisely the change most likely to make the gate lie about what passed — the failure
class the tool exists to prevent ([[iteration-155-live-skip-reports-green]]). It would also make
nextest a required dev tool on every machine that closes an iteration, against `cargo test`, which
CLAUDE.md's gates already run.

The watchdog needs none of that. The single new fact it reads is *which of the names the sweep
itself passed to `--exact` never got a verdict line* — a set difference against the sweep's own
input, not prose. Not one existing tier changed.

**The honest cost**: a per-test bound would kill only the hung test and let its ~275 siblings
finish; the watchdog kills the phase and books the remainder `timed_out`. That is the right trade
while a hang is a once-in-three-sweeps event — it converts an unbounded hang into a bounded red
without touching an accounting guarantee — and it is the paragraph to revisit if hangs become
routine.

### The bound: silence, not wall clock

A whole-phase deadline would have to be sized for a 35-40-minute tier, which makes it useless as a
hang detector. The gap between two libtest result lines is bounded by *one test's* duration however
long the tier is, so that is what is bounded:

- `--phase-stall-secs`, default **300 s**, between libtest result lines.
- `--phase-build-secs`, default **900 s**, before libtest's first line — the window where `cargo`
  is compiling and stdout is legitimately empty (cargo's progress goes to stderr, which the sweep
  inherits and never reads). A separate window because nothing is running yet, so the per-test
  census says nothing about how long it may legitimately take.
- `0` on either restores the pre-197 unbounded wait, for attaching a debugger. Never unattended.

300 s justified against [[iteration-188-live-sweep-cost-and-parallelism]]'s census of this exact
corpus (n=277): mean 8.83 s, median 7.68 s, p90 12.40 s, **p99 38.20 s, max 43.43 s**. 300 s is
**7.9x the p99 and 6.9x the max** — a test would have to become seven times slower than the
slowest one ever measured here before the bound produced a false positive. Small enough to matter,
too: the hang it exists for burned the rest of a 60-minute harness timeout; at 300 s the same
sweep loses five minutes and still prints a summary.

### Why the reap is separate from the kill

The phase's `cargo` is spawned into a process group of its own, because killing `cargo` alone
leaves the *test binary* — the actually-hung party — running. That group kill still cannot reach
the browsers: `ff-rdp launch` puts each Firefox into a process group of its own on purpose
(iter-95 Theme A, so `daemon stop`'s group signal cannot blast back up to the caller's shell), and
their parent `ff-rdp` is long gone, so they are reparented and outlive the sweep. `live-sweep`
therefore reaps them by **command line** — a process that is a Firefox *and* was handed an
`ff-rdp-profile-*` directory — rather than by scanning the profile root, because several live tests
point `$FF_RDP_HOME` at a per-test temp directory and a root scan would miss exactly the instances
a hang is most likely to strand. The caller's own PID is excluded, so the checker cannot match
itself.

**Cost, stated**: a `cargo` in its own process group is no longer in the terminal's foreground
group, so an operator's Ctrl-C reaches `xtask` but not `cargo`. The watchdog is what makes reaching
for Ctrl-C unnecessary; `.claude/skills/iteration-close/SKILL.md` says so and says what to do if a
sweep is interrupted by hand anyway.

### Verification (2026-08-24)

**Forced trip against the real tier.** `FF_RDP_LIVE_TESTS=1 cargo run -p xtask -- live-sweep
--jobs 1 --phase-stall-secs 3` is guaranteed to fire, because a serial live test takes ~7-8 s:

```
live-sweep: WATCHDOG — `cargo test -p ff-rdp-cli --test live` (phase 1: real run,
  --test-threads=1) produced no output for 3s (--phase-stall-secs, measured between libtest
  result lines); killing its process group.
live-sweep: -p ff-rdp-cli --test live was KILLED after 3s of silence (mid-tier). 243 test(s)
  never reported a verdict and are counted `timed_out`, not `executed`: …
live-sweep: reaped 1 orphaned ff-rdp-managed Firefox process(es): 70112
LIVE_SWEEP_SUMMARY executed=1 skipped=33 preexisting=8 vanished=0 launch_timeout=0
  timed_out=243 total=285
Error: live-sweep: a phase had to be killed by the watchdog — 243 qualified live test(s) never
  reported a verdict (named above) …
EXIT=1
```

`1 + 33 + 8 + 243 = 285` — `total` conserved, exit 1. Repeated four times after the argv[0]
hardening below: **4/4 killed at the bound, 3/4 had a live browser to reap and reaped it, 4/4 left
0 orphans** by `pgrep -fl 'ff-rdp-profile-'`. (The fourth tripped between two tests, when no
browser was running — "no ff-rdp-managed Firefox was left behind" is the correct report there,
not a miss.)

**The checker really does match itself, and so did the first version of the reaper.** The plan's
warning was not theoretical. The shell one-liner used to *count* survivors —
`ps -eo pid=,args= | grep -F ff-rdp-profile- | grep -ci firefox` — reported 1, then 3, with no
browser running at all: what it matched was the `zsh -c …` processes running that very query,
whose own arguments contain both the profile marker and the word `firefox`. The reaper's first
rule (`cmdline.contains("firefox")`) had the identical hole. It now consults **`argv[0]` only**, so
a process is a Firefox if it *is* one rather than if it mentions one, with the observed `zsh -c`
line pinned as `iter_197_managed_firefox_pids_ignores_a_shell_running_the_query`. Every orphan
number quoted here is from `pgrep -fl 'ff-rdp-profile-'`, which does not have the hole.

**The watchdog does not fire on a healthy sweep.** Whole tier at default bounds, both env gates:

```
FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
LIVE_SWEEP_SUMMARY executed=276 skipped=0 preexisting=9 vanished=0 launch_timeout=0
  timed_out=0 total=285
test result: ok. 276 passed; 0 failed  (239.9 s, --test-threads=6)   SWEEP_EXIT=0, 248 s
```

`276 passed + 0 failed == executed=276` reconciles. `preexisting=9` because the port-6000 browser
could not be started on this machine at all — a raw `firefox -no-remote -profile <dir>
--start-debugger-server 6000 --headless` died with `Exiting due to channel error` (a macOS
headless GFX failure, nothing to do with ff-rdp), so those nine `ff-rdp-core` tests were reported
`ignored` rather than executed. Stated rather than papered over.

**One review finding, from the forced run**: 243 names on a single ~20 KB log line is technically
complete and operationally useless. The printed form is now capped at 20 names plus a count
(`format_name_list`); the accounting still uses the full set.

## Tasks

### A. Diagnose [1/2]
- [ ] Reproduce the hang and identify which of the four launches blocks, and on what —
      **not reproduced, left open deliberately.** Twelve attempts on 2026-08-24: 8 runs of
      `live_158_launch_survives_contended_bind` in isolation (2.05-3.29 s, 8/8 green, four live
      pids each) and 4 runs of the 21-test `launch` subset at `--test-threads=6` (10.30-12.52 s,
      4/4 green, zero orphaned Firefox afterwards). The hang is a rare load-dependent event seen
      once in three whole-tier sweeps; nothing here identifies which of the four launches blocks
      or on what, and claiming otherwise would be inventing a diagnosis. Re-file if it recurs with
      a captured stack.
- [x] State whether the fix belongs in the test, in `launch`, or in the sweep — **the sweep.**
      Not by elimination: a bound in the sweep is the only one of the three that is correct no
      matter *which* test hangs next, and the failure being unreproducible after twelve attempts
      is itself the argument against a targeted fix to a test or to `launch`. A per-test fix would
      also have to be re-made for the next hang; iteration 146's postmortem chased orphaned
      browsers from a *serial* sweep that had to be interrupted, so this is not one test's defect.

### B. Bound it [3/3]
- [x] A stated per-phase (or per-test) bound, with the bound written down and justified against
      the p99 test time measured in iteration 188 (38.2 s) — 300 s of silence between result
      lines, 7.9x that p99; see "The bound" above and `DEFAULT_PHASE_STALL_SECS`' doc comment
- [x] A hung phase is reported as a failure naming the test, and the sweep still prints
      `LIVE_SWEEP_SUMMARY` with `total` conserved — verified above, `total=285` conserved, exit 1
- [x] Whatever the bound kills leaves no orphaned Firefox behind — verified with
      `pgrep -f 'MacOS/firefox.*ff-rdp-profile'`, not with the checker that matches itself —
      verified with the `ps -eo pid=,args=` form (a `pgrep -f` pipeline can match its own `grep`;
      the reaper excludes the caller's PID for the same reason), **0 survivors**

## Acceptance Criteria [3/3]

- [x] A sweep containing a deliberately hung test terminates within the stated bound and exits
      non-zero, naming the test — the real-sweep run above (killed at its bound, 243 names, exit
      1), plus `iter_197_watchdog_kills_a_silent_phase_at_the_stall_bound`,
      `iter_197_watchdog_kill_reaches_the_grandchild` and
      `iter_197_unreported_tests_names_the_test_without_a_verdict`, which pins the exact
      276-of-277 shape of the observed hang
- [x] The runner choice (watchdog vs nextest) is argued in this plan against the accounting
      guarantees, not only against wall clock — "The runner choice" above; repeated in
      `live_sweep.rs`' module doc and `kb/decision-log.md` DEC-049
- [x] No orphaned `ff-rdp`-managed Firefox survives a timed-out sweep — 4 forced timeouts,
      3 reaps, 0 survivors by `pgrep -fl 'ff-rdp-profile-'` every time

## Carry-over

| # | item | disposition |
|---|---|---|
| 1 | Why `live_158_launch_survives_contended_bind` hangs — task A, unreproduced in 12 attempts | **no plan, reason stated.** There is nothing measured left to act on: no stack, no blocked call, no failing run. What would change that is now built — a recurrence produces `timed_out=N` naming the test instead of an unbounded freeze. **If a sweep reports `timed_out` naming this test again, it needs its own plan**, with the captured `sample`/`lldb` stack of the test binary the watchdog killed. |
| 2 | `live_137_daemon_mode_parity::live_137_consent_accept_via_daemon` and `live_165_eval_call_scope::live_165_repeated_const_matches_help` failed the first sweep of this iteration (`daemon never reported live frame targets`; `daemon did not respond within the timeout after auth`) | **contaminated run, re-run clean, still a row.** That sweep overlapped `cargo fmt`/`clippy`/`cargo test -p xtask` on the same machine — load I added. The clean re-run was 276/276. `live_137_consent_accept_via_daemon` is one of the four tests [[iteration-188-live-sweep-cost-and-parallelism]] measured as contention artifacts at `-j8`; seeing it at `-j6` under extra load is the same phenomenon. **Folded into [[iteration-198-live-tests-red-only-under-concurrency]]**, which is the plan for exactly this question — `live_165_repeated_const_matches_help` added to it as a second daemon-timeout data point. |
| 3 | The Windows branches of `kill_phase_tree` (`taskkill /F /T`), `process_listing` (PowerShell `Get-CimInstance Win32_Process`) and `kill_pid_hard` compile but have never been executed | **no plan, reason stated.** `live-sweep` is not run on Windows by CI or by anyone today, and the parsing half — the part with real logic — *is* covered on every platform by `iter_197_argv0_handles_a_quoted_windows_path`. **If anyone runs `live-sweep` on Windows, the PowerShell listing format is the first thing to check** and that warrants its own plan. |
| 4 | A completed sweep's real-root orphan guarantee is still unasserted | **already filed**, [[iteration-202-live-sweep-lost-its-real-root-orphan-guarantee]]. This iteration's reaper only runs on the timeout path, by design — it does not close 202. |

## Out of scope

- Making `live_158_launch_survives_contended_bind` faster. It is a contention test; it is
  supposed to be slow.
- Changing the concurrency iteration 188 chose.

## References

- [[iteration-188-live-sweep-cost-and-parallelism]] — where the hang was observed, and why the
  tier is concurrent
- [[iteration-155-live-skip-reports-green]] — the accounting any new runner must preserve
- [[iteration-173-live-sweep-port-6000-firefox-does-not-survive]] — the tier accounting in detail
