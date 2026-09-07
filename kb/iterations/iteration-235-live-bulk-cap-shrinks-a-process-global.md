---
title: "Iteration 235: the two standing live-suite defects: live_bulk_cap's process-global cap and live_166's 200-on-304"
type: iteration
date: 2026-08-24
status: in-review
branch: iter-235/live-suite-defects
depends_on: [196]
first_call_sites: []
dogfood_path: |
  # 1. The last writer. iter-196 removed every shrink of MAX_FRAME_BYTES_CELL
  #    from the ff-rdp-core unit-test binary; this one survives, in the CLI's
  #    live-test binary:
  grep -rn "set_max_frame_bytes" crates/ff-rdp-cli/tests/
  #    expected TODAY: crates/ff-rdp-cli/tests/live/live_bulk_cap.rs — two calls,
  #                    one of them set_max_frame_bytes(1024)
  #    expected AFTER: no call that lowers the cap

  # 2. The blast radius is a whole test binary, not one test. Anything in the
  #    live suite that parses a frame over 1 KiB while this test holds the cap
  #    gets FrameTooLarge — a screenshot data URL or a longString body does:
  grep -rn "recv_bulk_with_handler\|\.recv()" crates/ff-rdp-cli/tests/live/ | wc -l
  #    expected TODAY: a two-digit count, none of them synchronised with the cap

  # 3. It has already happened once — DEC-022, iter-114, live_console_no_double_delivery
  #    went red on a leaked 1 KiB cap. The RAII guard added then fixes the leak
  #    (sequential), not the window (concurrent).

  # 4. After the fix, the live suite must still prove the cap rejects an
  #    oversized bulk announcement promptly:
  FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli --test live live_bulk_cap
  #    expected: pass, with no process-global mutation in the diff
  # --- from iteration 236 ---
  ff-rdp launch --headless
  ff-rdp navigate https://example.com --jq '.results.status'
  ff-rdp navigate https://example.com --jq '.results.status'
  # second call on a warm cache: expected 304, not 200 — the behaviour live_166 mis-asserts
tags: [iteration, testing, flaky, ff-rdp-cli, live-tests, carry-over, test-reliability]
---

# Iteration 235: the two standing live-suite defects: live_bulk_cap's process-global cap and live_166's 200-on-304

> **Renumbered 207 → 235 on 2026-09-06** so the pending queue runs as one contiguous sweep (DEC-051). Older PRs, commits and sweep logs cite it as iteration 207.

> **Premise check (2026-09-06):** the "today the suite runs `--test-threads=1`, so the window is empty" statement below is no longer true. `crates/xtask/src/live_sweep.rs` runs the CLI tier with `--test-threads={jobs}` since iteration 188, so the 1 KiB process-global window is open in every sweep. This is why the plan sits at the front of the run: it is a live confound for 249–251.

> **Merged 2026-09-06 (DEC-051 addendum):** absorbs [[iteration-236-live-166-cache-304]] as Part B. One branch, one PR, one carry-over sweep for both parts.

## Part A: live_bulk_cap still shrinks the process-global frame cap for every other live test

### Where this came from

Carry-over from [[iteration-196-frame-cap-lock-has-no-readers]]. That iteration removed every
shrink of the process-global frame cap from `ff-rdp-core`'s unit-test binary by giving each parser
a `*_with_cap` form and making `RaisedFrameCap::raise_to` panic on any value below
`DEFAULT_MAX_FRAME_BYTES`. `crates/ff-rdp-cli/tests/live/live_bulk_cap.rs` was left alone
deliberately: it is a different binary, so it cannot make `cargo test --workspace` red, and
iteration 196's plan says to file further process-global state rather than absorb it.

### The defect

```rust
let _cap_guard = FrameCapGuard(max_frame_bytes());
set_max_frame_bytes(1024);
```

`live_bulk_cap.rs` shrinks a process-global to 1 KiB for the duration of its own round-trip. Every
other test in the live binary that parses a frame larger than 1 KiB during that window fails with
`FrameTooLarge` — and live tests parse screenshot data URLs and longString bodies, which are
routinely far larger.

