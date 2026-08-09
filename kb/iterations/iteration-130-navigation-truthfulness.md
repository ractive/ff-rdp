---
branch: iter-130/navigation-truthfulness
date: 2026-07-19
depends_on:
  - kb/iterations/iteration-122-navigate-dom-complete-ff152.md
dogfood_path: |
  ff-rdp launch --headless --auto-consent
  ff-rdp navigate https://www.comparis.ch/hypotheken --jq '.results.committed_url'
  # → a real comparis URL, never "about:blank"
  ff-rdp navigate https://example.com >/dev/null && ff-rdp back --jq '.results'
  # → navigate-style envelope: {action, committed_url, ready_state, elapsed_ms}
first_call_sites: []
status: planned
---

# Iteration 130: navigation truthfulness — SPA committed_url, back/forward envelope, reload coupling

Bundles the remaining navigation-report gaps from [[dogfooding-session-61]] and
[[dogfooding-session-62]] into one navigation-focused iteration.

## Findings driving this iteration

1. **`committed_url` is `about:blank` on SPA route commits** (s61 #5, MODERATE — the
   last remaining non-cosmetic s61 regression). Iter-122/124 fixed static pages
   (example.com/wikipedia/github/httpbin all correct now), but comparis SPA routes still
   report `committed_url:"about:blank"` while `navigated`, `ready_state:complete`, and
   `eval location.href` all confirm the real URL landed. A caller trusting
   `committed_url` concludes the navigation failed.
2. **`back`/`forward` return only `{"action":"back"}`** (dogfood-62 #7, LOW): no
   `committed_url`/`ready_state`/`elapsed_ms`; the caller needs a follow-up eval to know
   where it landed.
3. **perf right after `reload` races** (dogfood-62 #8, LOW): `perf summary` immediately
   after `reload` reports `total_resources:0` (buffer cleared, new load not yet
   populated), silently indistinguishable from "page has no resources"; moments later
   the same command reports 50.

## Themes

- **A — `committed_url` reflects the real committed document on SPA flows.** Root-cause
  where the DOCUMENT_EVENT/location tracking loses the URL when the SPA router takes
  over after the server document commits (iter-122's fast-path + iter-124's probe
  re-resolution are prior art in `navigate`); fall back to reading `location.href` at
  completion if the protocol path genuinely reports nothing. `committed_url` must never
  be `about:blank` when `ready_state` is `complete` on an http(s) URL.
- **B — `back`/`forward`/`reload` return the navigate envelope.** Same fields
  (`committed_url`, `ready_state`, `elapsed_ms`) so all four navigation verbs are
  interchangeable for a caller; `reload` couples with load completion the same way
  `navigate` does.
- **C — perf declares "no data yet" instead of silent zeros.** When the resource
  buffer is empty immediately after a navigation/reload, `perf summary` (and audit)
  carry an explicit marker (e.g. `resources_pending: true` or a note) rather than a
  bare `total_resources:0`.

## Tasks

- [ ] A: trace the comparis flow (server doc commit → SPA router) and fix the
      committed-URL source; add the location.href completion fallback.
- [ ] B: extract the navigate envelope builder and reuse it in `back`, `forward`,
      `reload`; wire reload's completion wait.
- [ ] C: empty-buffer detection + explicit marker in perf summary/audit envelopes;
      help text documents the marker.
- [ ] Update help/cookbook for the four navigation verbs' shared envelope.

## Acceptance Criteria [0/4]

<!-- Each AC names a live test + asserted post-condition, per CLAUDE.md convention. -->

- [ ] live_130_spa_committed_url (network-gated): navigate
      https://www.comparis.ch/hypotheken → `committed_url` starts with
      `https://www.comparis.ch` and is not `about:blank`.
- [ ] live_130_back_forward_envelope: on local fixture pages, `back` and `forward`
      each return `committed_url` matching the landing page, a `ready_state`, and
      `elapsed_ms` > 0.
- [ ] live_130_reload_envelope: `reload` returns the navigate-style envelope with
      `ready_state:"complete"` on a static fixture.
- [ ] live_130_perf_no_silent_zero: `reload` followed immediately by `perf summary`
      either reports `total_resources` > 0 or carries the explicit pending/no-data
      marker — never a bare unmarked 0.

## Notes

Sibling plans from the same findings batch: [[iteration-128-network-hint-always-present]],
[[iteration-129-consent-and-cross-origin-frames]], [[iteration-131-measurement-honesty]],
[[iteration-132-cli-polish]].

**Adapted post-iter-129 (2026-08-09):**
- **`navigate --auto-consent` (new in iter-129) does NOT help comparis.ch.** It
  only recognises Sourcepoint-family CMPs (`CMP_TABLE` in
  `crates/ff-rdp-cli/src/commands/consent.rs`); comparis.ch's own consent
  overlay is a different mechanism (a `cmp_noscreen` eval-based workaround,
  not a frame-based CMP). This iteration's `dogfood_path` correctly
  keeps `launch --headless --auto-consent` (Consent-O-Matic, unchanged per
  DEC-023) rather than switching to `navigate --auto-consent` — do not
  "upgrade" it to the new flag while implementing Theme A, it would not fire.
- **Live-Firefox-only protocol bugs are a real, recurring risk for any change
  that touches `RdpTransport`/`actor_request` timing** — iter-129 landed 3 such
  bugs (documented in `kb/rdp/actors/watcher.md`'s iter-129 section and that
  plan's Notes) that zero mock-based unit tests caught: (1) `actor_request`'s
  reply loop (`recv_reply_from`) silently drops any "stray" packet delivered
  before a call's own ACK unless a `swap_event_sink` collector is installed
  around the call — relevant if Theme B's `back`/`forward`/`reload` envelope
  work waits on navigation-completion events the same way `navigate` does; (2)
  clearing a transport's read timeout to `None` instead of restoring its prior
  value makes every subsequent `recv()` hang forever instead of erroring — if
  any theme here temporarily narrows the read timeout for a poll loop (e.g.
  Theme C's empty-buffer detection, if it polls), use the new
  `RdpTransport::read_timeout()` (added in iter-129, `transport.rs`) to save
  and restore the exact prior value rather than guessing. Budget live-Firefox
  testing time accordingly — do not treat "unit tests are green" as a signal
  this iteration's protocol-facing themes (A, B) are done.
