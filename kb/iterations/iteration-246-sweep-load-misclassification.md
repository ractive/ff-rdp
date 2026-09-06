---
title: "Iteration 246: live tests red only under sweep load: classification, live_123, live_61q, and the --jobs 6 signatures"
type: iteration
date: 2026-08-30
status: planned
branch: iter-246/sweep-load-misclassification
depends_on:
  - 225
  - kb/iterations/iteration-188-live-sweep-cost-and-parallelism.md
first_call_sites:
  - primitive: (none — test-only change; no new pub item)
    site: crates/ff-rdp-cli/tests/live/live_123_daemon_autostart_and_registry.rs
dogfood_path: |
  # 1. The launch-timeout misclassification, reproduced under load.
  FF_RDP_LIVE_TESTS=1 cargo run -p xtask -- live-sweep 2>&1 | tee /tmp/sweep.log
  grep -E "LIVE_SWEEP_SUMMARY|did not open debug port" /tmp/sweep.log
  # expected AFTER this iteration: a test whose Firefox never opened its debug port is
  # counted in launch_timeout=N and named there — never as a bare `FAILED` line whose only
  # evidence is an error envelope buried in the test's stdout
  #
  # 2. The nested-cargo flake, reproduced by running the outer suite in parallel.
  cargo test --workspace -q
  cargo test --workspace -q
  # expected AFTER this iteration: check_firefox_refs::valid_in_range_ref_passes passes both
  # times; today it fails intermittently because it shells out to `cargo run -p xtask`
  # while the outer `cargo test` holds the build lock
  # --- from iteration 249 ---
  firefox -no-remote --start-debugger-server 6000 --headless   # raw browser, NOT ff-rdp launch
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
  # expected: 0 failures. The 2026-08-30 sweep on iter-220 reported
  #   live_123 …targets_debug_port_not_cli_port FAILED  ("eval on decoy port should succeed")
  # while the same test passed in isolation and in the immediately preceding sweep.
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live \
    live_daemon_stop_prior_instance_targets_debug_port_not_cli_port -- --include-ignored
  # expected: ok — it always is alone. That gap is the thing to close.
  # --- from iteration 250 ---
  # Reproduce under contention — the failure does not appear in isolation.
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
  # 2026-08-31 (iter-225's first sweep, 311 CLI-tier tests at --test-threads=6):
  #   live_61q_resource_bus::live_resource_dedupe ... FAILED
  #   panicked at crates/ff-rdp-cli/tests/live/live_61q_resource_bus.rs:258: subscribe A: Timeout
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live -q \
    live_61q_resource_bus::live_resource_dedupe -- --include-ignored --test-threads=1
  # ok in 2.6s — which is exactly why this needs its own plan rather than a re-run
  # --- from iteration 251 ---
  # Both tests pass serially and pass in most concurrent runs, so the only
  # honest reproduction is repetition at the sweep's own concurrency.

  # 1. Serial control — expected green:
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live \
    live_137_consent_accept_via_daemon -- --include-ignored --test-threads=1
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo test -p ff-rdp-cli --test live \
    live_145_click_frame_scan_js_exception_envelope -- --include-ignored --test-threads=1

  # 2. Under the real sweep, repeated. Each of these failed once across
  #    iteration 188's parallel sweeps (2026-08-23, --jobs 6, 10-core machine):
  #      live_145_error_envelope_completeness::live_145_click_frame_scan_js_exception_envelope
  #        "daemon never reported live frame targets"
  #      live_137_daemon_mode_parity::live_137_consent_accept_via_daemon
  for i in 1 2 3; do
    FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep --jobs 6
  done
  # Record the failure set per run. A fix means three runs with neither name in it.
tags: [iteration, live-tests, test-reliability, xtask, daemon, carry-over, flake, resource-bus, testing, flaky]
---

# Iteration 246: live tests red only under sweep load: classification, live_123, live_61q, and the --jobs 6 signatures

> **Renumbered 216 → 246 on 2026-09-06** so the pending queue runs as one contiguous sweep (DEC-051). Older PRs, commits and sweep logs cite it as iteration 216.

