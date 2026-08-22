---
title: "Iteration 179: live_62's runner assertion sees an empty network buffer, and now fails 8/8 on a machine where it used to pass"
type: iteration
date: 2026-08-18
status: in-review
branch: iter-179/runner-network-buffer-empty
depends_on: []
first_call_sites: []
dogfood_path: |
  # Product-or-harness boundary defect, surfaced while measuring iteration 180.
  # It reproduces SERIALLY on an idle machine — do not chase it as a
  # parallelism artifact, which is what it first looked like.

  # 1. Reproduce. FAILED 8/8 on 2026-08-18 under sustained load; PASSED 4/4 on
  #    2026-08-22 on the same machine, same commit, idle. So run this FIRST and
  #    expect either outcome — see "Re-measured 2026-08-22" below before
  #    concluding anything from a green run.
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
    cargo test -q -p ff-rdp-cli --test live -- --ignored --exact \
    live_62_page_map_index::live_runner_page_map_resolution

  # 1b. If it passes, load the machine and try again — that is the actual
  #     experiment now. Compare load averages, not just pass/fail.
  uptime   # 2026-08-18 (failing): 15.55 97.57 184.24 — sustained 15-min load 184
           # 2026-08-22 (passing):  6.55  6.38   7.32

  # 2. See the real error. The assertion prints ONLY stderr, and ff-rdp writes
  #    errors to STDOUT, so the panic message is empty as shipped. Patch it to
  #    print stdout (Theme A does this permanently) and the step-4 envelope is:
  #    {"step":4,"verb":"assert_network","ok":false,
  #     "error":"assert_network: no matching network request found
  #              (url_contains=\"/api/auth/sign-in\", status=200, method=POST)",
  #     "elapsed_ms":3025,"diagnostics":{"events_in_buffer":0}}
  #    Note events_in_buffer=0 — not "no MATCHING event", but no events AT ALL,
  #    on a run whose step 1 navigate reported a 200.

  # 3. Rule out what has already been ruled out (2026-08-18), before re-deriving:
  #    - not this batch's code: 8/8 failures at main 788f362 AND 8/8 at 4d639e2
  #      (pre-169), built in a separate worktree
  #    - not accumulated daemon state: fails with a fresh FF_RDP_HOME=$(mktemp -d)
  #    - not the installed binary: ff_rdp_bin() is CARGO_BIN_EXE_ff-rdp (debug)
  #    - not a Firefox update: bundle mtime Aug 12, BuildID 20260810162159,
  #      matching the browser that was running when it last passed
  #    - not the fixture server's port: FixtureServer binds 127.0.0.1:0

  # 4. The open question this iteration must answer: does `ff-rdp run` buffer
  #    network events at all on this path, and if so when does buffering start
  #    relative to the click that triggers the POST?
tags: [iteration, network, runner, live-tests, flaky]
---

# Iteration 179: the runner's network buffer is empty when `assert_network` reads it

Found 2026-08-18 while taking measurements for
[[iteration-180-live-sweep-cost-and-parallelism]]. It first appeared as a parallel-only failure
and is not one.

## What was observed

`live_62_page_map_index::live_runner_page_map_resolution` drives a four-step playbook through
`ff-rdp run` against a local fixture site: navigate to `/login`, type an email, click **Sign in**,
then `assert_network` for the resulting `POST /api/auth/sign-in`. Steps 1–3 succeed. Step 4 fails:

```json
{"step":4,"verb":"assert_network","ok":false,
 "error":"assert_network: no matching network request found (url_contains=\"/api/auth/sign-in\", status=200, method=POST)",
 "elapsed_ms":3025,"diagnostics":{"events_in_buffer":0}}
```

**`events_in_buffer: 0` is the interesting part.** This is not "the POST did not match" — the
runner's network buffer is empty, on a run whose step-1 navigate itself reported
`"status":200`. Whatever captured that navigate's status did not leave anything in the buffer
`assert_network` reads.

## Measured, before any diagnosis

