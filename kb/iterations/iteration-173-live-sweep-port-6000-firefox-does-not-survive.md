---
title: "Iteration 173: the hand-started port-6000 Firefox does not survive the CLI tier, so live-sweep reports 7 false ff-rdp-core failures"
type: iteration
date: 2026-08-16
status: in-review
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

## Theme A — diagnosis, recorded (2026-08-17)

**Cause: an external human kill. Not a product `killpg`, not a concurrent sweep, not Firefox
exiting by itself.** Evidence, suspect by suspect:

| suspect | verdict | evidence |
|---|---|---|
| A human killed it | **RULED IN** | Firefox processes on that machine were killed between 21:37 and 21:40, inside iteration 168's 21:31–21:45 CLI tier. Recorded in the `Added 2026-08-17` block above. |
| `daemon stop`'s process-group reap (`kill_process_group_force`) | **RULED OUT** | Two subsequent dual-gate sweeps on `main` at `4d639e2`, and this iteration's own sweep, ran the full `daemon stop`-exercising CLI tier with the same hand-started port-6000 browser alive throughout; the core tier reported 9/9 both times. A `killpg` reaching a foreign browser would have reproduced every time, not once. |
| A concurrent sweep from another working tree | **RULED OUT** | Same evidence: the repeat runs were on the same machine with the same layout and did not reproduce. |
| Firefox exiting by itself | **RULED OUT** | The same process survived 35-40 minutes of CLI tier on the repeat runs; a self-exit at ~13 minutes is not a property of the browser. |

The dogfood_path's step 3 ("poll for the pid throughout a sweep and record which phase it dies
in") was therefore not run as a hunt: the phase is known (CLI tier), the pid is known (iteration
168's log), and re-running it would have produced a browser that does not die — which is what the
two `4d639e2` sweeps and this iteration's sweep already demonstrated. Recording a non-reproduction
under a known-external cause is honest; three more 40-minute sweeps chasing it would not be.

Because the cause is external, **nothing in this iteration is a product fix**. What remains is
[[iteration-169-navigate-status-delivery-and-nav-verb-parity]] Theme C's single unexplained
occurrence from iteration 166's sweep, which stays open there.

## Theme C — the fixed port 6000 stays, decision recorded (2026-08-17)

**Decision: keep port 6000, and keep "classify, do not launch".** Reasoning:

- Making `live-sweep` bind 6000 itself inherits the whole ownership problem the fails-closed guard
  in `daemon/client.rs` exists to prevent — port 6000 is ff-rdp's documented default and the port
  a human is most likely to already be using by hand. A sweep that launches on it either collides
  with the operator's browser or has to decide whether to kill one it did not start, which is
  exactly the 2026-07-09 kill-scoping incident ([[iteration-110-post-batch-live-sweep]]).
- Moving the `ff-rdp-core` live tests to a free port means changing their contract — they connect
  to a browser they never launch, by design — which this plan lists as **out of scope**.
- The actual harm was never the fixed port. It was the sweep *asserting* a precondition it had
  checked once, 40 minutes earlier. Re-probing costs one TCP connect per target and removes the
  harm without touching the port policy.

So: port 6000 stays fixed, the sweep still refuses to start a browser on it, and the honesty comes
from re-probing rather than from ownership. Theme B's "better: have the sweep launch and own that
Firefox" option is **rejected** for the reason above.

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
      — **steps 1 and 2 yes, step 3 deliberately not**: the cause was already established as an
      external human kill before this iteration started, and step 3's polling hunt would have
      produced a browser that does not die (as the two `4d639e2` sweeps and this iteration's own
      sweep did). Left unticked rather than reworded; see Theme A above for what was run.
