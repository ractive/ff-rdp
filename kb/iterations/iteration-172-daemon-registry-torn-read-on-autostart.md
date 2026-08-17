---
title: "Iteration 172: the registry writer locks the published path, so autostart reads a zero-byte record and silently falls back to direct"
type: iteration
date: 2026-08-16
status: planned
branch: iter-172/registry-lock-on-published-path
depends_on: []
first_call_sites: []
dogfood_path: |
  # Product defect. Cause located 2026-08-17 by reading the writer: the registry
  # write takes its exclusive lock by opening the PUBLISHED path with
  # create(true) (registry.rs:121-127), so a zero-byte daemon.<port>.json exists
  # from the moment a write starts until the rename lands. Readers in that
  # window parse zero bytes. Reproduce it deterministically — do NOT hunt for it
  # under load.

  # 1. Baseline: the routed command must report meta.route == "daemon".
  ff-rdp --port <port> --verbose network --jq '.meta.route'
  # → EXPECTED on a quiet machine: "daemon"

  # 2. Deterministic repro of the empty read, no load required. Remove any
  #    existing record, then create the file the way the writer's lock step
  #    does — empty — and read it back through the product:
  RD=~/.ff-rdp; PORT=7401
  rm -f "$RD/daemon.$PORT.json"
  : > "$RD/daemon.$PORT.json"          # what create(true) leaves behind
  ff-rdp --port $PORT --verbose network --jq '.meta.route'
  # → EXPECTED (the defect): burns the full 20 s autostart budget, then
  #   "daemon_fallback": "... parsing registry at .../daemon.$PORT.json:
  #    EOF while parsing a value at line 1 column 0 — connecting directly"
  #   and meta.route == "direct". This is the exact envelope observed once in
  #   iteration 168's sweep (2026-08-16).

  # 3. Show it is the lock step, not a torn write: write_registry_in already
  #    does tmp-file + fs::rename (registry.rs:132-152, guarded by the unit test
  #    write_is_atomic_tmp_cleaned_up). Confirm by racing a real writer —
  #    hold the lock in one process and stat/read the path from another:
  #    the file is present and zero-length before the rename.
tags: [iteration, daemon, registry, reliability]
---

# Iteration 172: the daemon registry is read while it is being written

Carry-over from [[iteration-168-livefirefox-drop-does-not-wait-for-exit]]'s dual-gate live sweep
(2026-08-16, `executed=270 skipped=0 preexisting=0`).

## What was observed

> **Added 2026-08-17 — cause located by reading the writer. It is not a torn write, and the
> mechanism in the title is wrong.**
>
> `write_registry_in` (`crates/ff-rdp-cli/src/daemon/registry.rs:113`) already writes atomically —
> serialize to `daemon.<port>.json.tmp`, then `fs::rename` onto the target, guarded by the unit
> test `write_is_atomic_tmp_cleaned_up`. A torn or half-written file cannot come out of that path.
>
> The empty file comes from the **lock acquisition**, twenty-five lines earlier:
>
> ```rust
> // registry.rs:121-127 — "Acquire an exclusive lock on the registry file (creates it if absent)."
> let lock_file = fs::OpenOptions::new()
>     .create(true)        // ← creates a ZERO-BYTE daemon.<port>.json right here
>     .truncate(false)
>     .write(true)
>     .open(&registry_path)
> ```
>
> The writer locks **the registry path itself**. So the instant a write begins, an empty
> `daemon.<port>.json` exists; content only appears at the `rename`. Any reader polling in that
> window parses zero bytes and gets `EOF while parsing a value at line 1 column 0` — the observed
> error exactly. The autostart path polls this file for up to 20 s, so it is reachable whenever a
> read lands between the open and the rename.
>
> `acquire_spawn_lock_in` (same file, ~line 294) already solved this for the *spawn* lock by using
> a dedicated `daemon.<port>.spawn.lock`, and its doc comment states the reason: "so the lock
> lifetime is independent of registry write/rename churn". The registry writer never got the same
> treatment. That inconsistency is the defect.
>
> **This supersedes an earlier note that called the plan likely-obsolete.** That note reasoned the
> empty file was a side effect of a human SIGKILLing processes during the filing sweep (they did,
> 21:37–21:40, inside the 21:31–21:45 CLI tier). A kill cannot produce this file either — rename is
> atomic — so the external interference explains iterations 168's *other* sweep failures but not
> this one. `live_128_meta_route` passing in two later sweeps means the race is narrow, not absent.
>
> Retitle the plan when you pick it up: this is a **lock-on-the-published-path** bug, not a torn
> read. Theme A no longer needs a load repro — it needs a deterministic one (hold the lock open in
> one process, read the path from another). The reader-side hardening in Theme B still stands on
> its own: an unreadable registry should not burn the full 20 s budget and then degrade silently to
> `route: "direct"`.

`live_128_network_output_fidelity::live_128_meta_route` failed once in that sweep. The assertion
message carries the product's own diagnosis:

```text
"daemon_fallback": "warning: daemon started but did not register within 20s (registry write
 raced or was slow): reading daemon registry while waiting: parsing registry at
 /Users/james/.ff-rdp/daemon.53497.json: EOF while parsing a value at line 1 column 0
 — connecting directly (check /Users/james/.ff-rdp/daemon.log for details)"
```

`EOF while parsing a value at line 1 column 0` is an **empty file**, not malformed JSON. The
reader opened the registry between the writer creating it and the writer filling it — which, as
the note above establishes, is a window the writer opens deliberately by taking its lock on the
published path. The client then treated that read as "not registered", burned its full 20 s
budget, and fell back to a direct connection — so a command the caller asked to route through the
daemon silently did not.

It passes in isolation; it failed at the tail of a 14-minute contended tier. Contention widens the
window but does not create it.

## Why this is not iteration 164

[[iteration-164-two-failures-the-158-sweep-uncovered]] fixed the *harness* half — `with_daemon`
slept a fixed 500 ms instead of polling. This is the *product* half, one layer down: the poll is
now in place and running, and it is the individual read inside the poll that fails. A poll that
retries would have recovered; the message shows it did not treat the parse error as retryable.

## Themes

- **A — Reproduce deterministically before changing anything** (revised 2026-08-17 — this no
  longer needs load). Run the `dogfood_path`: create the zero-byte record by hand, and race a real
  writer to confirm the file is present-and-empty between the lock `open` and the `rename`. If the
  file is never observed empty, this diagnosis is wrong and the 20 s exhaustion has another cause;
  say so and close `obsolete`.
- **B — Fix the writer's lock target, and decide separately about the reader.** The write is
  already atomic (temp + `rename`); what publishes an empty file is locking the **published path**
  with `create(true)`. Move the lock to a sibling — `acquire_spawn_lock_in` already does this with
  `daemon.<port>.spawn.lock` for the same stated reason — so the record only ever exists complete.
  Then decide whether the reader should *also* treat a parse error as "not yet registered" and
  retry: with the writer fixed a retry is defence in depth, not the fix, and it should be argued
  for on its own merits rather than assumed.
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
