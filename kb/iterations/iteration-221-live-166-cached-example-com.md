---
title: "Iteration 221: live_166 asserts HTTP 200 on a URL it has already fetched, and Firefox serves it 304"
type: iteration
date: 2026-08-30
status: obsolete
branch: iter-221/live-166-cached-example-com
depends_on: []
first_call_sites:
  - primitive: (none — test-only change; no new pub item)
    site: crates/ff-rdp-cli/tests/live/live_166_navigate_document_status.rs
dogfood_path: |
  firefox -no-remote --start-debugger-server 6000 --headless   # raw browser, NOT ff-rdp launch
  ff-rdp navigate https://example.com  --jq '.results.status'
  # expected: 200
  ff-rdp navigate https://example.com/ --jq '.results.status'
  # TODAY: 304 — same document, same profile, revalidated from the necko cache.
  # live_166 asserts 200 on this second call and fails.
  # expected AFTER this iteration: the suite no longer depends on which of the two
  # a warm cache returns
tags: [iteration, live-tests, carry-over, flake]
---

> **Obsolete (2026-08-31):** duplicate of [[iteration-214-live-166-cache-304]], filed by the
> iter-220 sweep before checking for an existing plan. Its Tasks B/C and warm-cache AC were
> folded into 214. Nothing to do here.

# Iteration 221: `live_166` asserts HTTP 200 on a URL it has already fetched

## Why

Carry-over from [[iteration-220-with-page-after-navigating-click]]'s closing sweep. Two reds,
both in the same suite, both the same cause:

```
FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 → LIVE_SWEEP_SUMMARY
  executed=313 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=313
  302 passed / 2 failed (CLI tier) + 1 + 3 + 3 + 2 passed (core tiers)

live_166_navigate_document_status::live_166_navigate_reports_document_status
  daemon, trailing slash: expected HTTP 200, got
  {"navigated":"https://example.com/","status":304,…,"ready_state":"complete"}
live_166_navigate_document_status::live_166_navigate_status_direct_parity
  — same assertion, direct route
```

`live_166_navigate_reports_document_status` navigates to `https://example.com` (line 175) and
then to `https://example.com/` (line 184) **in the same Firefox profile**. Those canonicalise to
the same URL, so the second request is a conditional revalidation and `example.com` answers
`304 Not Modified`. `assert_status(…, 200, …)` then fails.

This is a defect in the test, not in `navigate`: reporting `304` for a revalidated document is
the *correct* behaviour, and reporting it is exactly what iteration 166 built. The test simply
encoded "the status of this URL is 200" as if it were a property of the URL rather than of the
request.

## Not a regression from iter-220

`navigate`'s only iter-220 change is `refresh_console_actor` consuming the transport's navigation
latch — it touches no HTTP path and no status extraction. The failure is reproducible from the
CLI by hand (see `dogfood_path`) with no `--with-page` anywhere.

Unverified: whether this has been red for a while or only started when `example.com` began
sending validators. Nobody has been quoting a network-gated sweep — iteration 160 ran with
`FF_RDP_LIVE_TESTS` only, `skipped=32`, and these two were among the unrun.

## Themes

- **A — Make the assertion about the request, not the URL.** The suite is testing that
  `results.status` tracks what the server actually said. Either accept `200` *or* `304` for the
  warm-cache leg and assert the first leg is `200`, or give each leg its own cache-busting query
  string so both are genuinely first fetches. Prefer whichever keeps the trailing-slash
  canonicalisation assertion (the reason line 184 exists) intact.
- **B — Ask whether the second leg needs the public internet at all.** The canonicalisation
  behaviour under test is `https://example.com` → `https://example.com/`; a local
  `FixtureServer` route proves the same thing without a network gate and without a cache the
  test does not control. If the leg can move, the suite gets shorter *and* deterministic.
- **C — Look for the same shape elsewhere.** Any live test that fetches one public URL twice in
  one profile has this bug latent. Grep the live suites for repeated `example.com` /
  `example.org` navigations before closing.

## Tasks

### A. Fix the two reds [0/2]
- [ ] Decide between "accept 200 or 304" and "cache-bust each leg", with the reason recorded
- [ ] Both `live_166_*` tests green in a `FF_RDP_LIVE_NETWORK_TESTS=1` sweep

### B. Reduce the network surface [0/1]
- [ ] Move the trailing-slash leg to a local fixture route if it can be done without weakening
      what it asserts; if it cannot, say why in the Outcome

### C. Same shape elsewhere [0/1]
- [ ] Grep the live suites for a second fetch of the same public URL in one profile; fix or file

## Acceptance Criteria [0/3]

- [ ] `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep` reports
      0 failures, with the real `LIVE_SWEEP_SUMMARY` line pasted into the PR body
- [ ] Running `live_166_navigate_document_status` twice in a row against the **same** profile
      passes both times (the warm-cache case is the one that was never exercised)
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q`
      clean

## Out of scope

- `navigate`'s status extraction itself. `304` is the right answer; only the assertion is wrong.

## References

- [[iteration-220-with-page-after-navigating-click]] — the sweep that surfaced this
- `crates/ff-rdp-cli/tests/live/live_166_navigate_document_status.rs` — lines 175 and 184