| question | answer | how |
|---|---|---|
| Is it this batch's code (169–173)? | **No** | 8/8 failures at `main` 788f362 **and** 8/8 at `4d639e2` (pre-169), built in a separate worktree |
| Is it accumulated daemon/registry state? | **No** | fails identically with a fresh `FF_RDP_HOME=$(mktemp -d)`; `~/.ff-rdp` had 3825 stale `launch-record.*.json` files, and clearing their influence changed nothing |
| Is it the freshly installed release binary? | **No** | `ff_rdp_bin()` is `CARGO_BIN_EXE_ff-rdp`, the cargo-built debug binary |
| Did Firefox update? | **No** | bundle mtime Aug 12, `BuildID=20260810162159`, identical to the browser running when it last passed |
| Is it a fixture-server port collision? | **No** | `FixtureServer::start` binds `127.0.0.1:0` |
| Is it parallelism? | **No** | reproduces serially, one test at a time, idle machine |

**And yet it passed hours earlier the same night**, in iteration 173's sweep (`executed=277`,
277 passed / 0 failed) and in iteration 171's (`executed=275`, only `live_160` failing). So the
machine changed in some way none of the checks above captures. **That unknown is this iteration's
subject** — a test that flips from 0/8 to 8/8 without a code change is worth more than the
assertion it makes.

### Re-measured 2026-08-22 — it passes 4/4, and the difference is machine load

Same commit, same machine, four days later, nothing rebuilt: **4/4 PASS**.

| date | result | load average (1/5/15 min) | context |
|---|---|---|---|
| 2026-08-17 | passed (2 full sweeps) | idle | iterations 171 and 173 sweeps |
| 2026-08-18 | **failed 8/8**, and 8/8 at `4d639e2` | **15.55 / 97.57 / 184.24** | immediately after six back-to-back `-j6` sweeps, with Spotlight indexing GBs of fresh profile dirs |
| 2026-08-22 | **passed 4/4** | **6.55 / 6.38 / 7.32** | idle machine |

**The leading hypothesis is therefore no longer "the subscription is broken".** It is that
`assert_network`'s wait is time-bounded (the failing envelope spent `elapsed_ms: 3025` before
giving up) and that under sustained load the events do not arrive inside it. That would put this in
the same class as [[iteration-177-slow3g-assertion-has-two-percent-headroom]] and the launch
timeouts [[iteration-173-live-sweep-port-6000-firefox-does-not-survive]] reclassified: a
time-bounded assertion with no margin, not a broken code path.

**This is correlation, not proof, and Theme B must not assume it.** Two things are still unexplained
and both matter more than the pass/fail flip:

1. `events_in_buffer: 0` means **zero** events, not "the POST was late" — a pure timeout on a busy
   machine would more plausibly show a partial buffer. Explain the zero, or show that zero is what
   a not-yet-started subscription looks like.
2. Whether load is the variable at all is **testable**: load the machine deliberately (the
   `-j6` sweep from [[iteration-180-live-sweep-cost-and-parallelism]] is a ready-made load
   generator) and re-run. If it fails under load and passes idle, that is the answer; if it fails
   idle too, the hypothesis above is wrong and should be struck from this plan.

Theme A (the stderr/stdout diagnosability bug) is **unaffected by any of this** — it is a real
defect in the test's failure reporting whether or not the assertion ever fires again, and it is the
reason none of the above was visible without patching the test by hand.

## Findings, measured 2026-08-22 during implementation

### The load hypothesis is confirmed by experiment, not by correlation

Same commit, same machine, same binary, within one hour:

| condition | load average (1/5/15) | result |
|---|---|---|
| idle | 8.56 / 6.70 / 7.20 → 10.14 / 7.21 / 7.37 | **4/4 PASS** |
| under a `-j6` nextest sweep | 137.80 / 39.82 / 18.95 → 220.08 / 77.99 / 34.38 | **8/8 FAIL** |

The load generator was [[iteration-180-live-sweep-cost-and-parallelism]]'s sweep, exactly as this
plan's step 1b proposed. Every one of the eight failures reported `events_in_buffer: 0` — never a
partial count. So the flip in Theme C is machine load, and the "some unknown state changed"
framing above is superseded.

### `events_in_buffer: 0` is explained, and zero is the only value it could have been

