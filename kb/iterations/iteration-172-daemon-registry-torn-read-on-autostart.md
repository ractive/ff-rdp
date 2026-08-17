---
title: "Iteration 172: the registry writer locks the published path, so autostart reads a zero-byte record and silently falls back to direct"
type: iteration
date: 2026-08-16
status: in-review
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
tags:
  - iteration
  - daemon
  - registry
  - reliability
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

## Observed again — iteration 171's live sweep (2026-08-17)

A fourth test in the same failure class, folded in by iter-171's carry-over sweep:

```text
FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
LIVE_SWEEP_SUMMARY executed=275 skipped=0 preexisting=0 total=275  -> 274 passed / 1 failed

live_160_envelope_honesty::live_160_ref_click_asserts_handler_effect   FAILED
  live_160_ref_click_asserts_handler_effect: the proxy daemon did not start for
  Firefox on port 63690
```

Passes in isolation on an idle machine in 8.88 s, after a CLI tier that took 2313 s — the same
load-dependence as the other three.

**Its cause is NOT confirmed to be this plan's.** The assertion prints only "the proxy daemon did
not start"; it never surfaces `meta.daemon_fallback`, so there is no evidence either way about the
zero-byte registry read. It is filed here because it is the same *class* (daemon autostart failing
under sweep load), not because the mechanism is established. **Theme A must confirm or reject it
explicitly rather than assuming it in** — assuming it in is exactly how iterations 172 and 173 got
filed against a contaminated sweep in the first place.

That gap is itself a carry-over: a live test that cannot say *why* the daemon did not start sends
the next reader hunting. iter-169 fixed the same shape in `live_158` (the assertion printed
`stderr` while ff-rdp writes errors to `stdout`). Whatever Theme C does about reporting should make
every daemon-start assertion in the live suite print the fallback reason it already has in hand.

## Why this is not iteration 164

[[iteration-164-two-failures-the-158-sweep-uncovered]] fixed the *harness* half — `with_daemon`
slept a fixed 500 ms instead of polling. This is the *product* half, one layer down: the poll is
now in place and running, and it is the individual read inside the poll that fails. A poll that
retries would have recovered; the message shows it did not treat the parse error as retryable.

## Observed again — iteration 170's live sweep (2026-08-17)

Folded in by iter-170's carry-over sweep. Two more tests carry this exact signature, so the
symptom set this iteration must clear is now three tests, not one:

```text
FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
LIVE_SWEEP_SUMMARY executed=274 skipped=0 preexisting=0 total=274  -> 271 passed / 3 failed

live_128_network_output_fidelity::live_128_meta_route          FAILED
  route "direct", meta.daemon_fallback: "daemon started but did not register within 20s
  (registry write raced or was slow): ... parsing registry at ~/.ff-rdp/daemon.49263.json:
  EOF while parsing a value at line 1 column 0"

live_134_meta_route_all_commands::live_134_meta_route_all_commands   FAILED
  `click` reported route "direct", warning daemon_autostart_failed:
  "daemon started but did not register within 20s (spawn died before the registry write):
  ... parsing registry at ~/.ff-rdp/daemon.51326.json: EOF while parsing a value at line 1
  column 0"
```

Both pass when re-run in isolation (3/3 ok, 60 s), which is consistent with the located cause: the
lock window is short, so a reader only lands in it when the machine is loaded. Note the two
`daemon_fallback` reasons differ — "registry write raced or was slow" vs "spawn died before the
registry write" — and the second one names a *dead spawn* while still reporting the zero-byte
parse error. Theme A should establish whether that second phrasing is the same defect or a
distinct one wearing the same error text; do not assume.

Theme C now has a measured cost: this route downgrade has silently reddened three unrelated tests
across two sweeps.

## Theme A — the reproduction, run 2026-08-17

**The diagnosis in the "Added 2026-08-17" block is correct.** Every step of `dogfood_path` ran; the
outputs are below verbatim.

### 1. Baseline — a routed command reports `daemon`

```console
$ ff-rdp --port 6000 --verbose network --jq '.meta.route'
"daemon"
```

### 2. Planted zero-byte record, `main` binary (21e777a, built in a scratch worktree)

```console
$ ff-rdp --port 6000 daemon stop; rm -f ~/.ff-rdp/daemon.6000.json; : > ~/.ff-rdp/daemon.6000.json
$ ls -la ~/.ff-rdp/daemon.6000.json
-rw-r--r--  1 james  staff  0 Aug 17 21:00 /Users/james/.ff-rdp/daemon.6000.json
$ <main>/ff-rdp --port 6000 --verbose network
  "daemon_fallback": "warning: failed to check daemon status: parsing registry at
   /Users/james/.ff-rdp/daemon.6000.json: EOF while parsing a value at line 1 column 0
   (check /Users/james/.ff-rdp/daemon.log for details)",
  "route": "direct"
--- elapsed: 11s
```

