---
title: "Iteration 171: a leaked profile dir's owner-PID marker outlives its process, so PID reuse makes dead profiles read as live"
type: iteration
date: 2026-08-16
status: planned
branch: iter-171/stale-owner-pid-marker
depends_on: [iteration-168-livefirefox-drop-does-not-wait-for-exit]
first_call_sites: []
dogfood_path: |
  # Test-harness/product-boundary defect. The observable is a false-positive
  # liveness read on a profile whose owning process is long gone.

  # 1. Show that a dropped LiveFirefox leaves its profile directory behind.
  #    Count ff-rdp-profile-* dirs before and after one live test.
  ff-rdp profiles list --jq '.results.path'
  ROOT=$(ff-rdp profiles list --jq -r '.results.path')
  ls "$ROOT" | grep -c '^ff-rdp-profile-'
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live -q -- --ignored \
    --exact live_128_network_output_fidelity::live_128_meta_route
  ls "$ROOT" | grep -c '^ff-rdp-profile-'
  # → EXPECTED: the count grows by one, and the new dir still contains a
  #   .ff-rdp-owner-pid naming a process that no longer exists.

  # 2. Show the marker is stale, not just present: read the pid and confirm it
  #    is dead.
  for d in "$ROOT"/ff-rdp-profile-*; do
    pid=$(cat "$d/.ff-rdp-owner-pid" 2>/dev/null) || continue
    kill -0 "$pid" 2>/dev/null && echo "$d: pid $pid ALIVE" || echo "$d: pid $pid dead"
  done

  # 3. Force the false positive. Reuse is the hazard, so simulate it rather
  #    than waiting for the OS: write a *live* unrelated pid into a dead
  #    profile's marker and confirm `profiles prune --all` then refuses it.
  #    (Do this in a scratch profile root, never the real one.)

  # 4. Measure how fast this machine actually recycles PIDs — how many process
  #    spawns, and how much wall clock, before a given pid comes round again.
  #    That sizes the real-world exposure over a ~40-minute live sweep.
tags: [iteration, testing, live-tests, harness, profiles]
---

# Iteration 171: stale owner-PID markers and PID reuse

Carry-over from [[iteration-168-livefirefox-drop-does-not-wait-for-exit]] Theme A3 and Theme C.
Iteration 168 fixed a real defect (`LiveFirefox::drop` signalled without waiting; measured window
16–27 ms) but its Theme A measurements **disproved its own causal claim**: that window cannot
explain the `live_96_profile_cleanup` precondition failure iter-165 observed, because 176 tests
run between `live_128_meta_route` and `live_96` in the sequential live binary. This iteration
picks up the explanation iteration 168 left open.

## What is actually suspected

`LiveFirefox::drop` kills the process but leaves the `ff-rdp-profile-*` directory on disk, and
that directory contains `.ff-rdp-owner-pid`. The marker therefore outlives the process it names,
for the rest of the sweep and beyond. `live_96_profile_cleanup`'s precondition — and
`profiles prune`'s own liveness check — ask `kill(pid, 0)`, which cannot distinguish "the process
that wrote this marker" from "whatever process now holds that pid". Once the OS recycles the pid,
a dead profile reads as live-owned and `prune --all` correctly refuses to remove it.

This matches every property of the iter-165 observation that iteration 168's mechanism did not:
it survives arbitrary wall-clock distance between the two tests, it is rare, and it gets likelier
the more processes the run spawns — which is exactly what "load average 18.6" describes.

### Added 2026-08-17 — a second, non-hypothetical way the marker goes stale

Reproduced by accident while re-running the sweep on `main` at `4d639e2`: a sweep **killed
mid-test** orphans that test's browsers outright, because `LiveFirefox::drop` never runs. A run
terminated during `live_158_launch_survives_contended_bind` left four Firefox processes alive for
over an hour. They were still holding their profile dirs when the *next* sweep ran, and broke it
twice — `live_158` failed with `port 7101 is already in use by firefox (PID 66554)`, and
`live_96_profile_cleanup` failed its precondition naming all four dirs.

