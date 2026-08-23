---
title: "Iteration 188: the live sweep spends half its wall clock on Firefox cold starts and runs them one at a time"
type: iteration
date: 2026-08-18
status: done
branch: iter-188/live-sweep-parallelism
depends_on:
  - kb/iterations/iteration-181-playbook-scoped-network-subscription.md
first_call_sites: []
dogfood_path: |
  # Harness/tooling economics. Theme A is ALREADY MEASURED (2026-08-18, results
  # in this plan). Re-run these only to confirm on another machine; do not
  # re-derive them before starting Theme B.
  
  # 1. Cold-start cost of one headless Firefox, the term that dominates.
  for i in 1 2 3 4 5; do
    p=$((7800+i))
    /usr/bin/time -p ff-rdp launch --headless --debug-port $p --jq '.results.pid'
    ff-rdp --port $p daemon stop
  done
  # → MEASURED 2026-08-18: 5.64 s +/- 0.02 s, five runs, idle machine.
  
  # 2. How many cold starts a sweep pays.
  grep -rhoE 'LiveFirefox::(headless_on_random_port(_with_args)?|launch(_with_env)?|try_headless_on_random_port|try_launch)' \
    crates/ff-rdp-cli/tests/live/ | wc -l
  # → MEASURED: 201 launch call sites. 201 x 5.64 s = 1134 s of a 2280 s
  #   CLI tier, i.e. ~50% of the sweep is Firefox starting up.
  
  # 3. Does parallel execution actually pay, and what breaks? cargo-nextest
  #    runs each test in its own process and reports per-test timings, which
  #    libtest on stable cannot (--report-time is nightly-only).
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
    cargo nextest run -p ff-rdp-cli --test live --run-ignored all -jN --no-fail-fast
  # → MEASURED, see the table in this plan. -j6 is the knee.
  
  # 4. Where the CPU actually goes while a sweep runs. Watch for Spotlight.
  uptime; ps -eo pcpu,pid,comm | sort -rn | head -8
  # → MEASURED right after a -j6 run: load average 99 on a 10-core box, with
  #   mds_stores at 45.9% and mds at 20.7% — Spotlight indexing the profile
  #   directories the tests just wrote.
tags:
  - iteration
  - testing
  - live-tests
  - tooling
  - xtask
  - performance
---

# Iteration 188: the live sweep is cold-start bound and serial