The RAII guard added in iter-114 (DEC-022) restores the cap afterwards, which fixes the *leak*.
It does nothing about the *window*. Today the live suite runs `--test-threads=1`, so the window is
empty in practice — but that is an accident of the runner's flags, not a property of the test, and
[[iteration-251-live-tests-red-only-under-concurrency]] is explicitly about raising live-suite
parallelism.

### What this iteration must decide

`RdpTransport::recv` reads the global cap, so unlike the core tests this one cannot simply pass a
cap to a free function. Two shapes:

- **Per-instance cap on the transport.** `RdpTransport`/`FramedReader` snapshot the cap at
  construction (or take an explicit override), so a test can cap one connection without touching
  the process. Note the review rule: a new `pub` item needs a non-test consumer in the same PR, so
  this only lands honestly if the CLI itself has a reason to set a per-connection cap.
- **Keep the global, prove the property differently.** The AC being defended is "an oversized bulk
  announcement is rejected before any body read, promptly". A cap of 1 KiB is convenient but not
  essential — announcing more than the *default* 256 MiB proves the same thing with no mutation at
  all, at the cost of a larger number in the header (no allocation either way, since the cap check
  precedes the read).

The second is a two-line change and removes the last writer outright. Prefer it unless something
about the 1 KiB cap turns out to be load-bearing.

### Tasks

#### A. The fix [2/2]
- [x] Remove the `set_max_frame_bytes` call from `live_bulk_cap.rs`, keeping the AC it defends
- [x] Confirm no other file under `crates/*/tests/` mutates the cap

#### B. Proof [0/1]
- [ ] `FF_RDP_LIVE_TESTS=1 cargo test-live -p ff-rdp-cli --test live` green, and the oversize
      rejection still measured under 500 ms

### Acceptance Criteria [3/3]

- [x] `grep -rn "set_max_frame_bytes" crates/*/tests/` returns nothing that lowers the cap
- [x] `live_bulk_frame_oversize_rejected` still asserts announced-length round-trip, the `max`
      field, and the sub-500 ms rejection
- [x] The live suite would survive `--test-threads>1` with respect to the frame cap — stated with
      the reason, not just asserted

### Out of scope

- **Raising live-suite parallelism.** That is [[iteration-251-live-tests-red-only-under-concurrency]];
  this iteration only removes one reason it would be unsafe.
- **Other process-global state in the live binary.** File it, do not absorb it.

### References

- `crates/ff-rdp-cli/tests/live/live_bulk_cap.rs:56` — the surviving writer
- `kb/decision-log.md` — DEC-048 (iter-196, supersedes DEC-029), DEC-022 (iter-114 leak)

## Part B: live_166 asserts HTTP 200 on a response Firefox caches as 304 (absorbed from iteration 236)

> **Renumbered 214 → 236 on 2026-09-06** so the pending queue runs as one contiguous sweep (DEC-051). Older PRs, commits and sweep logs cite it as iteration 214.

Found by [[iteration-210-act-and-see]]'s closing live sweep
(`FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1`, 279 passed / 4 failed). Two of the four:

```
live_166_navigate_document_status::live_166_navigate_reports_document_status FAILED
  daemon, trailing slash: expected HTTP 200, got
  {"navigated":"https://example.com/","status":304,...,"ready_state":"complete"}

live_166_navigate_document_status::live_166_navigate_status_direct_parity FAILED
  --no-daemon --with-network: expected HTTP 200, got status 304
```

Both reproduce on an isolated `--test-threads=1` re-run, so this is not the sweep's parallelism.

**ff-rdp is right and the test is wrong.** `live_166` asserts that navigating to a reachable page
reports HTTP 200. Firefox caches `https://example.com/`, so a repeat visit is a conditional
request the server answers `304 Not Modified` — and `results.status` reporting 304 is exactly the
document-status truthfulness iter-166 was built to deliver. The test encodes "the first,
uncached fetch" as if it were "any fetch".

Not caused by iter-210: its `navigate.rs` diff is `--with-page` plumbing only and touches nothing
in the document-status path. Filed as its own plan rather than dismissed as environmental, because
a live test that fails on a warm cache is a defect of ours either way — it will keep costing the
next iteration's sweep two red lines and an investigation.

Re-confirmed by [[iteration-220-with-page-after-navigating-click]]'s closing sweep on 2026-08-30
(`executed=313 … 310 passed / 3 failed`): the same two `live_166_*` assertions, same 304. The
sweep filed it again as iteration-221 before spotting this plan; 221 is now `obsolete` and its
tasks live here.