The error text matches the sweep failures character for character.

### 3. Racing a real writer — the file *is* observed empty, and it is the lock step

A probe added to a scratch checkout of `main` (not committed) that takes the lock exactly the way
`write_registry_in` did, then looks at the published path from a reader's point of view:

```console
$ cargo test -p ff-rdp-cli --bins probe_172 -- --nocapture     # on main, 21e777a
PROBE: published path exists=true len=0
PROBE: read error chain -> parsing registry at .../daemon.6000.json:
                           EOF while parsing a value at line 1 column 0
panicked: PROBE FAILS ON MAIN: the lock step published a 0-byte record
```

So the answers to the Theme A tasks are:

- **Is the registry file observed empty or truncated mid-write?** Empty, never truncated — the
  record is zero bytes for the entire span between the lock `open` and the `rename`. It is
  reproducible on demand (100 % of probe runs), not a rare race: the *reader* landing inside the
  span is what is rare.
- **Is the registry write atomic today?** Yes. `serde_json::to_string_pretty` → `daemon.<port>.json.tmp`
  → `fs::rename`, guarded by `write_is_atomic_tmp_cleaned_up`. The title's "torn read" mechanism is
  wrong and the retitled one is right.

### 4. After the fix, same planted file

```console
$ rm -f ~/.ff-rdp/daemon.6000.json; : > ~/.ff-rdp/daemon.6000.json
$ ff-rdp --port 6000 --verbose network --jq '{route: .meta.route, fallback: .meta.daemon_fallback}'
{"route":"daemon","fallback":null}
--- elapsed: 1s
$ ls -la ~/.ff-rdp/daemon.6000.*
-rw-------  232  daemon.6000.json
-rw-------    0  daemon.6000.spawn.lock
-rw-------    0  daemon.6000.write.lock      ← the lock is now a sibling
```

### 5. The "spawn died before the registry write" phrasing — same defect

The plan asked whether the second `daemon_fallback` wording was a distinct bug wearing the same
error text. It is not. `classify_registry_wait_failure` (`client.rs:491`) re-reads the registry to
decide what to say; on the zero-byte file that read returns `Err`, which falls through to the
catch-all `_` arm and prints **"spawn died before the registry write"** about a daemon that was
perfectly alive. One cause, two wordings — and the misclassification made the reports actively
misleading.

### 6. `live_160_ref_click_asserts_handler_effect` — NOT confirmed, and not assumed in

Its assertion still carried no evidence, so nothing in this reproduction attributes it to this
defect. What *is* now established is that its failure mode is reachable from this defect:
`with_daemon` gives up when `daemon status` never reports a running daemon, and `daemon status`
reads the same registry, so a zero-byte record makes it report "not running" for the full 30 s
budget. Reachable is not the same as responsible — a Firefox launch stall under sweep load produces
an identical `None`. The honest disposition is therefore: **cause unknown, evidence now collected
on the next occurrence** (Theme C below). The acceptance criterion covering it is left unticked.

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

## What was changed

Three layers, in decreasing order of how much of the defect each one removes.

### 1. The writer stops locking the published path (the fix)

`registry::acquire_registry_write_lock_in` (new) takes the exclusive lock on a sibling
`daemon.<port>.write.lock`, exactly as `acquire_spawn_lock_in` has done since iter-100 and for the
reason its own doc comment already gave. `write_registry_in` calls it and holds it across the tmp
write *and* the rename. The published record now only ever comes into existence via `fs::rename`,
so a reader sees either no file or a complete one.

The two lock helpers were collapsed onto one `acquire_file_lock(path, what)` and one `FileLock`
guard (`SpawnLock` was a one-field wrapper with the same body), and the stale-lock GC learned the
new filename via `parse_write_lock_port` / `parse_lock_port` — otherwise iter-172 would have
introduced a second class of zero-byte file that accumulates in `~/.ff-rdp/` forever, which is
dogfood-62 #9 all over again.

### 2. A zero-byte record reads as absent, not as corruption

`read_registry_in` returns `Ok(None)` for an empty (or whitespace-only) file. Argued on its own
merits rather than assumed in: layer 1 stops *this* build producing the file, but a copy left by a
pre-iter-172 build sits in `~/.ff-rdp/` indefinitely, and while it does it poisons that port
**permanently** — every invocation, not just one inside a race window. Deliberately narrow: only a
zero-length file is absence. Non-empty bytes that do not parse are still an error
(`read_corrupt_json_returns_error` is unchanged), because that really is corruption and swallowing
it would hide a genuine problem.