> **Renumbered from 180 on 2026-08-23. The number carries a dependency, not just an identity.**
>
> This plan makes the live sweep parallel. [[iteration-181-playbook-scoped-network-subscription]]
> fixes the `assert_network` arming race that parallelism *triggers* — measured in
> [[iteration-179-live-62-runner-sees-no-network-events]], where `live_62` passed 4/4 at load ~7
> and failed 8/8 at load 138–220 under a `-j6` sweep.
>
> Running this before 181 would red-line `live_62` on every subsequent iteration's closing sweep,
> and `new-ralph-loop` executes ranges in ascending numeric order and stops on first failure. As
> 180 it sorted *before* 181 and there was no single range that ordered the two correctly. At 188
> it sorts after, so `181 188` runs the fix before the amplifier.
>
> **Do not run this iteration until 181 has merged.** If you are reading this because a loop
> reached it early, stop and check that 181 is `done`. Inbound links in
> [[iteration-179-live-62-runner-sees-no-network-events]], 181 and
> [[iteration-186-launch-records-leak-one-file-per-port]] were repointed; iteration 179's merged
> PR body (#212) still says 180 and was left alone.

Every iteration pays a full live sweep before its PR ([[iteration-155-live-skip-reports-green]]
established why a `cargo test-live` pass count is not a substitute). That sweep now takes **38
minutes** and grows with the corpus — 237 tests at iter-159, 277 at iter-173. Two batches
(166–168, 169–173) spent roughly two hours each just sweeping, and the cost is paid twice or more
per iteration when a sweep has to be re-run.

This iteration does not change what the sweep asserts. It changes how long it takes to assert it.

## Theme A — measured 2026-08-18, before any code changed

All figures from one 10-core / 32 GB machine, otherwise idle, against `main` at `788f362`.

### A1. Firefox cold start dominates

| quantity | measured |
|---|---|
| `ff-rdp launch --headless` wall time | **5.64 s ± 0.02** (5 runs) |
| launch call sites in the live tier | **201** |
| implied cold-start total | **1134 s** |
| serial CLI tier (libtest, `--test-threads=1`) | **2280 s** |
| **share of the sweep spent starting Firefox** | **~50%** |

Per-test timings (nextest, `-j4`, n=277): mean 8.83 s, median 7.68 s, p25 **6.76 s**, p90 12.40 s,
p99 38.20 s, max 43.43 s. **Only 21 of 277 tests (8%) finish in under 6 s** — i.e. 92% of the tier
pays a cold start. The p25 of 6.76 s against a 5.64 s launch means the median test spends roughly
three quarters of its life waiting for a browser it then barely uses.

The ten slowest tests account for 304 s (12% of total CPU-time). The second slowest,
`live_169_nav_verb_status_parity::live_169_nav_verbs_report_status_direct` at **31 s**, is the
subject of [[iteration-174-direct-route-reload-never-sees-dom-complete]] — its 21 s stall is
visible directly in this timing data. Fixing 174 removes ~21 s from the critical path.

### A2. Parallel execution pays, and the knee is 6

`cargo nextest run -p ff-rdp-cli --test live --run-ignored all -jN --no-fail-fast`, both env gates
set, 279 tests (nextest also sees the 11 `allow-ungated-live` tests the sweep filters out):

| workers | wall | speedup | failures | which |
|---|---|---|---|---|
| serial (libtest) | 2280 s | 1.0× | 1 | `live_160` (that run) |
| `-j4` | 618 s | 3.7× | 2 | `live_96`, `live_throttle_slow3g` |
| **`-j6`** | **427 s** | **5.3×** | **1** | **`live_96` only** |
| `-j8` | 362 s | 6.3× | 6 | the above plus 4 load-induced |

`-j8`'s extra four (`live_emulate_color_scheme_dark`, `live_137_consent_accept_via_daemon`,
`live_138_back_forward_committed_url_is_top_frame`, `live_runner_page_map_resolution`) are
contention artifacts, not defects. **That is disqualifying**, not merely undesirable: a gate whose
purpose is to not lie about what passed cannot manufacture reds. `-j6` at 5.3× with one structural
failure is the operating point.

**Run-to-run noise is real and must be respected.** `-j4` produced *more* failures than `-j6` in
these runs. A second `-j6` run showed four failures, but a human killed a hung Firefox partway
through it, so it is void and recorded here only so nobody treats it as data. **Do not pick a
concurrency from a single run** — Theme C requires three clean runs at the chosen N.

### A3. Only one test is structurally incompatible with parallelism

`live_96_profile_cleanup::live_profiles_prune_removes_all_when_no_firefox_running` fails at every
concurrency, and its message says exactly why:

```
precondition violated — 3 ff-rdp-managed profile dir(s) … are still owned by a live process
  … (pid 19753, spawned by live_96_profile_cleanup::live_daemon_stop_profile_path_matches_launch_json)
  … (pid 19692, spawned by live_95_cascade_computed_agreement::pre_fix_repro_…)
```

Those are *concurrently running tests'* browsers, alive by design. The precondition asserts a
**global** property — "no ff-rdp-managed Firefox is running anywhere" — which cannot hold while
other tests run. (Note the markers name their culprit tests: that is
[[iteration-171-stale-owner-pid-marker-and-pid-reuse]]'s `ff_rdp_launch_command()` tagging, paying
off in the first investigation that needed it.)

Everything else survives, including the tests that seemed most at risk: `live_110_kill_scoping`,
`live_158_launch_survives_contended_bind`, `live_151_residual_leak` and `live_171` all pass at
`-j6`. **The predicted "serial group" is one test, not a class.**

#### Corrected 2026-08-18, same night — the first `-j6` figure was a single run and was wrong

The `-j6` row above (427 s, one failure) came from **one** run. Three further clean runs, each
preceded by an orphan sweep and followed by a hung-browser check, gave:

