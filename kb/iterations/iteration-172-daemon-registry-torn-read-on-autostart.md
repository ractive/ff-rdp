---
title: "Iteration 172: daemon autostart reads a half-written registry file and gives up, silently falling back to direct"
type: iteration
date: 2026-08-16
status: planned
branch: iter-172/daemon-registry-torn-read
depends_on: []
first_call_sites: []
dogfood_path: |
  # Product defect, surfaced by iteration 168's dual-gate sweep under contention.
  # It does not reproduce in isolation — `live_128_meta_route` passes on a
  # quiet machine — so reproduce it under load before changing anything.

  # 1. Baseline: the routed command must report meta.route == "daemon".
  ff-rdp --port <port> --verbose network --jq '.meta.route'
  # → EXPECTED on a quiet machine: "daemon"

  # 2. Under load, watch for the fallback. Run the sweep's own test:
  FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live -q -- --ignored \
    --exact live_128_network_output_fidelity::live_128_meta_route
  # → OBSERVED once in iteration 168's sweep (2026-08-16), at the tail of a
  #   14-minute CLI tier:
  #   "daemon_fallback": "warning: daemon started but did not register within
  #    20s (registry write raced or was slow): reading daemon registry while
  #    waiting: parsing registry at /Users/james/.ff-rdp/daemon.53497.json:
  #    EOF while parsing a value at line 1 column 0 — connecting directly"
  #   and meta.route == "direct" instead of "daemon".

  # 3. Read the registry file while a daemon is starting, repeatedly, and
  #    record how often it is observed empty or truncated. That is the whole
  #    question: is the reader tolerating a torn write, or is the writer
  #    non-atomic?
  #    Check whether the write goes through a temp file + rename, or writes
  #    the target path in place.
tags: [iteration, daemon, registry, reliability]
---

# Iteration 172: the daemon registry is read while it is being written

Carry-over from [[iteration-168-livefirefox-drop-does-not-wait-for-exit]]'s dual-gate live sweep
(2026-08-16, `executed=270 skipped=0 preexisting=0`).

## What was observed

> **Added 2026-08-17 — the trigger was almost certainly external, and this did not reproduce.**
> The sweep this plan was filed from was contaminated: a human killed Firefox processes on that
> machine between 21:37 and 21:40, inside the CLI tier's 21:31–21:45 window. A SIGKILL landing on a
> daemon mid-registry-write is a sufficient explanation for the empty file below, so the "torn
> write under load" premise is unsupported by this observation. Two subsequent dual-gate sweeps on
> `main` at `4d639e2` (`executed=270 skipped=0 preexisting=0`, one of them fully clean at
> 269 passed / 1 failed) had `live_128_meta_route` **pass** both times.
>
> Do **not** open this as "reproduce the torn read under load" — that reproduction may not exist.
> What survives is narrower and still worth fixing on its own terms: an empty or truncated
> registry file costs the full 20 s budget and then degrades **silently** to `route: "direct"`.
> That is a robustness defect whatever truncated the file, and it is checkable without a repro
> (write an empty `daemon.<pid>.json` by hand and watch the fallback). Check whether the writer
> uses temp-file + rename; if it already does, close this plan obsolete rather than hunting a race
> that a kill explains.

`live_128_network_output_fidelity::live_128_meta_route` failed once in that sweep. The assertion
message carries the product's own diagnosis:

```text
"daemon_fallback": "warning: daemon started but did not register within 20s (registry write
 raced or was slow): reading daemon registry while waiting: parsing registry at
 /Users/james/.ff-rdp/daemon.53497.json: EOF while parsing a value at line 1 column 0
 — connecting directly (check /Users/james/.ff-rdp/daemon.log for details)"
```

`EOF while parsing a value at line 1 column 0` is an **empty file**, not malformed JSON. The
reader opened the registry between the writer creating it and the writer filling it. The client
then treated a torn read as "not registered", burned its full 20 s budget, and fell back to a
direct connection — so a command the caller asked to route through the daemon silently did not.

It passes in isolation; it failed at the tail of a 14-minute contended tier.

## Why this is not iteration 164

[[iteration-164-two-failures-the-158-sweep-uncovered]] fixed the *harness* half — `with_daemon`
slept a fixed 500 ms instead of polling. This is the *product* half, one layer down: the poll is
now in place and running, and it is the individual read inside the poll that fails. A poll that
retries would have recovered; the message shows it did not treat the parse error as retryable.

## Themes

- **A — Reproduce under load before changing anything.** Run the `dogfood_path`. If the registry
  file is never observed empty, this diagnosis is wrong and the 20 s exhaustion has another cause;
  say so and close `obsolete`.
- **B — Decide whether to fix the writer, the reader, or both.** Atomic write (temp file +
  `rename`) makes a torn read impossible; treating a parse error as "not yet registered" and
  retrying makes it survivable. These are not alternatives — argue explicitly for whichever
  combination lands, because a retry alone leaves a real window and an atomic write alone still
  breaks if a reader sees a *missing* file as an error.
- **C — A silent route downgrade is its own defect.** The command reported `route: "direct"`
  after being asked for the daemon, with the reason only in `meta.daemon_fallback` and a warning.
  Decide whether that is loud enough, given that this cost an unrelated test a red.

## Tasks

### A. Reproduce
- [ ] Run every step of `dogfood_path` and paste actual outputs into this plan
- [ ] Record whether the registry file is observed empty or truncated mid-write, and how often
- [ ] Record whether the registry write is atomic (temp + rename) today

### B. Fix
- [ ] The chosen writer and/or reader change, with the rejected alternatives recorded
- [ ] Unit test: a torn (empty) registry read is retried, not treated as terminal
- [ ] Live test that exercises autostart registration

### C. Reporting
- [ ] Record the decision on how loudly a route downgrade is reported

## Acceptance Criteria [0/4]

- [ ] The Theme A reproduction is recorded, including the decision if it does not reproduce
- [ ] An empty or truncated registry file no longer ends the autostart wait early — asserted by a
      test that fails on `main`
- [ ] `live_128_meta_route` passes in a contended dual-gate sweep
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q`
      clean, plus a dual-gate live sweep

## Out of scope

- Reworking daemon autostart's 20 s budget. The budget was not the problem here; a single
  unretried read was.

## References

- [[iteration-168-livefirefox-drop-does-not-wait-for-exit]] — the sweep that surfaced this
- [[iteration-164-two-failures-the-158-sweep-uncovered]] — the harness-side daemon-readiness poll,
  which this is *not* a repeat of
