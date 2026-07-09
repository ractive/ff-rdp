---
title: "Iteration 109: network throttling & URL blocking — carry-over of iter-104 Theme C"
type: iteration
date: 2026-07-09
status: planned
branch: iter-109/network-throttle-block
depends_on: []
firefox_refs:
  - lines: 23-67
    path: devtools/shared/specs/network-parent.js
    why: >-
      setNetworkThrottling / setBlockedUrls / blockRequest specs — the theme C
      surface; note which methods declare no response block but are NOT oneway.
kb_refs:
  - kb/rdp/actors/network-parent.md
  - kb/iterations/iteration-104-security-pwa-audit-pack.md
first_call_sites:
  - primitive: >-
      throttle command driving
      NetworkParentFront::set_network_throttling / set_blocked_urls
    site: crates/ff-rdp-cli/src/commands/throttle.rs
dogfood_path: |
  ff-rdp launch --headless
  ff-rdp throttle slow-3g
  ff-rdp navigate https://example.com --with-network
  ff-rdp throttle off
tags: [iteration, network, throttle, blocking, perf, audit, carry-over, review-2026-07]
---

# Iteration 109: network throttling & URL blocking (iter-104 Theme C carry-over)

## Execution policies (2026-07-09, per James)

**Live tests:** do NOT run the full live Firefox suite during this iteration.
Run only the specific live tests this iteration's themes/ACs actually touch
(filtered, e.g. `cargo test -p ff-rdp-cli --test live <filter> --
--include-ignored`) plus the dogfood script. Full-suite validation happens
exactly once, in [[iteration-110-post-batch-live-sweep]], after iteration 109.

**Scoped testing — don't run everything N times:** while developing, run only
the tests affected by the change (`cargo test -p <crate> <filter>`). Run the
full `cargo test --workspace -q` exactly ONCE, as part of the final pre-PR
quality gates. The review agent must NOT re-run the full workspace suite
(implement's gate run + CI cover it); after review fixes, re-run only the
tests covering the files those fixes touched, then rely on CI.

**CI-wait:** merge once the required lanes pass (fmt, clippy, discipline,
supply-chain, fuzz, ubuntu/macos tests, verify-attestation). Do not block on
`live-tests` (advisory by design).
[[iteration-108-windows-ci-preexisting-reds]] landed and fixed the windows-latest
reds (`install_skill::*` + `nav_action::reload_wait_idle_*`) — `test
(windows-latest)` should be green going forward. If it shows failures, that
IS a regression: stop and fix (do not assume "known-red" carries over).
Also note: `test (ubuntu-latest)` has a separate, still-unfixed pre-existing
flake, `transport::tests::redact_sensitive_key_replaces_value` (found during
iter-108's PR #147 review, unrelated to this iteration's diff surface) — a
single failure limited to that one test on ubuntu is a known flake, not a
regression from this iteration's work; re-run the job once before treating it
as a real red.


Theme C of [[iteration-104-security-pwa-audit-pack]] (network throttling + URL
blocking via the network-parent actor) was **deferred at the theme boundary**
per iter-104's explicit "cut it before it slips" rule. It was dropped because
its wire path could not be live-verified in the implementation environment (no
reachable Firefox for a `getNetworkParentActor` trace), and the plan's own
carry-over discipline requires the follow-up to be filed before the iter-104 PR
merges. This is that follow-up.

## Why deferred, not half-landed

The plan warned (Theme C task 1) that `watcher.getNetworkParentActor()`
currently deserializes its reply as `response::ActorRef` (top-level `actor`
field) — the same flat shape that proved **wrong** for
`getTargetConfigurationActor` (real Firefox nests the actor under a named
typed-actor object; see `specs/watcher.rs` `ConfigurationActorRef` and the
iter-103 doc comment on `ActorRef`). This iteration is the first live consumer
of `getNetworkParentActor`, so its wire shape MUST be verified against a live
Firefox trace before trusting `ActorRef`; if nested, add a
`NetworkParentActorRef` following the `ConfigurationActorRef` pattern rather
than assuming the flat shape.

## Tasks [0/2]

- [ ] Add `set_network_throttling`/`set_blocked_urls` to a network-parent front
      (obtained via `watcher.getNetworkParentActor()`); note the protocol
      quirk: these methods declare **no response block but are NOT oneway** —
      use the same matched-request handling as `walker.releaseNode`. Verify the
      `getNetworkParentActor` reply shape against a live trace first.
- [ ] Add `ff-rdp throttle slow-3g|fast-3g|off` and
      `ff-rdp throttle --block <pattern>...`; envelope echoes active
      profile/blocklist.

## Acceptance Criteria [0/2]

- [ ] live_throttle_slow3g_slows_fetch: a timed in-page fetch under slow-3g
      takes measurably longer than baseline (≥2×).
- [ ] live_block_url_pattern: a request matching the blocked pattern is
      reported failed/blocked in `network` output while other requests succeed.

## References

- [[iteration-104-security-pwa-audit-pack]] — parent; Themes A & B landed there.
- [[deep-review-2026-07-fable5]] — gap C5 (throttling).
- [[iteration-103-target-configuration-cli]] — source of the
  `getNetworkParentActor` nested-actor-shape warning; cache-disabled there
  complements throttling for perf audits.