### 3. The autostart poll retries an unreadable read

`wait_for_registry` bailed on the first `Err`. It is polling a file another process is actively
producing, so "cannot read it yet" is a normal intermediate state, not a verdict — the only reason
it was terminal is that `read_registry` split "no record" and "unreadable record" into different
arms. It now keeps polling to its deadline and reports the last read error in the timeout message,
so a genuinely corrupt registry still names itself. This also makes the caller's
"did not register within 20s" *true*: on `main` that sentence was printed after a wait of roughly
50 ms.

Extracted `wait_for_registry_in(dir, …)` so the loop is unit-testable against a tempdir instead of
the real `~/.ff-rdp`.

### Rejected

- **Reader-only fix (retry, leave the writer alone).** Rejected: it would have left the product
  publishing an empty record on every single write and hoped every reader retried. `daemon status`,
  `doctor` and `classify_registry_wait_failure` all read the registry too.
- **Making `find_running_daemon` swallow *all* read errors.** Rejected: a genuinely corrupt record
  is worth a loud fallback, and `doctor` surfaces it. Only the zero-byte case — which is
  unambiguously "no record" — was reclassified.
- **Removing the empty file in a GC pass.** Rejected as a fix (it is a cleanup, not a cure) though
  the `rename` in layer 1 overwrites it on the next successful registration anyway.

## Tasks

### A. Reproduce
- [x] Run every step of `dogfood_path` and paste actual outputs into this plan
- [x] Record whether the registry file is observed empty or truncated mid-write, and how often
- [x] Record whether the registry write is atomic (temp + rename) today

### B. Fix
- [x] The chosen writer and/or reader change, with the rejected alternatives recorded
- [x] Unit test: a torn (empty) registry read is retried, not treated as terminal
      (`unit_172_wait_for_registry_keeps_polling_an_unreadable_record`,
      `unit_172_wait_for_registry_recovers_after_a_bad_read`)
- [x] Live test that exercises autostart registration
      (`live_172_zero_byte_registry_does_not_downgrade_to_direct`,
      `live_172_published_record_is_complete_and_lock_is_a_sibling`)

### C. Reporting
- [x] Record the decision on how loudly a route downgrade is reported

**Decision: the envelope reporting is left as it is; the *test harness* reporting is what was
wrong.**

`meta.route` is already unconditional (iter-128 Theme D) and `meta.daemon_fallback` already carries
the reason under `--verbose` (iter-164). Escalating a downgrade to a hard error was considered and
rejected: falling back to a direct connection is the correct behaviour when the daemon genuinely
cannot start, and a command that *worked* must not exit non-zero. Making the warning unconditional
on stderr was also rejected — it would break the JSON-only output contract for the common,
harmless case.

What actually cost three sweeps a red was on the other side of the fence: the live harness threw
the reason away. `LiveFirefox::with_daemon` returned a bare `Option`, so eighteen live tests could
only say "the proxy daemon did not start" and nothing more. `with_daemon_or_reason` (new) returns
the reason — the autostart trigger now runs `--verbose` so `meta.route` / `meta.daemon_fallback`
are present, and `daemon_route_note` extracts them. `with_daemon` delegates and prints the reason
to stderr, so **all eighteen existing call sites gained the diagnostic without being touched**;
`live_160_envelope_honesty` (the test whose cause could not be established) puts it directly in its
panic message. This is the general form of the fix iter-169 applied to `live_158`.

## Acceptance Criteria [4/4]

- [x] The Theme A reproduction is recorded, including the decision if it does not reproduce
      [2026-08-17: reproduced deterministically — the probe on `main` reports
      `published path exists=true len=0`, and the same planted record gives `route: "direct"` on
      `main` vs `route: "daemon"` here]
- [x] An empty or truncated registry file no longer ends the autostart wait early — asserted by a
      test that fails on `main`
      [`unit_172_wait_for_registry_keeps_polling_an_unreadable_record` asserts elapsed ≥ the full
      budget; `main` returns in ~0 ms. Plus `read_zero_byte_registry_is_treated_as_absent` and
      `a_blocked_writer_never_publishes_an_empty_record`]
- [x] `live_128_meta_route` passes in a contended dual-gate sweep
      [2026-08-17: passed in `executed=277`, both gates, CLI tier 2320 s — as did
      `live_134_meta_route_all_commands`, `live_123_daemon_autostart_tabless` and
      `live_160_ref_click_asserts_handler_effect`]
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q`
      clean, plus a dual-gate live sweep

## Live sweep (2026-08-17)

```text
FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep
LIVE_SWEEP_SUMMARY executed=277 skipped=0 preexisting=0 total=277   -> 276 passed / 1 failed
  ff-rdp-cli  live tier : 267 passed / 1 failed   (2320.36 s)
  ff-rdp-core      tier :   9 passed / 0 failed   (1 + 3 + 3 + 2)
