---
title: "Iteration 104: security & PWA audit pack — per-request TLS info, Web App Manifest validation, optional throttling/blocking"
type: iteration
date: 2026-07-09
status: planned
branch: iter-104/security-pwa-audit-pack
depends_on: []
firefox_refs:
  - lines: 340-360
    path: devtools/server/actors/network-monitor/network-event-actor.js
    why: >-
      getSecurityInfo implementation — returns the cached _securityInfo for
      the request (populated when the response is observed).
  - lines: 690-710
    path: devtools/server/actors/network-monitor/network-event-actor.js
    why: >-
      where _securityInfo is populated from the observed response — explains
      why the watcher must have seen the request for security info to exist.
  - lines: 14-17
    path: devtools/shared/specs/manifest.js
    why: >-
      fetchCanonicalManifest spec — one call returning the parsed Web App
      Manifest plus conformance errors.
  - lines: 23-67
    path: devtools/shared/specs/network-parent.js
    why: >-
      setNetworkThrottling / setBlockedUrls / blockRequest specs — the
      optional theme C surface; note which methods declare no response block
      but are not oneway.
kb_refs:
  - kb/research/deep-review-2026-07-fable5.md
  - kb/rdp/actors/network-event.md
first_call_sites:
  - primitive: NetworkEventFront::get_security_info + `network --security` output fields
    site: crates/ff-rdp-cli/src/commands/network.rs
  - primitive: manifest command driving ManifestFront::fetch_canonical_manifest
    site: crates/ff-rdp-cli/src/commands/manifest.rs
  - primitive: >-
      (stretch) throttle command driving
      NetworkParentFront::set_network_throttling / set_blocked_urls
    site: crates/ff-rdp-cli/src/commands/throttle.rs
dogfood_path: |
  ff-rdp launch --headless
  ff-rdp navigate https://example.com --with-network
  ff-rdp network --security --jq '.requests[0].security'
  # expected: {protocolVersion: "TLSv1.3", cipherSuite: "...", cert: {...}, hsts: ...}
  ff-rdp manifest
  # expected: parsed manifest JSON + conformance errors, or a clear "no manifest" result
tags: [iteration, security, tls, manifest, pwa, network, audit, review-2026-07]
---

# Iteration 104: security & PWA audit pack

## Live-test policy (2026-07-09, per James)