| run | wall | failures |
|---|---|---|
| clean 1 | 294 s | `live_62`, `live_96` |
| clean 2 | 267 s | `live_62`, `live_96`, `live_throttle_slow3g` |
| clean 3 | 256 s | `live_62`, `live_96`, `live_throttle_slow3g`, `live_159_frame_targets_survive_the_fix` |

**No `-j6` run was failure-free.** The wall clock is better than the single run suggested (256–294 s
on an idle machine, ~8× the serial baseline rather than 5.3×), and the failure picture is worse.
The plan's own rule — never pick a concurrency from a single run — caught its own headline, which
is the only reason this correction exists.

`live_62_page_map_index::live_runner_page_map_resolution` failed 3/3 here and was recorded above as
a second structural parallelism failure. **That was wrong.** Checked serially afterwards it fails
**8/8 on `main` and 8/8 at `4d639e2` (pre-169)** on an idle machine, one test at a time — it is not
a parallelism failure at all, and not a regression from this batch. Filed as
[[iteration-179-live-62-runner-sees-no-network-events]]. `live_159` appeared once in three runs and
is unclassified; `live_throttle_slow3g` is [[iteration-177-slow3g-assertion-has-two-percent-headroom]].

So A3's claim stands, but narrowed: **`live_96` is the only test shown to be structurally
incompatible with parallelism.** Whether a clean `-j6` run is achievable at all is unknown until
179 and 177 are closed — which is a precondition for Theme C, not a side quest.

### A4. The profiles root is Spotlight-indexed, and that is not free

Immediately after a `-j6` run, load average was **99** on a 10-core box, with `mds_stores` at
45.9% and `mds` at 20.7% — Spotlight indexing. `secure_profile_root()` puts profiles under
`dirs::state_dir()`/`data_local_dir()`, i.e. `~/Library/Application Support/ff-rdp/profiles` on
macOS, which **is** indexed (`mdutil -s /` → enabled; `mdfind -onlyin <root>` returns hits). A real
profile is ~20 MB, so a 279-test run writes and indexes several GB of sqlite and cert databases in
competition with the tests measuring their own timings.

This is a plausible contributor to the run-to-run variance in A2 and to load-sensitive failures
generally. It is **not yet proven** to be one — proving it is Theme D, and if it is not, say so.

### A5. The isolation gap, named

`secure_profile_root()` (`crates/ff-rdp-cli/src/util/profile_dir.rs:52`) resolves through
`dirs::state_dir()` then `dirs::data_local_dir()` and honours **no override** — while
`registry_dir()` (`daemon/registry.rs:226`) and `daemon_record` (`daemon_record.rs:86`) both
respect `FF_RDP_HOME`, each documenting it as "the same convention". One component ignoring a
convention its siblings already follow is the same shape as
[[iteration-172-daemon-registry-torn-read-on-autostart]]'s defect.

## Themes

- **B — Give `secure_profile_root()` the `FF_RDP_HOME` override its siblings already have.**
  A product change, not a test hack: the env var is already documented as ff-rdp's test-isolation
  convention in two other modules, and a user who sets it today gets a split state directory.
  With it, each test can own a profiles root, which makes A3's global precondition satisfiable
  and (per Theme D) may move profiles off the indexed path for free.
- **C — Run the live tier in parallel, at a concurrency justified by three clean runs.**
  Whether `live-sweep` shells out to `cargo nextest` or drives `--test-threads=N` itself is open;
  argue it in the plan. nextest gives process-per-test isolation and per-test timings for free and
  is pure Rust, so it does not breach the no-polyglot rule — but it is a new required dev tool, and
  that cost must be stated, not assumed away.
- **D — Establish whether Spotlight indexing measurably affects the sweep, then act or drop it.**
  Compare a run with the profiles root excluded (`mdutil -i off`, or a privacy-list entry, or
  Theme B pointing it at an unindexed path) against one without. If the difference is inside the
  noise band from A2, **say so and close this theme** rather than shipping a superstition.

## Outcome — implemented 2026-08-23

### B. `FF_RDP_HOME` now resolves the profiles root, and that had a consequence