Two things this adds to the plan, both cheap to act on:

- the false positive does **not** require PID reuse to bite. A killed runner leaves genuinely-live
  processes owning profile dirs that no test will ever clean up, which is the same end state and
  far more likely than pid recycling on a developer machine;
- **the owner-test marker did not name anything.** All four read `spawned by unknown test`, so
  iter-151 Theme A's marker — the thing that exists to name the culprit — does not survive the
  process being killed rather than dropped. Whatever Theme B does about staleness should also make
  the marker durable at *launch* time, not at drop time.

A clean sweep on the same commit, with those orphans cleared first, was 269 passed / 1 failed with
`live_96_profile_cleanup` **green** — so nothing here contradicts iteration 168's fix; it means
the remaining exposure needs either contention or an interrupted run to surface.

**Unverified.** iteration 168 measured the 16–27 ms window but did not measure PID recycling, and
did not force the false positive. Theme A below does both before anything is changed.

## Themes

- **A — Confirm or kill the hypothesis.** Run the `dogfood_path`. Establish (i) that dropped
  `LiveFirefox` instances really do leave markered directories behind, (ii) that a forced pid
  collision really does trip the precondition, and (iii) how fast this machine recycles pids in
  process-spawn and wall-clock terms. If a pid cannot plausibly recycle inside one sweep, this
  hypothesis is wrong — say so and look elsewhere (start with: did that Firefox actually die?).
- **B — Make the marker self-invalidating.** `kill(pid, 0)` is not an identity check. Options to
  weigh explicitly, not silently: record process start time alongside the pid and compare both;
  have `LiveFirefox::drop` remove its own profile directory once the process is confirmed gone
  (iteration 168 Theme C decided this is unhandled, not deliberate); or have the product's
  liveness check verify the process is actually a Firefox it owns. Whichever is chosen, the
  reasoning for rejecting the others belongs in the plan.
- **C — Decide where the fix lives.** The marker is written by the product
  (`util::profile_dir`) and read by both the product (`profiles prune`) and the harness
  (`live_96`, `live_151`, `live_168`). A harness-only fix leaves real users exposed to the same
  false positive; a product-side fix is a wider blast radius. Pick one and say why.

## Theme A — measured 2026-08-17, before any code changed

### A1. Does a dropped `LiveFirefox` leave a markered profile dir?

**The plan's own step-1 example does not reproduce, for a reason that does not rescue the
hypothesis.** Running `live_128_meta_route` left the profile count at exactly 20 before and after,
with the *same set* of directory names (`comm -13`/`-23` both empty):

```
before=20 after=20
--- new dirs ---   (none)
--- removed dirs --- (none)
```

`live_128_meta_route` calls `stop_daemon(port)`, and `daemon stop` runs `cleanup_profile_dir`,
which removes the managed directory. So the *right* probe is a launch with no `daemon stop`
after it. Done by hand:

```
$ ff-rdp launch --headless --debug-port 7311        # FF_RDP_LIVE_TEST_NAME=manual_theme_a
new dir: ff-rdp-profile-6EI9roVTjc93dRmD
  .ff-rdp-owner-pid .ff-rdp-owner-test .parentlock cert9.db cookies.sqlite … user.js
  pid=14338 test=manual_theme_a
$ kill -9 14338
alive after kill: no
dir still exists: yes
marker still names pid: 14338      ← stale, and it stays stale forever
```

Confirmed: the directory and its marker outlive the process. The 20 directories already on disk
when this started were all **marker-less** (only `user.js`), i.e. launches that died before
writing a marker — see A5.

### A2. Does a forced pid collision trip the checks?

Yes, both of them.

```
== (a) marker names DEAD pid 14338
  ff-rdp profiles prune --older-than 0s --dry-run → in would_remove: 1
== (b) marker forged to LIVE pid 17262 (simulated pid reuse)
  ff-rdp profiles prune --older-than 0s --dry-run → in would_remove: 0
== (c) live_96-style precondition scan
  PRECONDITION VIOLATION: ff-rdp-profile-6EI9roVTjc93dRmD pid 17262 alive, test=manual_theme_a
```

