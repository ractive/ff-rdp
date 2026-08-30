---
title: "Iteration 214: live_166 asserts HTTP 200 on a response Firefox caches as 304"
type: iteration
date: 2026-08-29
status: planned
branch: iter-214/live-166-cache-304
depends_on: []
dogfood_path: |
  ff-rdp launch --headless
  ff-rdp navigate https://example.com --jq '.results.status'
  ff-rdp navigate https://example.com --jq '.results.status'
  # second call on a warm cache: expected 304, not 200 — the behaviour live_166 mis-asserts
tags: [iteration, live-tests, test-reliability]
---

# Iteration 214: live_166 asserts HTTP 200 on a response Firefox caches as 304

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

## Themes

- **A — Make the assertion true of what the test actually exercises.** Either stop the fetch being
  conditional, or widen the assertion to the set of statuses a successful navigation can carry —
  and say which, rather than leaving a future reader to guess whether 304 was intended.

## Tasks

### A. Fix the assertion [0/3]
- [ ] Decide between the two honest fixes and record the reason in the test's own comment:
      (a) defeat the cache for this navigation (a cache-busting query parameter, or a
      `Cache-Control: no-cache` load), keeping the strict `200`; or (b) accept any
      non-error document status and assert on `status_reason` being null.
      (a) keeps the test's original intent — "the server answered 200" — and is preferred unless
      it turns out ff-rdp has no way to force a non-conditional load, which is itself worth knowing
- [ ] Apply it at `crates/ff-rdp-cli/tests/live/live_166_navigate_document_status.rs:121` and
      every sibling assertion with the same premise
- [ ] Check the rest of the live suite for the same assumption — any other test asserting a
      literal `200` from a repeatedly-visited public URL has this defect latent


### B. Reduce the network surface [0/1]
- [ ] Move the trailing-slash leg to a local fixture route if it can be done without weakening
      what it asserts; if it cannot, say why in the Outcome

### C. Same shape elsewhere [0/1]
- [ ] Grep the live suites for a second fetch of the same public URL in one profile; fix or file

## Acceptance Criteria [0/4]

- [ ] Running `live_166_navigate_document_status` twice in a row against the **same** profile
      passes both times (the warm-cache case is the one that was never exercised)
- [ ] `live_166_navigate_reports_document_status` and `live_166_navigate_status_direct_parity`
      pass on a **warm** profile — run them twice in a row against the same Firefox, not once
      against a fresh one
- [ ] The test states, in a comment, why 304 is or is not acceptable — so the next reader does not
      re-litigate it
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

- **Do not simply add 304 to a list of accepted statuses without saying why.** That is how an
  assertion stops meaning anything: the test would then pass if ff-rdp reported 304 for a
  navigation that genuinely got 200, which is the bug iter-166 existed to catch.
- **`example.com` is the variable here, not ff-rdp.** If the fix needs a URL whose caching
  behaviour is under our control, the fixture HTTP server the other live suites use
  (`FixtureServer`) is that URL — at the cost of no longer exercising a real remote origin, which
  is what this test wanted. Weigh it; do not swap silently.

## Out of scope

- The other two failures from the same sweep
  (`live_137_consent_accept_via_daemon`, `live_navigate_elapsed_matches_wall`). Both passed on the
  isolated re-run and are recorded as load-sensitive in
  [[iteration-210-act-and-see]]'s carry-over table, with the trigger for filing them stated there.

## References

- [[iteration-210-act-and-see]] — the sweep that found this; carry-over rows 2 and 3
- `crates/ff-rdp-cli/tests/live/live_166_navigate_document_status.rs:121`
- `crates/ff-rdp-cli/src/commands/navigate.rs` — `DocumentStatusTracker`, the code being asserted on