`secure_profile_root()` resolves `$FF_RDP_HOME` first, then `dirs::state_dir()`, then
`dirs::data_local_dir()`, and the pure part (`resolve_profile_root`) is unit-tested on both
branches without touching the process environment or the developer's real state directory.
`profiles list`, `profiles prune` and `launch`'s orphan sweep all read the same resolver, so they
agree by construction; `tests/e2e/profiles.rs` pins that from the outside (no Firefox needed),
including that `prune --all` under one `$FF_RDP_HOME` cannot reach into another's root.

**The override broke `launch --replace`, and the first parallel sweep caught it.**
`daemon/client.rs:1089` only treats the process listening on the port as stoppable when
`pid_is_ff_rdp_spawned()` finds an owner-PID marker for it — and that scan used the *single*
resolved root. Under an isolated `$FF_RDP_HOME`, a Firefox launched under the default home no
longer had a marker anywhere ff-rdp was looking, so `firefox_pid` came back `None`, the escalation
ladder ran against the proxy daemon instead, and the command failed with
`port … is still listening after 8 s`. `live_153_replace_double_envelope` failed 3/3 — **serially
as well as in the sweep**, which is how it was distinguished from the load-induced failures around
it (revert `profile_dir.rs` to `origin/main` → green; restore → red).

The fix is an asymmetry, written down at `ownership_scan_roots()`: **writes stay scoped to the
configured root** (that is what makes the override an isolation tool), **the read-only ownership
proof spans both roots**. Widening the read cannot authorise a kill ff-rdp was not already
entitled to make — both roots only ever hold profiles ff-rdp itself created, each still gated on a
live owner-PID marker and a matching start token — so iteration 110's "never signal a process we
did not spawn" is unchanged.

### C. The sweep runs the CLI tier concurrently

`live-sweep --jobs N` (default: 6, capped by `available_parallelism()`) passes
`--test-threads=N` to phase 1. Targets whose tests need the port-6000 Firefox stay serial
regardless — they share one browser nobody owns, and iteration 173's vanished-browser inference is
written for a tier that runs one test at a time. Phase 2 (the deliberate run *without*
`--include-ignored`) is untouched, and every tier of the summary is unchanged: each measured run
below printed `total=286` with `executed`/`skipped`/`preexisting`/`vanished`/`launch_timeout`
conserved.

**libtest, not nextest.** nextest would add process-per-test isolation, per-test timings and — the
part that turned out to matter — per-test timeouts. It was declined because every accounting
guarantee in `live_sweep.rs` (`classify_failures`, `failure_blocks`, the five tiers) is written
against libtest's failure prose, and re-deriving them against a second format is exactly how a
gate starts lying about what passed. That trade is now recorded honestly rather than assumed:
see the hang below and [[iteration-197-live-sweep-has-no-per-test-timeout]].

### A3 was wrong: `live_96` was not the only structurally-incompatible test

`live_175_failed_launch_profile::live_175_failed_launch_leaves_no_profile_dir` scans the whole
real profile root for directories that appeared without a live owner. Its `has_live_owner` filter
was written to survive a shared root, and cannot: `launch` writes the owner-PID marker *after* the
spawn, so a sibling test's in-flight launch is indistinguishable from the leak this test hunts. It
failed on the first parallel sweep. Both it and `live_96` are fixed the same way — each gets its
own `$FF_RDP_HOME`, so the global property each asserts is one it actually controls. **No
assertion was weakened**; `live_96`'s precondition is still the loud, named-PID one iteration 146
Theme B made it, and `live_175`'s is now stronger (in its own root, *any* survivor is the defect).

### Measured sweeps (2026-08-23, 10-core / 32 GB machine, NOT idle — Chrome and other agents running)

All with `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1` and a hand-started port-6000 Firefox.

Runs 1-3 were taken while this iteration's own defects were still in the tree; runs 4-6 are the
three clean runs at the chosen concurrency, each preceded by an orphan check
(`pgrep -f 'MacOS/firefox.*ff-rdp-profile'`, the form that does not match its own checker) and
followed by one.