> **Scope note (2026-09-06):** Theme D's `live_61q_live_resource_dedupe` row duplicates iteration 250, which runs after this plan and owns that test. Leave that row to 250 — now Part C of this plan. AC 5 (two consecutive sweeps failing on the same set or none) assumes 235/236 have landed; they precede this plan in the run.

> **Merged 2026-09-06 (DEC-051 addendum):** absorbs [[iteration-249-live-123-daemon-autostart-under-load]] as Part B, [[iteration-250-resource-bus-subscribe-timeout]] as Part C and [[iteration-251-live-tests-red-only-under-concurrency]] as Part D. One branch, one PR, one carry-over sweep for all parts. Order of work inside the PR: A (classification) first, then B/C (the per-test attributions), then D's sweep-level verdict, since D's clean-sweep ACs presuppose the others.

## Part A: load-sensitive live tests report as product defects

Found by [[iteration-211-find-not-guess]]'s closing live sweeps. It ran the sweep **twice** —
gates `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1` both times, on trees that differ only by
three test fixes — and both runs reported the same score against a **different** set of failures:

| run | summary | result | failures |
| --- | --- | --- | --- |
| 1 | `executed=297 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=297` | 283 passed / 5 failed | `live_102_longstring_roundtrip`, `live_211_page_text_query…`, `live_166` ×2, `live_186_launch_record_growth_bounded` |
| 2 | `executed=297 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=297` | 283 passed / 5 failed | `live_166` ×2, `live_100_e2e_sigterm_removes_registry`, `live_175_failed_launch_leaves_no_profile_dir`, `live_61q_live_resource_dedupe` |

The first two of run 1 were iter-211's own business (a `page-text` cap that a pre-existing test
had not been told about, and a fixture whose `<p>`-per-line HTML made `innerText` line numbers
disagree with source line numbers); both are closed in that PR, and both passed in run 2. The
`live_166` pair is [[iteration-236-live-166-cache-304]], already filed.

What is left is **four tests that failed in one run and passed in the other**, none of which
touches the surface iter-211 changed. That is the subject of this plan. One green run is not
evidence a load-sensitive test is fine — it is one sample — and the two runs here disagree about
which four tests they are.

### The failures

**1. `live_186_launch_record_gc::live_186_launch_record_growth_bounded`** (run 1; passed run 2)

```
`ff-rdp launch --headless --debug-port 62722` exited exit status: 1
  stdout: {"error":"Firefox (pid 76761) did not open debug port 62722 within 30s —
           raise --launch-timeout or set FF_RDP_LAUNCH_TIMEOUT_SECS","error_type":"User"}
```

This is exactly the shape iter-173 carved `launch_timeout=L` out of `executed` for: Firefox never
opened its debug port within the per-test budget under sweep load, which is an unmet
precondition, not a product failure. But the sweep reported `launch_timeout=0` and the test
reported a plain `FAILED`, because the classifier only recognises the timeout when the *harness*
raises it — here the timeout came from `ff-rdp launch` running **inside** the test, so it reaches
libtest as an ordinary assertion failure with the diagnosis sitting in captured stdout.

The consequence is the one the whole classification exists to prevent: a reader triaging
`283 passed / 5 failed` has to open each failure and read its stdout to find out that one of them
is the machine being busy. `launch_timeout` is meant to make that legible from the summary line.

**2. Three more, all timeout-shaped, all from run 2 only**

```
live_100_daemon_lifecycle_hardening::e2e_sigterm_removes_registry
  autostart_daemon: eval failed:
  e2e_sigterm_removes_registry: daemon never reported a pid

live_175_failed_launch_profile::live_175_failed_launch_leaves_no_profile_dir
  a launch that failed waiting for the debug port left 1 profile directory behind:
  ["ff-rdp-profile-E0IitROGtQDhVDZu"]

live_61q_resource_bus::live_resource_dedupe
  subscribe A: Timeout
```

Same family as #1: a wait that is generous when the machine is idle and not generous under a
297-test sweep. `live_175` is the interesting one — it asserts that a *failed* launch cleans up
its profile directory, and under load the launch failed and the cleanup did not finish, so the
test is reporting a real (if load-triggered) leak rather than only a slow wait. It should be
triaged separately from the other two.

**3. `xtask` `check_firefox_refs::valid_in_range_ref_passes`**

