---
title: "Iteration 180: the live sweep spends half its wall clock on Firefox cold starts and runs them one at a time"
type: iteration
date: 2026-08-18
status: planned
branch: iter-180/live-sweep-parallelism
depends_on: []
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
tags: [iteration, testing, live-tests, tooling, xtask, performance]
---

# Iteration 180: the live sweep is cold-start bound and serial

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

## Tasks

### A. Measure [4/4]
- [x] Cold-start cost of one headless Firefox, repeated
- [x] Launch call sites, and implied share of the tier
- [x] Wall clock and failure set at several concurrencies
- [x] Where the CPU goes during a run

### B. Product override [0/3]
- [ ] `secure_profile_root()` honours `FF_RDP_HOME`, matching `registry_dir()`'s documented
      convention, with the resolution order recorded in its doc comment
- [ ] Unit tests for both branches (set and unset), not requiring Firefox
- [ ] `profiles list`/`prune` and `launch`'s orphan sweep all agree on the overridden root

### C. Parallel sweep [0/4]
- [ ] A concurrency chosen from **three clean runs**, not one, with the failure set at that
      concurrency empty apart from known-open plans
- [ ] `live-sweep` runs the CLI tier in parallel, preserving `executed`/`skipped`/`preexisting`/
      `vanished`/`launch_timeout` accounting and the deliberate run-*without*-`--include-ignored`
      phase from [[iteration-173-live-sweep-port-6000-firefox-does-not-survive]]
- [ ] `live_96`'s prune precondition is satisfiable under parallelism — by isolation, not by
      weakening the assertion
- [ ] The new wall clock is recorded in this plan next to the 2280 s baseline

### D. Indexing [0/2]
- [ ] A/B measurement of the sweep with and without the profiles root indexed
- [ ] Act on the result, or close the theme explicitly if the difference is inside the noise

## Acceptance Criteria [0/5]

- [ ] The live sweep's wall clock is at least 3× lower than the 2280 s serial baseline, measured
      with both env gates and a hand-started port-6000 Firefox, and pasted into the PR
- [ ] The sweep still reports `executed`/`skipped`/`preexisting`/`vanished`/`launch_timeout` with
      `total` conserved, and still runs the preexisting set without `--include-ignored`
- [ ] No test's assertion was weakened to make it pass in parallel; `live_96`'s precondition is as
      loud as [[iteration-146-live-suite-reliability]] Theme B made it
- [ ] The chosen concurrency is backed by three clean runs recorded in this plan, and the failure
      set at that concurrency contains nothing that is not already an open plan
- [ ] `FF_RDP_HOME` resolves the profiles root, documented in the same terms as `registry_dir()`

## Out of scope

- **Browser reuse across tests.** The measured floor says a test cannot go below ~5.6 s without it,
  so it is the larger prize — but it changes ownership lifetime, which is the subject of iterations
  151, 168 and 171, and it would invalidate their guarantees. It needs its own plan, after this one
  establishes whether parallelism alone is enough.
- **Moving live tests onto recorded fixtures.** Discussed 2026-08-17: 141 fixtures and a
  `MockServerHandle` already exist, and only 3 e2e files consume them. That is a larger
  re-tiering — and it cannot catch what a live test catches when Firefox itself changes. Separate
  plan; this one keeps every test live.
- **Weakening the sweep's accounting to go faster.** [[iteration-155-live-skip-reports-green]] is
  the reason the sweep exists.
- **`live_throttle_slow3g_slows_fetch`'s 2% threshold margin** — already
  [[iteration-177-slow3g-assertion-has-two-percent-headroom]].

## References

- [[iteration-155-live-skip-reports-green]] — why the sweep exists at all
- [[iteration-173-live-sweep-port-6000-firefox-does-not-survive]] — the accounting this must preserve
- [[iteration-171-stale-owner-pid-marker-and-pid-reuse]] — the owner-test tagging that named A3's culprits
- [[iteration-174-direct-route-reload-never-sees-dom-complete]] — the 21 s stall visible in A1's tail