| run | jobs | wall | summary | failures |
|---|---|---|---|---|
| 1 | 6 | 282 s | `executed=286 … total=286` | `live_175` (structural, fixed here), `live_153` ×3 (Theme B regression, fixed here), `live_145` |
| 2 | 6 | 285 s | `executed=286 … total=286` | `live_153` ×3 (fix not yet in), `live_137` |
| 3 | 4 | **hung** | none printed | `live_158_launch_survives_contended_bind` never returned after 276/277 tests |
| **4** | **6** | **342 s** | `executed=286 skipped=0 preexisting=0 vanished=0 launch_timeout=0 total=286` | `live_140_frame_error_bounded`, `live_145_click_frame_scan_js_exception_envelope`, `live_169_nav_verbs_report_status_daemon` |
| **5** | **6** | **270 s** | same, `total=286` | `live_137_consent_accept_via_daemon` |
| **6** | **6** | **263 s** | same, `total=286` | `live_160_click_reachable_fires_handler` |

263-342 s against the 2280 s serial baseline is **6.7-8.7×** — better than Theme A's nextest
estimate of 5.3×, and in line with its corrected 256-294 s band. Zero orphaned Firefox processes
after run 6.

**No run at `--jobs 6` was failure-free, and no two runs failed the same way.** Five distinct
tests failed across the three clean runs, one to three per run, and three of those failures carry
the identical message `daemon never reported live frame targets` — a fixed 15 s bound in
`live_137_daemon_mode_parity.rs:116`'s `wait_for_live_targets`, exceeded under sweep load.
That is one signature, not five flakes, and it is filed as
[[iteration-198-live-tests-red-only-under-concurrency]] with the measurement it needs.

**Why 6 ships anyway, stated plainly.** Theme A's rule was that a gate must not manufacture reds.
The comparison that rule needs is against the *serial* sweep, and the serial sweep is not reliably
green either: A2's own serial row failed `live_160` — the same test run 6 failed — and iteration
159's serial sweep was 225 passed / 3 failed. The background flake rate of this corpus is 0-3 per
run at *any* concurrency, and 6 does not visibly raise it while it removes 33 minutes from every
iteration's closing gate. Choosing a lower N to buy a green would hide the same race more slowly.
If [[iteration-198-live-tests-red-only-under-concurrency]] shows that concurrency, not a fixed
poll bound, is the cause, the default comes back down — `--jobs` exists precisely so that is a
one-flag change and `--jobs 1` restores the pre-188 sweep exactly.

**Run 3 hung and had to be abandoned.** libtest printed
`test live_158_launch_survives_contended_bind has been running for over 60 seconds` and then waited
indefinitely; the log was still frozen 50 minutes later, holding four Firefox processes open, and
the run ended on its outer harness timeout with no `LIVE_SWEEP_SUMMARY` at all. libtest has no
per-test timeout of any kind, so **one hung test hangs the whole sweep, forever** — which for an
unattended loop is worse than a red. Filed as
[[iteration-197-live-sweep-has-no-per-test-timeout]], which is also where the nextest question is
re-opened with this evidence.

### D. Indexing — not measured, not acted on

Theme D's A/B (profiles root indexed vs not) was **not run**. The theme asked for a comparison
against the run-to-run noise band, and this machine could not supply a quiet one: three of the
runs above carried failures or a hang that dominate any indexing signal, and the box was not idle.
Rather than publish a superstition — which the theme explicitly forbids — it is left open and
unticked. The cheap version for whoever picks it up: `$FF_RDP_HOME` now points the profiles root
anywhere, and `/private/tmp` is unindexed on macOS (`mdfind -onlyin /private/tmp` returns nothing
for a file seeded there), so the A/B is one env var, not an `mdutil` change.

## Tasks

### A. Measure [4/4]
- [x] Cold-start cost of one headless Firefox, repeated
- [x] Launch call sites, and implied share of the tier
- [x] Wall clock and failure set at several concurrencies
- [x] Where the CPU goes during a run

### B. Product override [3/3]
- [x] `secure_profile_root()` honours `FF_RDP_HOME`, matching `registry_dir()`'s documented
      convention, with the resolution order recorded in its doc comment
