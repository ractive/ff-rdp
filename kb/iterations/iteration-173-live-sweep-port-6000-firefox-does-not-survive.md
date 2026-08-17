---
title: "Iteration 173: the hand-started port-6000 Firefox does not survive the CLI tier, so live-sweep reports 7 false ff-rdp-core failures"
type: iteration
date: 2026-08-16
status: planned
branch: iter-173/live-sweep-owns-port-6000
depends_on: []
first_call_sites: []
dogfood_path: |
  # Harness/tooling defect in `cargo run -p xtask -- live-sweep`.

  # 1. Start a Firefox on the fixed port 6000 the ff-rdp-core tier requires,
  #    and confirm live-sweep classifies the core tier as qualified.
  firefox -no-remote -headless -profile /tmp/ff-rdp-6000 \
    --start-debugger-server 6000 &
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
    cargo run -p xtask -- live-sweep
  # → OBSERVED 2026-08-16 (iteration 168's sweep): the sweep classified the
  #   core tier as "0 will report ignored (no Firefox on 6000)" at the start,
  #   then every ff-rdp-core test failed ~14 minutes later with
  #   ConnectionFailed(Os { code: 61, kind: ConnectionRefused }).
  #   7 tests across 4 binaries: live_129_frame_targets_enumerated,
  #   live_single_target_per_browsing_context, live_cache_disable_via_target_config,
  #   live_network_set_cookie_longstring, live_unwatch_targets_does_not_hang,
  #   live_connect_and_list_tabs, live_selected_tab_is_marked.

  # 2. Confirm they are green when Firefox is actually present — restart it and
  #    re-run just the core binaries.
  #    → OBSERVED: 7/7 pass. The failures are entirely about the browser being
  #      gone, not about the tests.

  # 3. Find out what kills it. Poll for the pid throughout a sweep and record
  #    which phase it dies in. Suspects to rule in or out, in order:
  #    `daemon stop`'s process-group reap (`kill_process_group_force` in
  #    crates/ff-rdp-cli/src/daemon/process.rs), a concurrent sweep from
  #    another working tree on the same machine, and Firefox exiting on its own.
tags: [iteration, testing, live-tests, tooling, xtask]
---

# Iteration 173: `live-sweep` depends on a port-6000 Firefox it neither owns nor re-checks

Carry-over from [[iteration-168-livefirefox-drop-does-not-wait-for-exit]]'s dual-gate live sweep
(2026-08-16, `LIVE_SWEEP_SUMMARY executed=270 skipped=0 preexisting=0 total=270`, 260 passed /
10 failed). Seven of those ten failures were this.

## What was observed

> **Added 2026-08-17 — the browser's death was external; the reporting defect is unaffected.**
> A human killed Firefox processes on that machine between 21:37 and 21:40, inside iteration 168's
> 21:31–21:45 CLI tier. Two subsequent dual-gate sweeps on `main` at `4d639e2` kept the same
> hand-started port-6000 browser alive through ~13 minutes of CLI tier each, and the core tier
> reported **9/9 passed** both times.
>
> This does **not** weaken the plan — it sharpens it. Theme A ("find out what kills it") is now
> largely answered for this instance and should not be a hunt: what remains is
> [[iteration-169-navigate-status-delivery-and-nav-verb-parity]] Theme C's single unexplained
> occurrence from iteration 166's sweep. The defect *this* plan is actually about is the
> **classification**: `live-sweep` probes port 6000 once and never re-checks, so a browser that
> vanishes for **any** reason — including an operator killing it — is reported as 7 failing tests
> rather than as an unmet precondition. An external kill is a perfectly good demonstration of that
> bug, and arguably the cleanest one available. Fix the reporting; treat the cause as out of scope
> here.

The `ff-rdp-core` live tests never launch a browser; they connect to whatever is on the fixed port
6000, and `live-sweep` probes that port **once, at classification time** to decide whether to run
them. In iteration 168's sweep the probe succeeded, the CLI tier then ran for 831 seconds, and by
the time the core tier executed the browser was gone. Every core test failed with
`ConnectionRefused`, and the sweep reported them as real failures.

Re-running the same four binaries against a freshly started port-6000 Firefox: **7/7 pass**.

## Why this matters more than "environmental"