`ff-rdp run` opens a **fresh connection per step**. `execute_assert_network` calls
`network::run_get_events_with_route`, which on the **direct** route arms the `network-event`
watcher *when the step starts*, drains for the step's `timeout`, and unwatches. Firefox's
`watchResources` delivers what happens while watching; it does not replay history. So a request
that completed before the arming is never delivered.

The playbook has **exactly one** request in flight (the `POST /api/auth/sign-in` the `click`
triggers). Losing the arming race therefore loses *all* of it. A partial buffer was never
possible, which is precisely why the zero looked like a broken subscription and was not one. The
plan's own caveat — "a pure timeout on a busy machine would more plausibly show a partial buffer" —
was a reasonable inference and is wrong for N=1.

The **daemon** route does not have this defect: it holds a standing subscription and buffers
across steps.

### What was shipped, and what was deliberately not

Shipped: Theme A in full, plus product diagnostics that make the failure self-explaining. The
panic message below is the real one from a loaded run, with no hand-patching — compare it with the
empty string this plan opened on:

```text
live_runner_page_map_resolution: ff-rdp run exited with non-zero status — status=Some(1) stdout=
{"step":1,"verb":"navigate","ok":true,...,"status":200,...}
{"step":2,"verb":"type","ok":true,...}
{"step":3,"verb":"click","ok":true,"results":{"clicked":true,...}}
{"step":4,"verb":"assert_network","ok":false,"error":"assert_network: no matching network request
 found (...)","elapsed_ms":3018,"diagnostics":{"events_in_buffer":0,"route":"direct",
 "drain_window_ms":2000,"empty_buffer_hint":"direct route: `run` opens a fresh connection per step
 and arms the network watcher only when this step starts, ..."}}
```

**Not shipped: the fix for the race.** Giving `run` a playbook-scoped subscription means making a
deliberately per-step-stateless runner hold state across steps, and it must not disturb the daemon
path. That is [[iteration-181-playbook-scoped-network-subscription]], filed before this PR merges.
`live_62` is therefore left exactly as it is — still red under load. Softening it was forbidden by
this plan's own "Out of scope", and routing it through the daemon would have made it green while
no longer exercising the route that has the defect.

### Theme A, counted

| tier | offending invocations | disposition |
|---|---|---|
| `crates/ff-rdp-cli/tests/live/` + `crates/ff-rdp-core/tests/` | **198** across 60 files (186 rewritten mechanically to `common::output_note`, 12 by hand) | **fixed here** |
| `crates/ff-rdp-cli/tests/e2e/` | **246** | [[iteration-182-e2e-tier-stdout-evidence]] — separate `support` module, and 444 edits in one PR is not reviewable |

`crates/ff-rdp-cli/tests/iter_179_harness_stdout_evidence.rs` is the guard that stops a fourth
instance: it parses every `assert!`/`assert_eq!`/`assert_ne!`/`panic!` invocation in the live trees
(balanced across lines, string-literal aware) and fails if one names `stderr` without `stdout`.

## Why it looked like a parallelism failure, and why that matters

It was first seen failing 3/3 across `-j6` runs during iteration 180's measurements, alongside
`live_96` (which *is* structurally parallel-incompatible). The obvious inference — "a second test
that cannot run in parallel" — was wrong, and only checking it serially disproved it.
[[iteration-180-live-sweep-cost-and-parallelism]] A3 has been corrected accordingly.

## Themes

- **A — Make the assertion say what happened.** `live_62_page_map_index.rs:245` prints only
  `stderr`, and ff-rdp writes errors to **stdout**, so the shipped panic message is
  `ff-rdp run exited with non-zero status — stderr: ` with nothing after it. This is the **third**
  instance of the same bug: [[iteration-169-navigate-status-delivery-and-nav-verb-parity]] fixed it
  for `live_158`, and iteration 172 fixed the sibling case for `live_160`'s daemon reason. Fix it
  here, then **sweep the live suite for every other assertion that prints `stderr` from an ff-rdp
  invocation** — this keeps recurring because nobody has looked for the whole set.
- **B — Establish why the buffer is empty.** Does `ff-rdp run` subscribe to network events for the
  whole playbook, or per-step? If per-step, a `click` that triggers a request the *next* step
  asserts on is a race by construction. `events_in_buffer: 0` after a successful navigate suggests
  the subscription is not live at all during `run`, which would be a product defect affecting every
  `assert_network` user, not just this test.
