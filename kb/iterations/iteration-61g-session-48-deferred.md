---
title: "Iteration 61g: navigate blocking + network buffer + sources fallback"
type: iteration
date: 2026-05-17
status: planned
branch: iter-61g/session-48-deferred
depends_on: [iteration-61f-session-48-ergonomics]
tags:
  - iteration
  - dogfood-feedback
  - navigate
  - network
  - sources
  - agent-speed
  - error-ux
---

# Iteration 61g: navigate blocking + network buffer + sources fallback

The three [[dogfooding-session-48]] findings that
[[iteration-61f-session-48-ergonomics]] explicitly deferred because
each needs a design choice rather than a one-line fix. None are
correctness bugs in the protocol layer — they are agent-ergonomics
papercuts that compound across longer sessions.

1. **`navigate` returns before page commit** (session 48 #1). The
   command resolves when the navigation request is acknowledged, not
   when the new document is the active one. An agent doing
   `navigate` → `page-text` in immediate sequence sometimes reads the
   *previous* page's text and acts on stale state.
2. **`network` buffer carries entries across navigations** (#2). A
   webvitals beacon from the previous page showed up as one of the
   "slowest" requests after navigating to a new origin. The watcher
   buffer is cumulative across the daemon's lifetime, mixing pages.
3. **`sources` undercounts inline scripts and degrades under CSP**
   (#7). On Hacker News (CSP-eval-blocked) it returned 1 source for a
   page with 4+ `<script>` blocks. The `fallback_method: "js-eval"`
   path can't run when the page CSP blocks `eval()`, and there's no
   non-eval fallback.

All three converge on the same theme: **commands should be honest
about what they observed and when.** Navigate should not claim
success when the user-visible state hasn't transitioned; network
should be scoped to "since the last navigate" by default; sources
should fall back to a non-eval path before giving up.

## Tasks

### A. `navigate` waits for the new document by default

#### A1. Treat navigate as a transition, not a fire-and-forget — **major** [0/3]
- [ ] After dispatching the navigate, poll the target's current URL
  (or `document.readyState`, or both) for up to `cli.timeout` ms.
  Return as soon as either: (a) the URL changes to one whose origin or
  pathname matches the requested URL, or (b) readyState reaches
  `interactive` / `complete`.
- [ ] Include the observed `committed_url` and `ready_state` in the
  result payload so the agent can confirm what landed:
  `{"navigated": "https://x.example", "committed_url": "https://x.example/", "ready_state": "interactive", "elapsed_ms": 420}`.
- [ ] Preserve the old behaviour behind `--no-wait` for cases where
  the caller wants the previous fire-and-forget semantics
  (e.g. starting a navigation, doing other things, then waiting
  manually with `wait --text` or `wait --selector`).

#### A2. `--wait-for` predicate on navigate — **minor** [0/2]
- [ ] Add `--wait-for <selector|text|eval>` (same vocabulary as
  iter-59) so `navigate URL --wait-for ".athing"` is one call.
- [ ] When `--wait-for` is given, the predicate's timeout uses the
  same `cli.timeout` budget; failure surfaces a selector-aware error
  ("navigated to X but `.athing` did not appear within Ymss").

#### A3. Don't double-wait in the script runner — **minor** [0/1]
- [ ] Audit the runner's `navigate` verb to make sure the new default
  blocking doesn't compound with an explicit `wait` step (no double
  budget consumed, no double-counted `elapsed_ms`).

### B. `network` buffer scoped to the current navigation

#### B1. Mark a navigation boundary in the buffer — **major** [0/2]
- [ ] On every `navigate` (CLI command or in-page navigation observed
  via the watcher), insert a "boundary" marker into the daemon's
  network buffer carrying the new top-level document URL and a
  monotonic sequence number.
- [ ] No buffer truncation — keep the full history. The marker is
  metadata, not a delete.

#### B2. Default `network` to "since last navigation" — **major** [0/3]
- [ ] `ff-rdp network` (no flag) returns only entries observed after
  the most recent boundary. Summary, slowest, by-cause-type all
  recompute against that window.
- [ ] Add `--since <navigation-index|all>`:
  - `--since all` (or `--since 0`) returns the full cumulative buffer
    (old default, kept for parity / debugging).
  - `--since -1` (default) is the current navigation.
  - `--since -2` is one navigation back, etc.
- [ ] `meta` carries the boundary marker that scoped the result:
  `{"since": {"index": -1, "url": "https://x.example/page", "sequence": 17}}`.

#### B3. `network --follow` emits boundary events — **minor** [0/2]
- [ ] When a navigation occurs while a follow is active, emit a
  single NDJSON line: `{"event": "navigation", "url": "...",
  "sequence": N}`. The agent can then reset its own accounting.
- [ ] Document the new event in `kb/reference/network.md` (or the
  in-tree help).

### C. `sources` non-eval fallback

#### C1. Walk the DOM via the WalkerActor — **major** [0/2]
- [ ] When the SourceActor-based listing is empty or unavailable and
  the page's CSP would block the existing `js-eval` fallback, fall
  back to walking `document.scripts` via the WalkerActor's native
  RDP messages (no `evaluate_js_async`). Emit one source entry per
  `<script>` tag with the `src` URL when present, and a synthetic
  `inline://document/<tag-index>` URL for inline scripts.
- [ ] Surface the path used in `meta.fallback_method`:
  `"sources-actor"` (best), `"walker-actor"` (this new fallback),
  `"js-eval"` (existing), or absent when the SourceActor produced
  the full list.

#### C2. CSP detection — **minor** [0/2]
- [ ] Before invoking the `js-eval` fallback, probe whether the page
  CSP allows `eval` (a one-liner via the WalkerActor reading the
  meta CSP header would do, or just attempt a no-op eval and check
  for the CSP exception class).
- [ ] If eval is blocked, skip directly to the WalkerActor fallback
  rather than emitting a CSP exception that contaminates the result.

#### C3. Tests — **required** [0/2]
- [ ] Add a fixture page (or a mock-server response) with three
  `<script>` tags: one external, one inline, one with `src` and a
  CSP that blocks eval. Assert sources returns 3 entries with the
  right URL shapes.
- [ ] Regression test: on a CSP-eval-free page, the result remains
  identical to the pre-iter-61g behaviour (no change to the happy
  path).

### D. Documentation

#### D1. Update `navigate --help` — **trivial** [0/1]
- [ ] Document the new wait-by-default contract, the
  `committed_url` / `ready_state` fields, and `--no-wait` /
  `--wait-for`.

#### D2. Update `network --help` — **trivial** [0/1]
- [ ] Document `--since` and the default-scoped behaviour. Call out
  that `network --since all` is the pre-iter-61g default if anyone
  was scripting against it.

#### D3. Tick the dogfood-session-48 findings — **trivial** [0/1]
- [ ] In [[dogfooding-session-48]], mark #1, #2, #7 as closed once
  this iteration merges, with a pointer back here.

## Acceptance Criteria [0/6]

- [ ] `ff-rdp navigate URL && ff-rdp page-text` from a fresh agent
  session reliably reads the *new* page's text without a `sleep` or
  manual `wait`. Verified against three sites: a fast static page
  (HN), an SPA with client-side routing, and a slow heavy site
  (comparis.ch/hypotheken).
- [ ] `--no-wait` opts back into the pre-iter-61g fire-and-forget
  behaviour; an existing script that relied on it can pass the flag
  and observe identical timing semantics.
- [ ] `ff-rdp network` after a navigation shows zero entries from
  the previous page. `ff-rdp network --since all` returns the full
  cumulative buffer (parity with pre-iter-61g).
- [ ] `ff-rdp network --follow` emits a `{"event": "navigation",
  "url": "..."}` line when the active tab navigates.
- [ ] On a page with CSP that blocks `eval()` and N `<script>` tags
  (a hand-authored fixture + at least one real site, e.g. HN),
  `ff-rdp sources` returns at least N entries. `meta.fallback_method`
  identifies which path produced them.
- [ ] All workspace `cargo test -q` continues to pass; no test in
  `network.rs`, `navigate.rs`, or `sources.rs` is silently downgraded
  to `#[ignore]` to make a regression go away.

## Design Notes

**Why default-block `navigate`?** Every dogfood session for the
last six iterations has hit this: a new agent writes the obvious
chain `navigate → page-text` and gets stale content. The escape
hatch (`--no-wait`) preserves the old behaviour for the rare caller
that needs it (e.g. scripted parallel navigation across tabs). The
common case should be the safe one.

**Why scope `network` to the current navigation by default?** Same
argument. The buffer-mixing bug only bit because the default was
"cumulative since the daemon started" — surprising for anyone who
just navigated to a new page expecting a fresh slate. Cumulative is
still available via `--since all`.

**Why WalkerActor for sources?** Page-side `eval()` is the *one*
thing we can reliably break: any half-decent site CSP blocks it. The
WalkerActor speaks the native RDP DOM walking protocol and is not
gated by content-policy. iter-26 (network) and iter-37 (daemon
fixes) both moved away from eval-based queries for the same reason —
sources is just the last command that didn't.

**Scope discipline.** Resist expanding into the underlying
session-48 finding #4 (`computed --prop` repeatable — already shipped
in iter-61f) or session-47 finding #4 (`wait --wait-timeout` vs
`click --wait-for-timeout` naming — a deliberate breaking-change
candidate worth its own iteration). Three findings, three themes,
one merge.

## References

- [[dogfooding-session-48]] — the bug report this iteration closes.
- [[iteration-61f-session-48-ergonomics]] — the four fixes already
  landed from the same session.
- [[iteration-61e-device-actor-version-keys]] — sibling fix bundle
  from [[dogfooding-session-47]] that 61f stacked on top of.
- [[iteration-59-autowait-pointer-retry]] — the `--wait-for`
  vocabulary that A2 reuses.
- Implicated files:
  - `crates/ff-rdp-cli/src/commands/navigate.rs` — task A.
  - `crates/ff-rdp-core/src/connection.rs` + the daemon's network
    buffer code — task B1, B2.
  - `crates/ff-rdp-cli/src/commands/network.rs` — task B2, B3.
  - `crates/ff-rdp-cli/src/commands/sources.rs` — task C.
  - `crates/ff-rdp-core/src/actors/walker.rs` (or equivalent) — task
    C1 helper.