`live-sweep` exists to stop a live suite from reporting results it did not earn
([[iteration-155-live-skip-reports-green]], iter-158). This is the same failure mode with the sign
flipped: seven tests reported *red* for a reason that has nothing to do with the code under test,
in the one artifact every iteration pastes into its PR body. A reviewer who trusts the summary
either chases seven ghosts or learns to discount core-tier reds — and the second is how a real
regression gets waved through.

The `preexisting=K` counter already encodes "needs a Firefox somebody else started". It is
computed once and never revisited.

## A second precondition the sweep reports as a product failure (folded in from iter-170)

iter-170's carry-over sweep hit a different unmet precondition that the sweep reported the same
wrong way — as a failing test:

```text
live_123_daemon_autostart_and_registry::live_daemon_autostart_tabless   FAILED
  RawFirefox: /Applications/Firefox.app/.../firefox (pid 43844) never opened debug port
  64638 within 30s (raise FF_RDP_LIVE_LAUNCH_TIMEOUT_SECS)
```

It passes on a re-run in isolation. The sweep is serial and took 38 minutes, so a per-test 30 s
launch budget is being spent against a fully loaded machine — iter-158 raised that budget to 30 s
for exactly this contention and the sweep still exceeds it.

This belongs to Theme B for the same reason the port-6000 case does: **a browser that could not be
started is an unmet precondition, not a failing assertion**, and reporting it as the latter is what
sends the next reader hunting a product defect that is not there. Whatever Theme B does for the
port-6000 probe should give a launch timeout inside a sweep the same distinct classification — at
minimum a separate count in `LIVE_SWEEP_SUMMARY` so a reader can tell "the product is broken" from
"the machine could not start a browser in time". Whether the budget itself should scale with the
sweep is a separate question and may be the right answer instead; decide it on evidence.

## Themes

- **A — Establish what actually kills it.** Run the `dogfood_path` step 3. Do not guess: the
  suspects (`daemon stop`'s process-group reap, a concurrent sweep from another tree, Firefox
  exiting by itself) have different fixes, and one of them — a product `killpg` reaching a browser
  ff-rdp did not launch — would be a **product defect of exactly the class
  [[iteration-110-post-batch-live-sweep]] exists to prevent**, not a harness annoyance. Establish which
  before writing any code.
- **B — Make the sweep not lie about it.** At minimum, re-probe port 6000 immediately before the
  core tier and report `preexisting` rather than `failed` when the browser has gone. Better: have
  the sweep launch and own that Firefox for the duration, which removes the manual setup step the
  `iteration-close` skill currently asks every iteration to perform by hand.
- **C — Decide whether the fixed port 6000 earns its keep.** Every other live tier picks a free
  port. A fixed port is also what made the 2026-07-09 kill-scoping incident possible. Deciding is
  in scope; changing it may not be.

## Tasks

### A. Diagnose
- [ ] Run every step of `dogfood_path` and paste actual outputs into this plan
- [ ] Record which phase the port-6000 browser dies in, with the pid and a timestamp
- [ ] Rule each suspect in or out by name, with evidence
- [ ] If the cause is a product `killpg` reaching a foreign browser, file that separately as a
      product defect — it is not a harness fix

### B. Fix the sweep
- [ ] Port 6000 is re-probed immediately before the core tier, or the sweep owns the browser
- [ ] A vanished browser is reported as `preexisting`/`ignored`, never as a test failure
- [ ] Unit test for the classification, without a real Firefox

### C. Port policy
- [ ] Record the decision on the fixed port 6000, and the reasoning

## Acceptance Criteria [0/4]

- [ ] The Theme A diagnosis is recorded, naming the cause and the evidence for it
- [ ] A sweep whose port-6000 browser disappears mid-run does not report core-tier tests as
      failed — asserted by a test that fails on `main`
- [ ] `LIVE_SWEEP_SUMMARY` still distinguishes executed / skipped / preexisting honestly, and
      never inflates `executed` to hide the change
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q`
      clean, plus a dual-gate live sweep

## Out of scope

- Making the `ff-rdp-core` live tests launch their own Firefox. That is a larger change to those
  tests' contract; this iteration is about the sweep not misreporting.

## References

- [[iteration-168-livefirefox-drop-does-not-wait-for-exit]] — the sweep that surfaced this
- [[iteration-155-live-skip-reports-green]] — why `live-sweep` exists at all
- [[iteration-110-post-batch-live-sweep]] — an ff-rdp operation must never signal a Firefox it did not
  launch; relevant if Theme A finds a product `killpg` behind this