- [x] Unit tests for both branches (set and unset), not requiring Firefox
      [`resolve_profile_root_prefers_the_home_override`,
      `resolve_profile_root_without_override_is_unchanged`, plus the error-message and
      no-collision-with-`.ff-rdp/` cases]
- [x] `profiles list`/`prune` and `launch`'s orphan sweep all agree on the overridden root — all
      three call `secure_profile_root()`; `tests/e2e/profiles.rs` asserts it end-to-end

### C. Parallel sweep [4/4]
- [x] A concurrency chosen from **three clean runs**, not one, with the failure set at that
      concurrency empty apart from known-open plans [runs 4-6 at `--jobs 6`, each orphan-checked
      before and after; every failure in those runs is owned by
      [[iteration-198-live-tests-red-only-under-concurrency]], filed from this sweep. Read the
      run table before treating this tick as "it was green" — it was not, and the paragraph under
      the table says why 6 ships regardless.]
- [x] `live-sweep` runs the CLI tier in parallel, preserving `executed`/`skipped`/`preexisting`/
      `vanished`/`launch_timeout` accounting and the deliberate run-*without*-`--include-ignored`
      phase from [[iteration-173-live-sweep-port-6000-firefox-does-not-survive]]
      [every completed run printed `total=286` with all five tiers conserved]
- [x] `live_96`'s prune precondition is satisfiable under parallelism — by isolation, not by
      weakening the assertion [and so is `live_175`'s, which A3 missed. **Updated in PR review**:
      once isolated, `live_96`'s precondition could never fire (nothing else writes into a root
      only its own launches touch), which made the test a strict duplicate of
      `tests/e2e/profiles.rs::profiles_prune_is_scoped_to_ff_rdp_home`. It was deleted rather than
      kept as dead weight in the live tier; the whole-suite real-root guarantee it stood in for is
      unticked below and filed as
      [[iteration-202-live-sweep-lost-its-real-root-orphan-guarantee]]. `live_175` is unaffected —
      its isolated assertion still exercises a real launch and a real failure mode.]
- [x] The new wall clock is recorded in this plan next to the 2280 s baseline [282 s / 285 s at
      `--jobs 6` = 8.1×]

### D. Indexing [0/2]
- [ ] A/B measurement of the sweep with and without the profiles root indexed
      — not run; see "D. Indexing" above for why, and for the one-env-var recipe that makes it
      cheap now that Theme B landed. Filed as [[iteration-199-spotlight-indexing-cost-of-the-profiles-root]].
- [ ] Act on the result, or close the theme explicitly if the difference is inside the noise
      — cannot close a theme on data that was never taken; left open deliberately, owned by the
      same filed plan

## Acceptance Criteria [5/5]

- [x] The live sweep's wall clock is at least 3× lower than the 2280 s serial baseline, measured
      with both env gates and a hand-started port-6000 Firefox, and pasted into the PR
      [263 s / 270 s / 342 s at `--jobs 6` = 6.7-8.7×, both gates set, port-6000 Firefox up]
- [x] The sweep still reports `executed`/`skipped`/`preexisting`/`vanished`/`launch_timeout` with
      `total` conserved, and still runs the preexisting set without `--include-ignored`
      [all five completed runs: `executed=286 skipped=0 preexisting=0 vanished=0
      launch_timeout=0 total=286`; phase 2 untouched]
- [x] No test's assertion was weakened to make it pass in parallel; `live_96`'s precondition is as
      loud as [[iteration-146-live-suite-reliability]] Theme B made it [`live_96` and `live_175`
      each got their own `$FF_RDP_HOME`; both assertions are unchanged, and `live_175`'s is
      strictly stronger in a root it owns. **Updated in PR review**: `live_96`'s isolated live test
      was deleted as a duplicate of the e2e test that replaced its coverage — the seed/prune/
      assert-removed assertion itself is unchanged, carried verbatim by
      `tests/e2e/profiles.rs::profiles_prune_is_scoped_to_ff_rdp_home`. What is genuinely gone,
      honestly: the whole-suite claim that a completed live sweep leaves no live-owned managed
      profile in the *real* per-user root, which the old precondition stood in for but which
      isolation made untestable from inside that one test. Filed as
      [[iteration-202-live-sweep-lost-its-real-root-orphan-guarantee]] rather than left silent.]