```

Hand-started Firefox on port 6000 (`/tmp/ff-rdp-sweep-profile-6000`), orphan check clean before
the run (`pgrep -f 'ff-rdp/profiles'` → 0). Nothing external interfered with this run.

`executed=277` against the baseline-of-record's `executed=275` on `main` at 21e777a: **+2, exactly
the two `live_172_*` tests this PR adds.** No test was lost.

All four tests of this plan's symptom set passed:

```text
live_128_network_output_fidelity::live_128_meta_route                         ok
live_134_meta_route_all_commands::live_134_meta_route_all_commands            ok
live_123_daemon_autostart_and_registry::live_daemon_autostart_tabless         ok
live_160_envelope_honesty::live_160_ref_click_asserts_handler_effect          ok
live_172_zero_byte_registry::live_172_zero_byte_registry_does_not_downgrade_to_direct   ok
live_172_zero_byte_registry::live_172_published_record_is_complete_and_lock_is_a_sibling ok
```

## Carry-over

| # | Item | Where it came from | Disposition |
|---|---|---|---|
| 1 | `live_109_throttle_block::live_throttle_slow3g_slows_fetch` FAILED — `baseline=409ms throttled=779ms` (1.90×) against a `>= 2.0×` assertion | this sweep, the only non-green line | **file** — [[iteration-177-slow3g-assertion-has-two-percent-headroom]] (`check-iteration-plan: OK`). Re-ran isolated and idle: 378 ms / 775 ms = **2.05×**, i.e. the assertion has ~2 % headroom on a *good* run. The throttled figure moved 0.5 % across a 2320 s load swing; the baseline moved 8 %. Not a throttling regression, and not dismissible as "environmental" |
| 2 | `live_128_meta_route`, `live_134_meta_route_all_commands`, `live_123_daemon_autostart_tabless` — failed in iters 168 and 170's sweeps, **passed this run** | this plan's symptom set | **closed in this PR** — the sibling write lock plus the zero-byte read. Not resting on the green run: the mechanism was confirmed by probe on `main` and by the planted-record before/after, and `live_172_zero_byte_registry_does_not_downgrade_to_direct` fails on `main` |
| 3 | `live_160_ref_click_asserts_handler_effect` — failed in iter-171's sweep, **passed this run** | this plan, folded in as cause-unknown | **no plan, with a stated reason.** Its cause was never established and this iteration did not establish it either (see Theme A §6): the failure mode is *reachable* from this defect but a Firefox launch stall under load produces an identical `None`. One green run is not evidence. What has changed is that it can no longer fail silently — `with_daemon_or_reason` now prints `meta.route` / `meta.daemon_fallback`. **If it fails again, the printed reason attributes it, and it needs its own plan** |
| 4 | `xtask live-sweep` classifies any live source containing the bare substring `firefox_port` as needing a pre-existing Firefox on port 6000, so a CLI test that merely reads the registry field is silently moved into the wrong tier | hit while writing `live_172_published_record_is_complete_and_lock_is_a_sibling`; caught by `test_158_real_core_targets_are_preexisting` | **fold** — added as Theme D + a fifth AC to [[iteration-173-live-sweep-port-6000-firefox-does-not-survive]], which owns sweep classification. Worked around here by not naming the field, with a comment saying why |
| 5 | `ff-rdp daemon stop` on port 6000 reported `stopped Firefox (pid 64823) but port 6000 is still listening after 8 s`, while the hand-started browser that actually owned 6000 (pid 62618) was untouched and still serving | observed during the Theme A repro | **no plan, with a stated reason.** The message is accurate and the behaviour is the designed one (`live_90`/`live_158` assert that `daemon stop` also stops the Firefox the daemon recorded); it named a PID it had recorded, declined to escalate further, and told the operator to run `lsof`. Nothing measured is left to act on. **If it is ever shown to signal a Firefox ff-rdp did not launch, that is an [[iteration-110-post-batch-live-sweep]]-class product defect and needs its own plan** |

## Out of scope

- Reworking daemon autostart's 20 s budget. The budget was not the problem here; a single
  unretried read was.

## References

- [[iteration-168-livefirefox-drop-does-not-wait-for-exit]] — the sweep that surfaced this
- [[iteration-164-two-failures-the-158-sweep-uncovered]] — the harness-side daemon-readiness poll,
  which this is *not* a repeat of