Failed once during `cargo test --workspace -q`, with an empty stderr, and passed on an immediate
isolated re-run:

```
thread 'valid_in_range_ref_passes' panicked at crates/xtask/tests/check_firefox_refs.rs:105:5:
expected success for in-range ref; stderr:
```

The test shells out to `cargo run -p xtask -- check-firefox-refs …` from inside a `cargo test`
that already holds the workspace build lock. The nested invocation can block, or observe a
half-written binary, and the assertion's failure message interpolates only `stderr` — which is
empty in exactly this case, so the message carries no evidence at all. **Not** flagged by
`iter_179_harness_stdout_evidence`, whose scan covers `crates/ff-rdp-cli/tests`, not
`crates/xtask/tests`.

Filed rather than dismissed because "flaky, passed on retry" is a diagnosis, not a disposition:
one green re-run is not evidence, and a test that cannot say why it failed will waste the next
reader's time the same way it wasted this one's.

### Themes

- **A — Classify an in-test launch timeout as `launch_timeout`.** The sweep already parses test
  output; teach it this error envelope so the count in the summary line is complete.
- **B — Stop `xtask` tests from shelling out into a locked build.** Either call the gate's
  library entry point directly, or give the nested invocation its own `CARGO_TARGET_DIR`.
- **C — Extend the stdout-evidence scan to `crates/xtask/tests`.** The rule ("every assertion
  naming `stderr` must name `stdout` too") is not ff-rdp-cli-specific; the scan's roots are.
- **D — Triage the four load-sensitive tests.** For each of `live_100`, `live_175`, `live_186`,
  `live_61q`, decide whether the fix is a longer wait, a wait on the right condition, or a real
  product bug the load exposed — and say which, per test. `live_175`'s leaked profile directory
  is the one most likely to be the third.

### Tasks

#### A. Launch-timeout classification [0/2]
- [ ] `live-sweep` recognises the `did not open debug port … within Ns` error envelope in a
      failing test's captured stdout and counts that test in `launch_timeout` rather than as a
      product failure
- [ ] The summary line names the affected tests, as `vanished` and `timed_out` already do

#### B. Nested-cargo isolation [0/2]
- [ ] `crates/xtask/tests/check_firefox_refs.rs` no longer races the outer build lock — call the
      check directly, or set a distinct `CARGO_TARGET_DIR` for the nested run
- [ ] Audit the other `xtask` integration tests for the same `cargo run` shape and fix them the
      same way

#### C. Evidence scan [0/1]
- [ ] `iter_179_harness_stdout_evidence`'s `scanned_roots` includes `crates/xtask/tests`, with a
      per-root floor like the existing ones

#### D. Triage the load-sensitive four [0/2]
- [ ] For `live_100_e2e_sigterm_removes_registry`, `live_175_failed_launch_leaves_no_profile_dir`,
      `live_186_launch_record_growth_bounded` and `live_61q_live_resource_dedupe`, record in this
      plan which of the three causes applies — and treat "flaky" as a starting point, not an
      answer
- [ ] Fix the ones that are waits; file the ones that are product bugs as their own plans

### Acceptance Criteria [0/5]

- [ ] A sweep run against a machine loaded enough to reproduce the launch timeout reports it in
      `launch_timeout=N`, not as a bare `FAILED` — captured as a real sweep summary in this
      plan's Outcome section
- [ ] `launch_timeout > 0` still fails the sweep (iter-173's rule is unchanged); only the
      classification and the naming change
- [ ] `cargo test --workspace -q` run three times consecutively passes three times, with
      `check_firefox_refs` green in all three
- [ ] `unit_179_no_assertion_reports_stderr_without_stdout` flags a deliberately-broken assertion
      planted under `crates/xtask/tests`, and the real tree is clean
- [ ] Two consecutive full sweeps with the same gates fail on the same set of tests or on none —
      the disagreement between iter-211's two runs is the defect being closed — satisfied by Part D AC 1; tick together

### Design notes

- **Do not raise the launch timeout to make Theme A's symptom go away.** The 30 s budget is the
  measurement; the defect is that exceeding it is filed under the wrong heading. Raising it hides
  the signal in exactly the way `FF_RDP_LAUNCH_TIMEOUT_SECS` exists to let a caller do
  *deliberately*.
- **Theme B is not "add a retry".** A retry would turn a lock race into a slower lock race and
  would leave the empty-stderr message in place. Removing the nested `cargo` is the fix.

### Out of scope

- The `live_166` HTTP 304 failures from the same sweep — already filed as
  [[iteration-236-live-166-cache-304]].
- Changing `live_186`'s own assertions. The test is measuring the right thing; the sweep is
  reporting its failure under the wrong heading.

### References

- [[iteration-211-find-not-guess]] — the two sweeps that found these
- [[iteration-236-live-166-cache-304]] — the other outstanding live-suite honesty item
- `crates/xtask/src/live_sweep.rs` — where `vanished`/`launch_timeout`/`timed_out` are classified
- `crates/ff-rdp-cli/tests/iter_179_harness_stdout_evidence.rs` — `scanned_roots`, Theme C

## Part B: live_123's decoy-port eval fails under sweep contention and the assertion says nothing about why (absorbed from iteration 249)

> **Renumbered 222 → 249 on 2026-09-06** so the pending queue runs as one contiguous sweep (DEC-051). Older PRs, commits and sweep logs cite it as iteration 222.

### Why

Carry-over from [[iteration-220-with-page-after-navigating-click]]'s closing sweep.

```
FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 → LIVE_SWEEP_SUMMARY
  executed=313 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=313
  301 passed / 3 failed (CLI tier)

live_123_daemon_autostart_and_registry::live_daemon_stop_prior_instance_targets_debug_port_not_cli_port
  panicked at live_123_daemon_autostart_and_registry.rs:320:
  eval on decoy port should succeed
```

Evidence on how load-dependent it is, all on the same commit:

| run | result |
|---|---|
| full sweep, 2026-08-30 21:5x | **FAILED** |
| full sweep, 2026-08-30 21:2x (three commits earlier, same test code) | ok |
| `cargo test … live_daemon_stop_prior_instance_targets_debug_port_not_cli_port` alone | ok, 8.1 s |

This is exactly the shape `kb/discipline-rationale.md` and the `iteration-close` skill warn
about: iter-153 shipped a broken feature certified by a truthful isolated run. A test that only
fails inside a sweep is not "environmental" — it is a test (or a product) that does not survive
contention, and the sweep is the only place anyone ever sees it.

### What the failure does and does not tell us

The test launches **two** `LiveFirefox` instances back to back and then runs `eval 1` against
each, autostarting a proxy daemon per port inside an isolated `FF_RDP_HOME`. Only the first
`eval` failed. `assert!(ok_decoy, "eval on decoy port should succeed")` asserts a **bool** — so
the record contains no exit code, no stderr, no envelope, and no way to tell apart:

- the daemon autostart lost a race under load (the likeliest reading — two Firefoxes plus ~300
  other live tests were competing for CPU),
- `eval` hit the direct-connection fallback and that failed,
- Firefox on the decoy port was not up yet despite `LiveFirefox` returning.

Nothing in the sweep log distinguishes them. That is the first thing to fix, because without it
the next occurrence is just as uninformative.

### Themes

- **A — Make the assertion say what happened.** `run_json` already has the `Output`; the panic
  should carry exit status, stdout and stderr, the way `live_210`'s `run_json` does. A red that
  names its cause is worth more than a red that is merely reproducible.
- **B — Then find the race.** With a real message, decide whether the fix belongs in the test
  (wait for the daemon the way `wait_daemon_running` already does, *before* the first `eval`
  rather than after) or in `eval`'s daemon-autostart path (a retry, or a longer
  `--daemon-timeout` under load). Do not guess before Theme A has produced one honest failure.
- **C — Do not "fix" it by loosening the assertion.** If the daemon autostart genuinely cannot
  survive a loaded machine, that is a product finding about `--daemon-timeout` defaults, and it
  belongs in the Outcome — not papered over with a retry loop in the test.

### Tasks

#### A. Honest failure text [0/1]
- [ ] `live_123`'s eval assertions report exit code, stdout and stderr on failure

#### B. Diagnose and fix [0/2]
- [ ] Reproduce under artificial load (run the suite with `--test-threads` raised, or alongside a
      CPU hog) and capture one real failure message
- [ ] Fix in the layer the message points at, and say which in the Outcome

#### C. Sibling tests [0/1]
- [ ] Check the other `live_123_*` tests (and `live_164`'s autostart tests) for the same
      bool-only assertion shape

### Acceptance Criteria [0/2]

- [ ] `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep` reports
      0 failures across **two consecutive** sweeps, both `LIVE_SWEEP_SUMMARY` lines pasted — satisfied by Part D AC 1; tick together
- [ ] A deliberately-broken daemon autostart makes `live_123` print the CLI's actual stderr,
      not just `eval on decoy port should succeed`

### Out of scope

- The two `live_166` reds from the same sweep — those are
  [[iteration-236-live-166-cache-304]] (filed as 221 by this sweep, reconciled into 214, now 236).

### References

- [[iteration-220-with-page-after-navigating-click]] — the sweep that surfaced this
- `crates/ff-rdp-cli/tests/live/live_123_daemon_autostart_and_registry.rs:320`
- `kb/discipline-rationale.md` — why an isolated pass is not evidence

## Part C: live_resource_dedupe times out on its first subscribe under sweep load (absorbed from iteration 250)

> **Renumbered 229 → 250 on 2026-09-06** so the pending queue runs as one contiguous sweep (DEC-051). Older PRs, commits and sweep logs cite it as iteration 229.

### Why

`ResourceCommand::subscribe` returned `Timeout` on the **first** of two subscriptions during
iteration 225's live sweep — one failure in 311 CLI-tier tests running at `--test-threads=6`, and
green in 2.6 s when re-run alone. Nothing in 225's diff touches the resource bus (it changed
`commands/page_view.rs`, `commands/page_view_js.rs` and their tests), so this is not that
iteration's defect; it is an unowned, load-sensitive failure that has never been filed.

`kb/discipline-rationale.md`'s rule applies: **"environmental" is a diagnosis, not a
disposition.** A live test that times out under contention is either

- a test that under-budgets a real round trip that legitimately takes longer when six Firefox
  processes share the machine — a test defect, fixable by budgeting honestly rather than by
  raising a number until it stops failing; or
- `getWatcher` / `watchResources` genuinely serialising behind something on the parent process,
  in which case a caller with a busy browser sees the same timeout and it is a product defect.

Which of the two it is has not been established, and a sweep that quietly re-runs it green is
precisely how that question stops being asked.

### Themes

- **A — Reproduce deliberately.** Run the suite under a controlled load (`--test-threads=6` over
  the CLI tier, or a narrower harness that spawns N browsers) until the failure recurs, and
  capture *which* leg of the subscribe is slow: the `getWatcher` request, the `watchResources`
  reply, or the first resource frame.
- **B — Attribute it.** Instrument the timing on the failing leg. If the wire round trip is
  inside the budget and the test's own wait is not, it is a test defect; if the reply genuinely
  does not arrive, it is a product defect and gets its own follow-up.
- **C — Fix the attributed cause, not the symptom.** A raised timeout is acceptable *only* with a
  measured distribution behind it, recorded in the plan.

### Tasks

#### A. Reproduce [0/2]
- [ ] Recur the failure under controlled load, with the run recorded
- [ ] Identify which leg of `subscribe` exceeds its budget

#### B. Attribute [0/1]
- [ ] State, with timings, whether this is a test-budget defect or a product defect

#### C. Fix [0/1]
- [ ] Land the fix the attribution points at; no timeout raised without a measured distribution

### Acceptance Criteria [0/3]

- [ ] The failure is reproduced deliberately at least once, not merely waited for
- [ ] The cause is attributed to the test or to the product, in writing, with timings
- [ ] Three consecutive full live sweeps at the default thread count with
      `live_61q_resource_bus` green — satisfied by Part D AC 1; tick together

### Out of scope

- Any other flaky live test. If the investigation finds a shared cause (parent-process
  serialisation under N browsers), file that as its own plan rather than widening this one — now Part D of this plan.

### References

- [[iteration-225-reader-excerpt-infobox]] — the sweep that surfaced it
- `crates/ff-rdp-cli/tests/live/live_61q_resource_bus.rs:258` — the failing assertion

## Part D: the daemon's frame-target subscription misses a fixed 15 s bound under a parallel sweep (absorbed from iteration 251)

> **Renumbered 198 → 251 on 2026-09-06** so the pending queue runs as one contiguous sweep (DEC-051). Older PRs, commits and sweep logs cite it as iteration 198.

### Where this came from

[[iteration-188-live-sweep-cost-and-parallelism]] made the CLI live tier run concurrently and then
ran it repeatedly. Two tests failed once each across those runs, and neither is explained by any
open plan:

| run | jobs | failing test | message |
|---|---|---|---|
| 1 | 6 | `live_145_error_envelope_completeness::live_145_click_frame_scan_js_exception_envelope` | `daemon never reported live frame targets` |
| 2 | 6 | `live_137_daemon_mode_parity::live_137_consent_accept_via_daemon` | `daemon never reported live frame targets` |
| 4 | 6 | `live_140_element_targeting::live_140_frame_error_bounded` | frame-target assertion |
| 4 | 6 | `live_145_…click_frame_scan_js_exception_envelope` | `daemon never reported live frame targets` |
| 4 | 6 | `live_169_nav_verb_status_parity::live_169_nav_verbs_report_status_daemon` | `status: null, status_reason: "not_observed"` after `elapsed_ms: 21017` |
| 5 | 6 | `live_137_…consent_accept_via_daemon` | `daemon never reported live frame targets` |
| 6 | 6 | `live_160_envelope_honesty::live_160_click_reachable_fires_handler` | click handler assertion |
| iter-191 sweep, 2026-08-23 | default | `live_174_direct_route_events_path::live_174_nav_verbs_resolve_from_events_daemon` | `navigate: page did not fire dom-complete within the timeout`; **passes alone** — re-ran `--test-threads=1` immediately after: `1 passed` in 5.41 s |
| iter-191 sweep (contaminated run, see note), 2026-08-23 | default | `live_137_…consent_accept_via_daemon`, `live_140_frame_error_bounded`, `live_140_frame_filter_count_accurate`, `live_111_daemon_follow_cross_process::live_daemon_follow_survives_cross_process_nav`, `live_navigate_default_fast::live_navigate_elapsed_matches_wall` | five failures in one run; the same run also failed `live_158_launch_reports_effective_wait_bound` on a **fixed port 7105 held by an orphaned Firefox** from an aborted earlier sweep, so that run's load was not representative. The clean re-run left only the `live_174` row above |
| iter-197 sweep (contaminated: overlapped `cargo fmt`/`clippy`/`cargo test -p xtask` on the same box), 2026-08-24 | default (6) | `live_137_…consent_accept_via_daemon` | `daemon never reported live frame targets` — status showed `target_count: 1, live_target_count: 0` after 17 s uptime |
| iter-197 sweep (same contaminated run), 2026-08-24 | default (6) | `live_165_eval_call_scope::live_165_repeated_const_matches_help` | **a second signature**: `daemon did not respond within the timeout after auth — the daemon may be overloaded or the connection is stale` (`error_type: Timeout`). Not a frame-target assertion at all — the daemon stopped answering after a successful auth. The clean re-run of the same sweep was **276 passed / 0 failed**, so both are load-sensitive, not deterministic |

| iter-224 sweep 1, 2026-08-31 | default (6) | `live_171_recycled_owner_pid::live_171_recycled_owner_pid_no_longer_reads_as_live` | `would_remove=[]` — the recycled-PID profile read as owned. **Passed alone** immediately afterwards (`--test-threads=1`, 2 passed in 2.86 s), and passed in sweep 2 of the same branch |
| iter-224 sweep 2, 2026-08-31 | default (6) | `live_160_envelope_honesty::live_160_type_emits_key_events` | **the second signature again**: `daemon did not respond within the timeout after auth — the daemon may be overloaded or the connection is stale` (`error_type: Timeout`), on the `eval JSON.stringify(window.__keys)` step. Passed in sweep 1 of the same branch. This is the `live_165` row's message on a different test, which is evidence the signature belongs to the daemon under load and not to any one test |

**No test failed twice in the same way in consecutive runs, and no run repeated another's failure
set** — but three of the seven failures carry the *same* message, which is the thread to pull.

iter-197 adds a **second** signature to pull on alongside it: `live_165_repeated_const_matches_help`
failed with `daemon did not respond within the timeout after auth`, which is the daemon going quiet
*after* a successful handshake rather than a frame-target wait expiring. If both signatures share a
cause — a daemon that cannot keep up under concurrent load — that is one defect, not two; if they do
not, `live_165` needs its own row in whatever this iteration concludes. Do not assume.

### The shared signature

`live_137_daemon_mode_parity.rs:116`:

```rust
fn wait_for_live_targets(port: u16) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    …
}
```

A fixed 15 s bound on "the daemon has established its frame-target subscription". `live_145`
carries its own copy of the same wait with the same message. On an idle machine that bound is
never approached; under a `--jobs 6` sweep (load average 150+ with six Firefox instances starting)
it is. `live_169`'s failure is the same shape one level up: the daemon reported
`status_reason: "not_observed"` after **21 s**, i.e. it did observe the navigation, just not
inside whatever window the assertion allows.

`live_137` was also on iteration 188's Theme A list of tests that failed at `-j8` under
`cargo nextest`, classified there as a "contention artifact, not a defect" — on no evidence beyond
the fact that it passed at lower concurrency. **That classification is the thing this iteration
exists to replace.** "It only fails under load" is a description, not a diagnosis: iteration 179 spent a
whole iteration establishing that exactly this description hid a real arming race
(`assert_network` on the direct route), which iteration 181 then fixed.

Both failures are on the **daemon** route, which is the same neighbourhood as the fix in 181 and
the frame-target bookkeeping in [[iteration-129-consent-and-cross-origin-frames]].

### The question to answer

For each test, decide between:

1. **A real race in the product** — e.g. a `watchTargets`/frame-target subscription that is armed
   after the event it needs, which is 179's shape exactly. Fix the product.
2. **A race in the test** — a poll bound that is generous on an idle machine and too tight at
   load average 150. Fix the bound, and say what the new bound is measured against.
3. **A resource ceiling** — the machine cannot start N browsers and keep them responsive. Then it
   is a concurrency-policy finding for `live-sweep`, not a test fix.

Do not accept (3) without a measurement, and do not accept (2) without stating the observed
timing distribution. A widened timeout that hides a real race is how iteration 179's defect
survived several iterations.

### Tasks

#### A. Reproduce [0/3]
- [ ] Each named test 10x serially (control) and across at least 3 sweeps at `--jobs 6`
- [ ] Capture the daemon-route logs from a failing instance, not just the assertion text
- [ ] Instrument `wait_for_live_targets` to record how long the subscription *actually* takes,
      idle and under sweep load — the distribution, not one number

#### B. Classify and act [0/2]
- [ ] One of the three verdicts above per test, in writing, with the evidence
- [ ] The fix that follows from the verdict, plus the repeated run that shows it holding

### Acceptance Criteria [0/3]

- [ ] Three consecutive `--jobs 6` sweeps with an empty failure set
- [ ] Neither fix widens a timeout without a stated measurement behind the new value
- [ ] If either turns out to be a product race, it has a live Firefox test that fails before the
      fix and passes after — not only a unit test

### Out of scope

- `live_153_replace_double_envelope` — diagnosed and fixed inside iteration 188 (it was a real
  regression from that iteration's `FF_RDP_HOME` change, not load; it failed serially too, which
  is exactly how it was told apart from the tests listed here).
- Lowering the sweep's concurrency to make these pass. That trades the 8x this batch bought for a
  green that hides the same race; iteration 188 chose to keep the speed and own the flakes here.
- The sweep hanging on `live_158_launch_survives_contended_bind` — that is
  [[iteration-197-live-sweep-has-no-per-test-timeout]].

### References

- [[iteration-188-live-sweep-cost-and-parallelism]] — the parallel sweep that surfaced both
- [[iteration-179-live-62-runner-sees-no-network-events]] — the precedent: "fails only under load"
  turned out to be a real arming race
- [[iteration-181-playbook-scoped-network-subscription]] — its fix, on the daemon/direct split

## Closing acceptance criterion (covers all parts) [0/1]

- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean. (covers all parts)