One byte of difference in the marker flips an abandoned profile from "reclaim it" to "never
reclaim it at any age", and makes `live_96_profile_cleanup` fail its precondition naming a
browser that has been dead for minutes. Note the plan's phrasing "`prune --all` then refuses it"
is not accurate — `--all` *does* remove live-owner dirs (it warns and lists them under
`removed_live`, iter-97 Theme C). The path that refuses is the **age-gated** one, which is worse:
it refuses silently and forever.

### A3. How fast does this machine recycle pids?

```
1000 `sh -c :` spawns: pid 15558 → 16662  (1104 pids) in 4.83 s  ≈ 229 pids/second
macOS PID_MAX = 99 999 → one full wrap ≈ 437 s ≈ 7.3 minutes of saturated spawning
```

A ~40-minute live sweep spawns several `ff-rdp` processes per test across ~270 tests, plus a
Firefox parent and its content processes per launch. A wrap inside one sweep is **plausible**, not
exotic — and every leaked marker is a live target for the whole remainder of that sweep. The
hypothesis holds; the iteration proceeds.

### A4. Correction to the "Added 2026-08-17" block

That block inferred, from four orphaned profiles all reading `spawned by unknown test`, that
"the owner-test marker … does not survive the process being killed rather than dropped". **That
inference is wrong.** `live_158_launch_survives_contended_bind` — the test the interrupted run
died in — spawns `ff-rdp launch` through a bare `Command::new(ff_rdp_bin())` and therefore never
sets `FF_RDP_LIVE_TEST_NAME` at all. The marker did not decay; it was never requested. **22 such
call sites exist across 12 live-test files**, and all of them were producing unattributable
profiles. (The first pass claimed 20 across 10 and was wrong: this PR's own review found two more
— `live_123_daemon_autostart_and_registry.rs`'s `launch --replace` and `live_153`'s shared
`run_raw` helper — which is why the count is stated here as verified rather than as remembered.
`live_158`'s two remaining bare `Command::new(ff_rdp_bin())` sites are `daemon stop` and `tabs`;
neither spawns a Firefox, so neither needs the tag.) Fixed by routing them through a tagged `ff_rdp_launch_command()` (and
`ff_rdp_launch_command_for` for `live_158`'s worker threads, which are unnamed and would otherwise
still stamp `unknown`).

The block's *other* point stands and is acted on: markers are now written the instant the PID
exists, not after the port probe, which under contention can legitimately run for tens of seconds
(iter-158). A caller killed inside that window used to leave a completely unmarked directory.

### A5. Residual, filed rather than fixed here

The 20 pre-existing directories contained `user.js` and nothing else — no marker, no Firefox
artefacts. Those are launches that failed (or were killed) between `build_command` and the marker
write, and marker-less directories fall back to the 7-day mtime heuristic, so they linger. Moving
the marker write earlier shrinks that window to almost nothing but does not close it: a failure
inside `build_command` itself still leaks. Filed as
[[iteration-175-failed-launch-leaks-unmarked-profile-dir]].

## Theme B — the chosen mechanism, and why not the others

**Chosen: record the owning process's OS start time alongside its PID and compare both.**

- `daemon::process::process_start_token(pid) -> Option<String>` returns an opaque per-incarnation
  token: `proc_pidinfo(PROC_PIDTBSDINFO)` on macOS, `/proc/<pid>/stat` field 22 on Linux,
  `GetProcessTimes` on Windows, `None` elsewhere.
- It is persisted in a **sibling** file, `.ff-rdp-owner-start`, not as a second line of
  `.ff-rdp-owner-pid`. Three out-of-crate readers (`live_96`, `live_151`, `live_168`) parse that
  file with `read_to_string(..).trim().parse::<u32>()`; a two-line body would make every one of
  them silently stop matching, which for `live_96` means its precondition quietly stops firing —
  the exact softening AC 3 forbids.
- `owner_liveness()` grades the pair into four states, because the two consumers want *opposite*
  fallbacks when identity cannot be established: a deletion path must keep the directory when in
  doubt, and the kill-scoping gate must refuse to signal when in doubt.