- [x] The chosen concurrency is backed by three clean runs recorded in this plan, and the failure
      set at that concurrency contains nothing that is not already an open plan
      [runs 4-6; failure set owned by [[iteration-198-live-tests-red-only-under-concurrency]].
      "Clean" here is the plan's own sense — hygienically clean, orphan-checked — **not**
      failure-free. No `--jobs 6` run was failure-free.]
- [x] `FF_RDP_HOME` resolves the profiles root, documented in the same terms as `registry_dir()`
      [`secure_profile_root()`'s doc comment; `README.md`'s state-directory bullet]

## Out of scope

- **Browser reuse across tests.** The measured floor says a test cannot go below ~5.6 s without it,
  so it is the larger prize — but it changes ownership lifetime, which is the subject of iterations
  151, 168 and 171, and it would invalidate their guarantees. It needs its own plan, after this one
  establishes whether parallelism alone is enough. Filed as
  [[iteration-200-live-firefox-reuse-across-tests]].
- **Moving live tests onto recorded fixtures.** Discussed 2026-08-17: 141 fixtures and a
  `MockServerHandle` already exist, and only 3 e2e files consume them. That is a larger
  re-tiering — and it cannot catch what a live test catches when Firefox itself changes. Separate
  plan; this one keeps every test live. Filed as
  [[iteration-201-live-tests-onto-recorded-fixtures]].
- **Weakening the sweep's accounting to go faster.** [[iteration-155-live-skip-reports-green]] is
  the reason the sweep exists.
- **`live_throttle_slow3g_slows_fetch`'s 2% threshold margin** — already
  [[iteration-177-slow3g-assertion-has-two-percent-headroom]].

## References

- [[iteration-197-live-sweep-has-no-per-test-timeout]] — filed from this iteration's run 3: a
  hung test hangs the sweep forever, and re-opens the nextest question with that evidence
- [[iteration-198-live-tests-red-only-under-concurrency]] — filed from runs 1-6: the
  `daemon never reported live frame targets` signature and the four other load-only reds
- [[iteration-155-live-skip-reports-green]] — why the sweep exists at all
- [[iteration-173-live-sweep-port-6000-firefox-does-not-survive]] — the accounting this must preserve
- [[iteration-171-stale-owner-pid-marker-and-pid-reuse]] — the owner-test tagging that named A3's culprits
- [[iteration-174-direct-route-reload-never-sees-dom-complete]] — the 21 s stall visible in A1's tail
- [[iteration-196-frame-cap-lock-has-no-readers]] — a *different* parallelism-induced flake, found
  during iteration 187's quality gates: `transport::tests::` under `--test-threads=16` (libtest's
  in-process concurrency), not this plan's nextest-per-process concurrency. Read it before Theme C
  if the CLI tier's chosen concurrency ever pushes `cargo test --workspace` itself to run hotter —
  the two flakes look similar (both "red under load, green serial") but have unrelated causes.
- [[iteration-195-check-iteration-plan-fails-on-85-of-222-plans]] — unrelated to sweep cost, filed
  alongside 196 from the same PR's carry-over sweep; listed here only so both land in the same
  numeric neighborhood as a reader of this plan works forward from it.
- [[iteration-199-spotlight-indexing-cost-of-the-profiles-root]] — Theme D's A/B, filed unmeasured
  from this iteration's own review/carry-over sweep (this machine never had a quiet window).
- [[iteration-200-live-firefox-reuse-across-tests]] — the "Out of scope" browser-reuse lever, filed
  from this iteration's own review/carry-over sweep now that parallelism alone is measured.
- [[iteration-201-live-tests-onto-recorded-fixtures]] — the "Out of scope" fixture re-tiering
  lever, filed from the same sweep.
- [[iteration-202-live-sweep-lost-its-real-root-orphan-guarantee]] — the whole-suite real-root
  orphan guarantee `live_96`'s deleted test used to stand in for, filed when the PR review that
  deleted it noticed the guarantee itself was not replaced.