- **C — Explain the flip.** It passed in two sweeps and then failed 16/16 across two commits within
  hours. Find the state that changed. If it cannot be found, say so explicitly and make the test
  report enough diagnostics that the next occurrence is self-explaining — do not close this by
  waiting for it to pass again.

## Tasks

### A. Diagnosability [2/2]
- [x] `live_62`'s runner assertion prints stdout as well as stderr
- [x] Every other live assertion that prints only `stderr` from an ff-rdp invocation is found and
      fixed, with the count recorded here — **198** in the live tiers; 246 more in the e2e tier,
      split to [[iteration-182-e2e-tier-stdout-evidence]]

### B. The empty buffer [3/3]
- [x] Record when `ff-rdp run` subscribes to network events, and for how long — **direct route:
      armed when the `assert_network` step starts, dropped when it ends, window = the step's
      `timeout` (2000 ms here). Daemon route: standing subscription, buffers across steps.**
- [x] Record whether `events_in_buffer` is ever non-zero in this playbook, at any step — **no.
      It is 1 on a passing run and 0 on every one of the 8 failing runs; there is only one
      request, so it can only ever be 1 or 0.**
- [x] Fix, or record that the runner's contract is per-step and the test's expectation is wrong —
      either is an acceptable outcome, but say which — **recorded: the contract is per-step on the
      direct route, and the test's expectation is a race by construction. The fix is
      [[iteration-181-playbook-scoped-network-subscription]], not this PR.**

### C. The flip [1/2]
- [ ] Reproduce on a second machine, or establish that it is specific to this one — **not done.
      Only one machine was available to this run. What was established instead is that the
      variable is machine *load*, which is reproducible on demand here; whether another machine
      shows the same threshold is untested.**
- [x] Name the state that changed, or record explicitly that it could not be found — **machine
      load. 4/4 PASS at 15-min load ~7; 8/8 FAIL at 1-min load 138–220 under a `-j6` sweep.**

## Acceptance Criteria [3/4]

- [x] The failure message alone is enough to diagnose the next occurrence, without patching the
      test to see it — demonstrated on a real loaded failure; see the envelope quoted above
- [x] `events_in_buffer: 0` is explained — either the subscription window is wrong (fix it) or the
      test asserts something `run` never promised (fix the test, and say so in the plan) —
      **the test asserts something the direct route never promised. Saying so is this plan's
      "Findings" section; fixing it is [[iteration-181-playbook-scoped-network-subscription]].**
- [ ] `live_62_page_map_index::live_runner_page_map_resolution` passes 10/10 serially on the
      machine where it currently fails 8/8, with the run recorded

      **Left unticked deliberately.** The premise is now known to be wrong: the test does not
      "currently fail 8/8" in a fixed sense — it passes 4/4 idle and fails 8/8 under load, on the
      same commit within one hour. There is no machine state on which 10/10 would mean anything.
      Making it pass under load requires the product fix in
      [[iteration-181-playbook-scoped-network-subscription]], and this PR does not contain it.
      Every way to tick this box from here — running the playbook through the daemon, widening the
      step timeout, retrying — would be the softening this plan's "Out of scope" forbids.
- [x] If the cause is environmental and cannot be pinned, that is stated plainly rather than the
      test being relaxed until it goes green — it *was* pinned (load), the mechanism is named
      (the direct-route arming race), and the test is untouched

## Out of scope

- **Relaxing or `#[ignore]`-ing the assertion to make the suite green.** The precondition-loudness
  rule from [[iteration-146-live-suite-reliability]] Theme B applies: a detector that gets softened
  because it fired is worse than no detector.
- **Parallel execution of the live tier** — [[iteration-180-live-sweep-cost-and-parallelism]].
- **`live_96`'s global prune precondition**, which *is* structurally parallel-incompatible and is
  handled in 180.

## References

- [[iteration-180-live-sweep-cost-and-parallelism]] — the measurement session this surfaced in
- [[iteration-169-navigate-status-delivery-and-nav-verb-parity]] — the first stderr/stdout fix
- [[iteration-164-two-failures-the-158-sweep-uncovered]] — prior art on network-event subscription
  being destroyed by an adjacent call