Rejected:

- **Have `LiveFirefox::drop` remove its own profile directory.** Harness-only, so `profiles prune`
  and every real user keep the false positive; and it does nothing for the case that actually bit
  us twice, where the runner is killed and `drop` never runs at all.
- **Have the liveness check verify the process is a Firefox ff-rdp owns** (match the executable
  and its `-profile` argument). Strictly more work than the start time for the same answer,
  needs a per-OS process-inspection path anyway, and it is defeated by the one case start time
  handles for free — a recycled PID that happens to *be* another Firefox, which on a machine
  running a live sweep is not a remote possibility.
- **Age out markers by wall clock** (treat a marker older than N hours as stale). Wrong in both
  directions: a long-running session is not stale, and a recycled PID one minute after death is.

## Theme C — placement

**Product-side.** The marker is written by `util::profile_dir` and read by the product's own
`profiles prune`, `launch`'s orphan sweep and iter-110's kill-scoping gate; the harness is merely
one more reader. A harness-only fix would leave a real user's profile store growing without bound
(the age-gated prune skipping a recycled-PID directory forever) and would leave the kill gate
holding a stale permission slip for a PID ff-rdp no longer owns — the one thing that gate exists
to prevent. Blast radius is contained by making the new signal *additive*: a profile with no
`.ff-rdp-owner-start` (every profile written by an older ff-rdp) grades exactly as it did before.

## Tasks

### A. Verify
- [x] Run every step of `dogfood_path` and paste actual outputs into this plan
- [x] Record whether a dropped `LiveFirefox` leaves a markered profile dir, with counts
- [x] Record the forced-collision result: does a live pid in a dead profile's marker trip
      `live_96`'s precondition and `prune --all`?
- [x] Record this machine's PID recycle rate (spawns and wall clock)

### B. Fix
- [x] The chosen invalidation mechanism, with the rejected alternatives recorded
- [x] Unit tests that do not require a real Firefox
- [x] A live test that fails on `main` and passes on the branch, the way `live_168` does

### C. Placement
- [x] Record whether the fix is harness-side or product-side, and why

## Acceptance Criteria [4/4]

- [x] The Theme A verification is recorded in this plan, including the decision that follows if
      the hypothesis does not hold
- [x] A stale marker naming a recycled pid no longer reads as a live owner — asserted by a test
      that fails on `main`
      — `pre_fix_repro_recycled_owner_pid_reads_as_live`,
      `pre_fix_repro_prune_never_reclaims_recycled_pid_profile`,
      `unit_pid_is_ff_rdp_spawned_refuses_recycled_pid` and
      `live_171_recycled_owner_pid_no_longer_reads_as_live`. "Fails on main" was demonstrated,
      not asserted: `owner_liveness` was temporarily patched back to a bare `kill(pid, 0)` and all
      three unit tests failed, then the patch was reverted.
- [x] `live_96_profile_cleanup`'s precondition is left as loud as iter-146 Theme B made it (no
      softening back into a skip) — untouched; the sibling-file marker layout was chosen
      specifically so its `parse::<u32>()` keeps matching
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q`
      clean, plus a dual-gate live sweep

## Out of scope

- Re-litigating iteration 168's bounded wait. It closed a separately measured defect and stays.
- Softening `live_profiles_prune_removes_all_when_no_firefox_running`'s precondition. Same reason
  as in iteration 168: iter-146 Theme B removed the skip on purpose.

## References

- [[iteration-168-livefirefox-drop-does-not-wait-for-exit]] — Theme A3 (why the 16–27 ms window
  cannot explain iter-165) and Theme C (the profile dir is left behind, and that is unhandled
  rather than deliberate)
- [[iteration-165-eval-scope-leak-contradicts-help]] — the sweep that surfaced the original
  `live_96` failure
- [[iteration-151-residual-live-firefox-leak]] — `OWNER_TEST_MARKER`, which names the culprit test
  in the failure message
- [[iteration-146-live-suite-reliability]] — Theme B, the loud precondition that detects this
