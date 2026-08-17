---
title: "Iteration 179: live_62's runner assertion sees an empty network buffer, and now fails 8/8 on a machine where it used to pass"
type: iteration
date: 2026-08-18
status: planned
branch: iter-179/runner-network-buffer-empty
depends_on: []
first_call_sites: []
dogfood_path: |
  # Product-or-harness boundary defect, surfaced while measuring iteration 178.
  # It reproduces SERIALLY on an idle machine — do not chase it as a
  # parallelism artifact, which is what it first looked like.

  # 1. Reproduce. Expect FAILED (measured 8/8 on 2026-08-18).
  FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 \
    cargo test -q -p ff-rdp-cli --test live -- --ignored --exact \
    live_62_page_map_index::live_runner_page_map_resolution

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
[[iteration-178-live-sweep-cost-and-parallelism]]. It first appeared as a parallel-only failure
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

## Why it looked like a parallelism failure, and why that matters

It was first seen failing 3/3 across `-j6` runs during iteration 178's measurements, alongside
`live_96` (which *is* structurally parallel-incompatible). The obvious inference — "a second test
that cannot run in parallel" — was wrong, and only checking it serially disproved it.
[[iteration-178-live-sweep-cost-and-parallelism]] A3 has been corrected accordingly.

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

### A. Diagnosability [0/2]
- [ ] `live_62`'s runner assertion prints stdout as well as stderr
- [ ] Every other live assertion that prints only `stderr` from an ff-rdp invocation is found and
      fixed, with the count recorded here

### B. The empty buffer [0/3]
- [ ] Record when `ff-rdp run` subscribes to network events, and for how long
- [ ] Record whether `events_in_buffer` is ever non-zero in this playbook, at any step
- [ ] Fix, or record that the runner's contract is per-step and the test's expectation is wrong —
      either is an acceptable outcome, but say which

### C. The flip [0/2]
- [ ] Reproduce on a second machine, or establish that it is specific to this one
- [ ] Name the state that changed, or record explicitly that it could not be found

## Acceptance Criteria [0/4]

- [ ] The failure message alone is enough to diagnose the next occurrence, without patching the
      test to see it
- [ ] `events_in_buffer: 0` is explained — either the subscription window is wrong (fix it) or the
      test asserts something `run` never promised (fix the test, and say so in the plan)
- [ ] `live_62_page_map_index::live_runner_page_map_resolution` passes 10/10 serially on the
      machine where it currently fails 8/8, with the run recorded
- [ ] If the cause is environmental and cannot be pinned, that is stated plainly rather than the
      test being relaxed until it goes green

## Out of scope

- **Relaxing or `#[ignore]`-ing the assertion to make the suite green.** The precondition-loudness
  rule from [[iteration-146-live-suite-reliability]] Theme B applies: a detector that gets softened
  because it fired is worse than no detector.
- **Parallel execution of the live tier** — [[iteration-178-live-sweep-cost-and-parallelism]].
- **`live_96`'s global prune precondition**, which *is* structurally parallel-incompatible and is
  handled in 178.

## References

- [[iteration-178-live-sweep-cost-and-parallelism]] — the measurement session this surfaced in
- [[iteration-169-navigate-status-delivery-and-nav-verb-parity]] — the first stderr/stdout fix
- [[iteration-164-two-failures-the-158-sweep-uncovered]] — prior art on network-event subscription
  being destroyed by an adjacent call