### Themes

- **A — Make the assertion true of what the test actually exercises.** Either stop the fetch being
  conditional, or widen the assertion to the set of statuses a successful navigation can carry —
  and say which, rather than leaving a future reader to guess whether 304 was intended.

### Tasks

#### A. Fix the assertion [3/3]
- [x] Decide between the two honest fixes and record the reason in the test's own comment:
      (a) defeat the cache for this navigation (a cache-busting query parameter, or a
      `Cache-Control: no-cache` load), keeping the strict `200`; or (b) accept any
      non-error document status and assert on `status_reason` being null.
      (a) keeps the test's original intent — "the server answered 200" — and is preferred unless
      it turns out ff-rdp has no way to force a non-conditional load, which is itself worth knowing
- [x] Apply it at `crates/ff-rdp-cli/tests/live/live_166_navigate_document_status.rs:121` and
      every sibling assertion with the same premise
- [x] Check the rest of the live suite for the same assumption — any other test asserting a
      literal `200` from a repeatedly-visited public URL has this defect latent


#### B. Reduce the network surface [1/1]
- [x] Move the trailing-slash leg to a local fixture route if it can be done without weakening
      what it asserts; if it cannot, say why in the Outcome

#### C. Same shape elsewhere [1/1]
- [x] Grep the live suites for a second fetch of the same public URL in one profile; fix or file

### Acceptance Criteria [2/4]

- [ ] Running `live_166_navigate_document_status` twice in a row against the **same** profile
      passes both times (the warm-cache case is the one that was never exercised)
- [ ] `live_166_navigate_reports_document_status` and `live_166_navigate_status_direct_parity`
      pass on a **warm** profile — run them twice in a row against the same Firefox, not once
      against a fresh one
- [x] The test states, in a comment, why 304 is or is not acceptable — so the next reader does not
      re-litigate it
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean. (covers both parts)

### Design notes

- **Do not simply add 304 to a list of accepted statuses without saying why.** That is how an
  assertion stops meaning anything: the test would then pass if ff-rdp reported 304 for a
  navigation that genuinely got 200, which is the bug iter-166 existed to catch.
- **`example.com` is the variable here, not ff-rdp.** If the fix needs a URL whose caching
  behaviour is under our control, the fixture HTTP server the other live suites use
  (`FixtureServer`) is that URL — at the cost of no longer exercising a real remote origin, which
  is what this test wanted. Weigh it; do not swap silently.

### Out of scope

- The other two failures from the same sweep
  (`live_137_consent_accept_via_daemon`, `live_navigate_elapsed_matches_wall`). Both passed on the
  isolated re-run and are recorded as load-sensitive in
  [[iteration-210-act-and-see]]'s carry-over table, with the trigger for filing them stated there.

### References

- [[iteration-210-act-and-see]] — the sweep that found this; carry-over rows 2 and 3
- `crates/ff-rdp-cli/tests/live/live_166_navigate_document_status.rs:121`
- `crates/ff-rdp-cli/src/commands/navigate.rs` — `DocumentStatusTracker`, the code being asserted on

## Outcome (2026-09-07, branch `iter-235/live-suite-defects`)

Both parts landed as the plan's preferred option. Recorded in `kb/decision-log.md` as **DEC-053**,
which also annotates DEC-048's "Not fixed here" clause as resolved.

### Part A — the last cap writer is gone

`crates/ff-rdp-cli/tests/live/live_bulk_cap.rs` no longer calls `set_max_frame_bytes`, and its
`FrameCapGuard` RAII type is deleted with it. The test now reads `max_frame_bytes()` and announces
twice that: `announced = 512 MiB` against `max = 256 MiB`, where it used to announce `2 KiB`
against a cap it had shrunk to `1 KiB`. The plan's second shape was taken, unchanged — the 1 KiB
cap turned out not to be load-bearing, because the cap check precedes the body read, so neither
number allocates. The first shape (a per-instance cap on `RdpTransport`) was rejected for the
reason the plan anticipated: it would add a `pub` API whose only consumer is a test.