- [x] Record which phase the port-6000 browser dies in, with the pid and a timestamp
      (iteration 168's CLI tier, 21:31–21:45, killed 21:37–21:40)
- [x] Rule each suspect in or out by name, with evidence — see the Theme A table
- [x] If the cause is a product `killpg` reaching a foreign browser, file that separately as a
      product defect — it is not a harness fix (it is not; nothing to file)

### B. Fix the sweep
- [x] Port 6000 is re-probed immediately before the core tier, or the sweep owns the browser
      (`repartition_for_probe`, called per target in `run()`; owning the browser rejected — Theme C)
- [x] A vanished browser is reported as `preexisting`/`ignored`, never as a test failure
      (new `vanished=V` count; the tests run without `--include-ignored`, and a browser that dies
      *mid*-tier is caught by a post-phase re-probe in `classify_failures`)
- [x] Unit test for the classification, without a real Firefox
      (`test_173_vanished_browser_moves_core_tests_out_of_qualified`,
      `test_173_connection_refused_after_browser_loss_is_not_a_genuine_failure`, and six more —
      eight `test_173_*` tests total cover Theme B/the launch-timeout classification, plus two more
      for Task D, for ten new tests overall; verified via `cargo test -p xtask live_sweep:: -- --list`)

### C. Port policy
- [x] Record the decision on the fixed port 6000, and the reasoning — see Theme C above

### D. `PREEXISTING_MARKERS` misclassifies on a bare substring (folded in by iter-172, 2026-08-17)
- [x] Make the `preexisting` classification robust to a live source that merely *mentions*
      `firefox_port`, without weakening the executed / skipped / preexisting accounting
      (`SELF_LAUNCH_MARKERS` overrides the positive markers; `test_158_real_core_targets_are_preexisting`
      still passes unchanged, so the core tier's classification is untouched)
- [x] Unit test with a source that names the field but launches its own Firefox
      (`test_173_registry_assertion_does_not_make_a_suite_preexisting`, plus
      `test_173_self_launch_marker_does_not_weaken_the_preexisting_tier` fencing the other direction)

`source_needs_preexisting_instance` decides the tier by substring match, and one of its three
markers is the bare word `firefox_port` (`live_sweep.rs:107-114`). That is the name of a field in
`daemon.<port>.json`, so **any `ff-rdp-cli` live test that reads the registry back and asserts on
that field is silently reclassified as needing a Firefox somebody else started on port 6000** —
even though it launches its own. iter-172 hit this while writing
`live_172_published_record_is_complete_and_lock_is_a_sibling`: the two new tests moved into the
`preexisting` bucket and tripped `test_158_real_core_targets_are_preexisting`
(`assertion left: 2, right: 0 — the ff-rdp-cli live suites launch their own Firefox`).

That assertion caught it, which is the good news. The bad news is what it caught it *with*: a
whole-workspace invariant test, not a message about the file in question, and the only available
workaround was for the test to avoid writing the word — which iter-172 did, with a comment
explaining why. The next author will not know to.

Consequence if it goes unfixed: a CLI test that mentions the field is classified `preexisting`,
so when nothing is listening on 6000 it is reported `ignored` instead of run. That is the same
false-green shape as [[iteration-155-live-skip-reports-green]], reached by a different road.

## What was built (2026-08-17)

All in `crates/xtask/src/live_sweep.rs`.

1. **`repartition_for_probe(gated, gates, probe_now)`** — re-partitions one target against a
   *fresh* TCP probe of 127.0.0.1:6000, taken in `run()` immediately before that target runs
   instead of once for the whole sweep. Returns the partition to drive `cargo test` with plus the
   names that left `qualified`. Those go into phase 2 (no `--include-ignored`), so libtest reports
   them `ignored` in its own vocabulary — the same mechanism iter-155 chose, not a fabricated
   status.
2. **`classify_failures(stdout, browser_still_up, target_needs_preexisting)`** — attributes each
   `FAILED` test in a phase's output to `vanished` / `launch_timeout` / `genuine`, by parsing
   libtest's `---- <name> stdout ----` blocks. This catches the browser dying *inside* the core
   tier, which the pre-tier re-probe alone cannot. A launch timeout is attributed first: it names
   its own cause in the panic message, so it is never swept into the weaker "browser is gone"
   inference.
3. **Streaming `run_phase`** — the phases used to inherit stdout; parsing requires capturing it.
   The output is echoed through line by line so a 35-40 minute tier still shows progress live.
4. **`SELF_LAUNCH_MARKERS`** (Task D) — a source containing `LiveFirefox` / `RawFirefox` is never
   `preexisting`, whatever else it mentions. 94 of the 97 `tests/live/` files contain one; no
   `ff-rdp-core` live target does, so the core tier's classification is bit-for-bit unchanged.
5. **`LIVE_SWEEP_SUMMARY … vanished=V launch_timeout=L`** — both carved *out* of `executed`, never
   added on top, so `total=T` is conserved and no reclassification can inflate the number a PR
   body quotes (`test_173_summary_total_conserves_every_tier`).

**What deliberately did not change.** `vanished` does not fail the sweep (those tests never ran).
`launch_timeout` **does** — it is a red libtest result, and turning reds green on inference is how
a real regression gets waved through. The plan asked only for a distinct count, and that is what
it gets. The executed / skipped / preexisting accounting, the deliberate phase-2 run *without*
`--include-ignored`, and the empty-scan guard are all untouched.

## Acceptance Criteria [5/5]

- [x] The Theme A diagnosis is recorded, naming the cause and the evidence for it
      — external human kill; three other suspects ruled out by name in the Theme A table
- [x] A sweep whose port-6000 browser disappears mid-run does not report core-tier tests as
      failed — asserted by a test that fails on `main`
      (`test_173_vanished_browser_moves_core_tests_out_of_qualified` and
      `test_173_connection_refused_after_browser_loss_is_not_a_genuine_failure`; both reference
      `repartition_for_probe` / `classify_failures`, which do not exist on `main`)
- [x] `LIVE_SWEEP_SUMMARY` still distinguishes executed / skipped / preexisting honestly, and
      never inflates `executed` to hide the change
      (`test_173_summary_total_conserves_every_tier`: the new tiers are subtracted from
      `executed`, and `total` is invariant under the reclassification)
- [x] A `ff-rdp-cli` live source that names `firefox_port` but launches its own Firefox is
      classified as executed, not `preexisting` — asserted by a test that fails on `main`
      (`test_173_registry_assertion_does_not_make_a_suite_preexisting`)
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q`
      clean, plus a dual-gate live sweep
      [2026-08-17, `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1` + hand-started Firefox on
      6000 → `LIVE_SWEEP_SUMMARY executed=277 skipped=0 preexisting=0 vanished=0
      launch_timeout=0 total=277`, **277 passed / 0 failed**, exit 0; CLI tier 268/268 in
      2267.62 s, core tier 9/9]

## Live sweep, 2026-08-17

```
FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
LIVE_SWEEP_SUMMARY executed=277 skipped=0 preexisting=0 vanished=0 launch_timeout=0 total=277
```

277 passed / 0 failed, exit 0. 22:21:12 → 22:59:15 (38 min). Both gates set; a hand-started
headless Firefox (`/tmp/ff-rdp-sweep-profile-6000`) held port 6000 for the whole run and was
still there afterwards — same pid set before and after, `pgrep -f 'ff-rdp/profiles'` empty both
times.

- `ff-rdp-cli --test live`: 268 qualified, **268 passed; 0 failed** in 2267.62 s.
- `ff-rdp-core`: 9 qualified across 4 binaries, **9 passed; 0 failed**. That includes all seven
  tests this plan was filed about — `live_129_frame_targets_enumerated`,
  `live_single_target_per_browsing_context`, `live_cache_disable_via_target_config`,
  `live_network_set_cookie_longstring`, `live_unwatch_targets_does_not_hang`,
  `live_connect_and_list_tabs`, `live_selected_tab_is_marked` — all `ok`.

`executed` rose from the batch baseline's 275 to 277. That is **not** an improvement in coverage
measurement: it is iter-172's two new registry tests, which this iteration's Task D also un-hid
from the `preexisting` bucket. Nothing was removed from the corpus.

**What this sweep did not exercise.** `vanished=0 launch_timeout=0` means the new runtime paths
never fired: nothing failed, and the browser did not die. The re-probe ran (once per
`ff-rdp-core` target, four times) and found the browser present each time, which is the
no-op branch. The classification itself is covered only by the eight deterministic unit tests. A
live end-to-end demonstration would need a 38-minute sweep plus a deliberately timed kill; see
the carry-over row.

## Out of scope

- Making the `ff-rdp-core` live tests launch their own Firefox. That is a larger change to those
  tests' contract; this iteration is about the sweep not misreporting.

## References

- [[iteration-168-livefirefox-drop-does-not-wait-for-exit]] — the sweep that surfaced this
- [[iteration-155-live-skip-reports-green]] — why `live-sweep` exists at all
- [[iteration-110-post-batch-live-sweep]] — an ff-rdp operation must never signal a Firefox it did not
  launch; relevant if Theme A finds a product `killpg` behind this

## Carry-over (2026-08-17, reviewer-updated 2026-08-17)

Built line by line from `sweep.log`. **Every line of that log is green** — 277 `ok`, zero
`FAILED`, zero `panicked`, zero `error[`, exit 0 — so there is no row sourced from a non-green
sweep line. The rows below come from the other three sources the closing procedure names: ACs left
unticked, items that passed *this* run but failed a previous one, and findings stated in the plan
prose.

**Reviewer note:** iter-173 is the last iteration of this run — there is no next iteration's plan
to fold anything into. Rows 1, 2, 4 and 6 were originally dispositioned "no plan, with a stated
reason", which is only a safe disposition when a later iteration is expected to revisit the
trigger condition. Since none will, they are now filed together as
[[iteration-178-live-sweep-carryover-watch-conditions]] instead (`check-iteration-plan: OK`).
Rows 3, 5 and 7 stay as originally dispositioned — they are genuinely resolved, not deferred.

| # | item | source | disposition |
|---|---|---|---|
| 1 | Task A's first box — "run every step of `dogfood_path`" — left **unticked**. Steps 1 and 2 were run; step 3 (poll for the pid through a sweep to find what kills the browser) was deliberately not run as a hunt. | AC/task left unticked | **filed as [[iteration-178-live-sweep-carryover-watch-conditions]], watch condition 4.** The cause was established as an external human kill before this iteration began (Theme A table, three other suspects ruled out by name), and step 3 against a browser that does not die produces a non-reproduction, which is what this sweep and the two `4d639e2` sweeps already are. The task box stays empty rather than reworded. Trigger: a port-6000 browser dies in a future sweep on a machine no human touched. |
| 2 | `live_160_envelope_honesty::live_160_ref_click_asserts_handler_effect` — **passed this run**; failed iter-168's and iter-171's sweeps. | passed now, failed before | **filed as [[iteration-178-live-sweep-carryover-watch-conditions]], watch condition 1**, carrying forward iter-172's rule verbatim: its cause was never established, one green run is not evidence, but `with_daemon_or_reason` now prints `meta.route` / `meta.daemon_fallback`. Trigger: this test fails again — the printed reason then attributes it. |
| 3 | `live_123_daemon_autostart_and_registry::live_daemon_autostart_tabless` launch timeout — **passed this run**; failed iter-170's sweep with `never opened debug port … within 30s`. | passed now, failed before | **closed in this PR, for the reporting half only.** A launch timeout now lands in `launch_timeout=L` with an explicit stderr line naming the tests, instead of being indistinguishable from a product failure (`classify_failures`, `test_173_launch_timeout_is_classified_separately_from_a_real_failure`). The *budget* is untouched — see row 4. |
| 4 | "Whether the launch budget itself should scale with the sweep is a separate question and may be the right answer instead; decide it on evidence." | plan prose, iter-170 fold-in | **filed as [[iteration-178-live-sweep-carryover-watch-conditions]], watch condition 2.** The evidence available today argues against changing it: `launch_timeout=0` across 277 tests in a 38-minute serial sweep on this machine. Raising a timeout with no failing measurement to point at is guessing. Trigger: any sweep reporting `launch_timeout>0` — the count now exists precisely so that evidence is collectable. |
| 5 | Theme B's stronger option: "better: have the sweep launch and own that Firefox", which would also remove the manual setup step `iteration-close` asks every iteration to perform by hand. | plan prose | **closed in this PR** — **rejected**, with the reasoning recorded in Theme C and DEC-043. Binding port 6000 inherits the ownership problem the fails-closed guard in `daemon/client.rs` exists to prevent (the 2026-07-09 kill-scoping incident). The manual setup step therefore stays. |
| 6 | The `vanished` and `launch_timeout` runtime paths **never fired in this sweep** (`vanished=0 launch_timeout=0`): nothing failed and the browser did not die, so only the re-probe's no-op branch executed live. | plan prose (live-sweep section) | **filed as [[iteration-178-live-sweep-carryover-watch-conditions]], watch condition 3.** The classification is covered today by eight deterministic unit tests over real captured libtest output, which is the level the ACs asked for; a live demonstration costs a 38-minute sweep plus a deliberately timed `kill` of the operator's browser, and would prove nothing the unit tests do not. Trigger: a future sweep reports `vanished>0` (or `launch_timeout>0`) whose numbers don't reconcile against the log — that is a bug in this iteration's code. |
| 7 | The single unexplained port-6000 death from iteration 166's sweep — not the iteration-168 one, which Theme A attributes. | plan prose (`Added 2026-08-17` block) | **already filed** — it is [[iteration-169-navigate-status-delivery-and-nav-verb-parity]] Theme C and stays open there. Not duplicated here. |

Nothing external interfered with this sweep: the port-6000 Firefox held the port throughout with
an unchanged pid set, and `pgrep -f 'ff-rdp/profiles'` was empty both before and after.
