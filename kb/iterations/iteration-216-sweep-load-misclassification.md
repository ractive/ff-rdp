---
title: "Iteration 216: load-sensitive live tests report as product defects"
type: iteration
date: 2026-08-30
status: planned
branch: iter-216/sweep-load-misclassification
depends_on: []
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
tags: [iteration, live-tests, test-reliability, xtask]
---

# Iteration 216: load-sensitive live tests report as product defects

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
`live_166` pair is [[iteration-214-live-166-cache-304]], already filed.

What is left is **four tests that failed in one run and passed in the other**, none of which
touches the surface iter-211 changed. That is the subject of this plan. One green run is not
evidence a load-sensitive test is fine — it is one sample — and the two runs here disagree about
which four tests they are.

## The failures

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

## Themes

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

## Tasks

### A. Launch-timeout classification [0/2]
- [ ] `live-sweep` recognises the `did not open debug port … within Ns` error envelope in a
      failing test's captured stdout and counts that test in `launch_timeout` rather than as a
      product failure
- [ ] The summary line names the affected tests, as `vanished` and `timed_out` already do

### B. Nested-cargo isolation [0/2]
- [ ] `crates/xtask/tests/check_firefox_refs.rs` no longer races the outer build lock — call the
      check directly, or set a distinct `CARGO_TARGET_DIR` for the nested run
- [ ] Audit the other `xtask` integration tests for the same `cargo run` shape and fix them the
      same way

### C. Evidence scan [0/1]
- [ ] `iter_179_harness_stdout_evidence`'s `scanned_roots` includes `crates/xtask/tests`, with a
      per-root floor like the existing ones

### D. Triage the load-sensitive four [0/2]
- [ ] For `live_100_e2e_sigterm_removes_registry`, `live_175_failed_launch_leaves_no_profile_dir`,
      `live_186_launch_record_growth_bounded` and `live_61q_live_resource_dedupe`, record in this
      plan which of the three causes applies — and treat "flaky" as a starting point, not an
      answer
- [ ] Fix the ones that are waits; file the ones that are product bugs as their own plans

## Acceptance Criteria [0/6]

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
      the disagreement between iter-211's two runs is the defect being closed
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

- **Do not raise the launch timeout to make Theme A's symptom go away.** The 30 s budget is the
  measurement; the defect is that exceeding it is filed under the wrong heading. Raising it hides
  the signal in exactly the way `FF_RDP_LAUNCH_TIMEOUT_SECS` exists to let a caller do
  *deliberately*.
- **Theme B is not "add a retry".** A retry would turn a lock race into a slower lock race and
  would leave the empty-stderr message in place. Removing the nested `cargo` is the fix.

## Out of scope

- The `live_166` HTTP 304 failures from the same sweep — already filed as
  [[iteration-214-live-166-cache-304]].
- Changing `live_186`'s own assertions. The test is measuring the right thing; the sweep is
  reporting its failure under the wrong heading.

## References

- [[iteration-211-find-not-guess]] — the two sweeps that found these
- [[iteration-214-live-166-cache-304]] — the other outstanding live-suite honesty item
- `crates/xtask/src/live_sweep.rs` — where `vanished`/`launch_timeout`/`timed_out` are classified
- `crates/ff-rdp-cli/tests/iter_179_harness_stdout_evidence.rs` — `scanned_roots`, Theme C