- `grep -rn "set_max_frame_bytes" crates/*/tests/` → no hits at all. The only writer left in the
  workspace is `crates/ff-rdp-cli/src/main.rs:294`, where `--max-frame-mb` belongs.
- `FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_bulk_cap -- --include-ignored`
  → `1 passed`, and the assertions it passed are the same three: announced-length round-trip,
  `max` equal to the cap, and rejection under 500 ms.
- Why `--test-threads>1` is now safe with respect to the frame cap, structurally rather than by
  convention: there is no writer left to synchronise with. The sweep below ran the CLI tier at
  `--test-threads=6` and produced no `FrameTooLarge` anywhere.

### Part B — `live_166` gets a fresh URL, not a wider assertion

The four `https://example.com` legs across `live_166_navigate_reports_document_status` and
`live_166_navigate_status_direct_parity` now request a unique
`?ff-rdp-cache-bust=<nanos>-<serial>` URL from a new `uncached_example_url()` helper, which also
returns the canonicalised form so `committed_url` is compared against a computed value instead of
a hardcoded `"https://example.com/"`. The strict `200` is unchanged, and the helper's doc comment
states why 304 is *prevented* rather than accepted — widening to `200 | 304` would also pass a
navigation that genuinely got 200 but was reported 304, the exact defect class iteration 166
exists to catch.

The cache buster keeps the canonicalisation coverage: Firefox still rewrites
`https://example.com?x` → `https://example.com/?x`, the missing-slash shape that *was* the
iteration 166 defect.

**Task B (move the trailing-slash leg to a fixture) — deliberately not done, reason as required
by the task.** `FixtureServer` sends `Cache-Control: no-store` and no validators, so it can never
answer 304 — which is exactly why `live_138_navigate_reports_200`, `live_169`'s reload leg and
this file's own `live_166_navigate_status_reflects_the_server` were immune to this defect. But it
also cannot reproduce the `host` vs `host/` distinction, because `{base}/ok` is spelled
identically either way. Moving the leg would trade away the only real-origin coverage in the file
for a duplicate of a fixture test that already exists.

**Task C (same shape elsewhere) — swept, nothing else to fix or file.** Every other literal-`200`
status assertion in `crates/ff-rdp-cli/tests/live/` is against `FixtureServer`
(`live_138_navigate_reports_200`, `live_169_nav_verb_status_parity`'s reload leg,
`live_166_navigate_status_reflects_the_server`). Sixteen other live files do fetch
`https://example.com` more than once in a profile, but none asserts a status on it —
`live_159_daemon_watcher_regression` only checks `status` is non-null.

### Honest limits — two ACs left unticked

- **Part A AC "the live suite is green".** It is not: the closing sweep is 309 passed / 11 failed.
  None of the eleven is this iteration's, and `live_bulk_cap` itself passes; seven are a Firefox
  155 `drawSnapshot` break filed as [[iteration-257-firefox-155-drawsnapshot-dictionary-arg]] and
  four are pre-existing or load-sensitive. Ticking "green" would be false, so it stays empty.
- **Part B ACs "run twice in a row against the same profile / the same Firefox".** The premise is
  wrong: `live_166`'s tests call `LiveFirefox::headless_on_random_port()`, which always creates a
  *fresh* temp profile, so the harness has no way to point them at a warm one. What was measured
  instead, on 2026-09-07: the two tests run back to back twice, `4 passed` both times; and a
  hand dogfood on a **single** launched instance (one profile, warm after the first hit) —
  `navigate https://example.com` three times → `200, 200, 200`, then three distinct cache-busted
  URLs → `200, 200, 200`. Note what that first row says: **today the plain repeat did not
  reproduce the 304 at all.** The 304 depends on `example.com`'s current cache headers and
  Firefox's revalidation heuristics, neither of which is ours — which is the argument for removing
  the dependency rather than asserting around it, and why the fix is a cache buster and not a
  wider assertion.

### Closing live sweep

`FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep`, with a raw
`firefox -no-remote -profile <tmp> --start-debugger-server 6000 --headless` on port 6000:

```
LIVE_SWEEP_SUMMARY executed=320 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=320
```

309 passed / 11 failed; `309 + 11 == 320 == executed`, so the record reconciles. Both `live_166`
legs are green in it. The eleven failures and their dispositions are in the PR body's
`## Carry-over` table.