Do NOT run the full live Firefox suite (`cargo test-live`, or `--test live --
--include-ignored` without a filter) during this iteration — neither while
implementing nor while reviewing. Run ONLY (1) the specific live tests this
plan's ACs name, filtered (e.g. `cargo test -p ff-rdp-cli --test live
<filter> -- --include-ignored`), and (2) this iteration's dogfood script
(required by check-iteration-ready). Full-suite validation is deferred to
[[iteration-107-post-105-live-sweep]], which runs once after iteration 105
merges and fixes all fallout there.

The RDP surface audit ([[deep-review-2026-07-fable5]], gaps C2/C3/C5) found
two small-effort actors that directly serve the site-audit workflows ff-rdp
already targets (`/site-audit`'s Security category, dogfooding session 51's
security run, the debug skill's manifest playbook — which currently does a
raw in-page fetch instead of asking Firefox):

1. **`network-event.getSecurityInfo`** — per-request TLS/certificate detail:
   `protocolVersion` (flag TLS 1.0/1.1), `cipherSuite`, certificate
   subject/issuer/validity/fingerprint, HSTS, and `weaknessReasons`. ff-rdp
   already captures the network-event actor ids it needs (same ids used for
   `getResponseHeaders`) — this is one more method on an actor we hold.
2. **`manifest.fetchCanonicalManifest`** — the parsed Web App Manifest
   **plus conformance errors** in one call: a PWA-readiness audit primitive.

As a stretch (theme C, explicitly droppable): **network throttling and URL
blocking** via the network-parent actor — Lighthouse-style Slow-3G perf runs
and resilience tests ("does the page survive its analytics CDN being
blocked?").

## Themes

- **A — `network --security`.** TLS/cert per request, joined into the
  existing network output.
- **B — `manifest` command.** PWA manifest + conformance errors.
- **C — (stretch) `throttle`.** Named throttling profiles + URL blocking.

## Tasks

### A. network --security [0/3]
- [ ] Add `get_security_info` to the network-event actor surface in core
      (spec-checked; annotate any drift per the allow-spec-drift rule).
- [ ] Add `--security` to `ff-rdp network`: for each captured HTTPS request,
      attach a `security` object (protocolVersion, cipherSuite, cert summary,
      hsts, weaknessReasons); plain-HTTP requests get `security: null`
      **plus** a top-level `insecure_requests` count so audits can flag
      mixed content at a glance.
- [ ] Document the population constraint in `kb/rdp/actors/network-event.md`:
      security info exists only for requests the watcher observed
      (daemon-buffered or `--with-network` windows), matching
      `network-event-actor.js:690-710`.

### B. manifest command [0/3]
- [ ] Add a manifest front in core (`fetchCanonicalManifest` on the tab
      target's manifest actor) + `kb/rdp/actors/` note (actor-kb-sync).
- [ ] Add `ff-rdp manifest`: JSON envelope with the parsed manifest and its
      conformance `errors` array; "page has no manifest" is a structured
      non-error result (`manifest: null, reason: "..."`), not an exit
      failure.
- [ ] Retire the raw-fetch manifest playbook step in the debug skill docs in
      favor of the command (doc pointer update only).

### C. (stretch — drop before slipping) throttle [0/2]
- [ ] Add `set_network_throttling`/`set_blocked_urls` to a network-parent
      front (obtained via `watcher.getNetworkParentActor()`); note the
      protocol quirk: these methods declare **no response block but are NOT
      oneway** — use the same matched-request handling as
      `walker.releaseNode`.
- [ ] Add `ff-rdp throttle slow-3g|fast-3g|off` and
      `ff-rdp throttle --block <pattern>...`; envelope echoes active
      profile/blocklist.

## Acceptance Criteria [0/6]

- [ ] live_network_security_info_https: for a captured https request,
      `security.protocolVersion` starts with "TLS" and `cipherSuite` is
      non-empty; for an http request `security` is null and
      `insecure_requests` ≥ 1.
- [ ] live_manifest_fetch_canonical: a fixture page with a linked manifest
      returns its parsed `name`/`start_url` and an `errors` array; a page
      without a manifest returns `manifest: null` with exit code 0.
- [ ] e2e_manifest_no_tab_error_shape: `manifest` against a closed/unknown
      tab produces the standard error envelope (error_type + non-zero exit).
- [ ] live_throttle_slow3g_slows_fetch (stretch — annotate
      `[deferred — new plan: …]` if theme C is dropped): a timed in-page
      fetch under slow-3g takes measurably longer than baseline (≥2×).
- [ ] live_block_url_pattern (stretch, same annotation rule): a request
      matching the blocked pattern is reported failed/blocked in `network`
      output while other requests succeed.
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Design notes

- Security info rides on actor ids the network pipeline already tracks — the
  join happens at output-assembly time in the CLI, not in the buffer, so the
  daemon buffer schema is untouched.
- `manifest` uses the *tab target's* manifest actor (same acquisition
  pattern as other tab-scoped fronts). Live-network test pages should come
  from the standard fixture-recording flow, not hand-crafted JSON.
- Theme C is genuinely optional: it has its own actor, its own quirks, and
  its own tests. If it threatens the iteration, cut it at the theme boundary
  and file the follow-up plan (carry-over rule) — do not half-land it.

## Out of scope

- `network-content.sendHTTPRequest` (request replay/crafting — gap C-#5 in
  the review) — separate iteration; active-tester semantics deserve their
  own security review.
- Service-worker enumeration / worker targets for deeper PWA audits.
- site-audit skill integration (consume the new commands there once landed).

## References

- [[deep-review-2026-07-fable5]] — gaps C2 (getSecurityInfo), C3 (manifest),
  C5 (throttling).
- [[iteration-103-target-configuration-cli]] — sibling review-driven feature
  pack; cache-disabled there complements throttling here for perf audits.
