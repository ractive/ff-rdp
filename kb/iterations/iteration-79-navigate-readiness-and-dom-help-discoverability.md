---
title: "Iteration 79: Fix navigate readiness-event misses + surface styles/computed under dom --help"
type: iteration
date: 2026-05-25
status: in-progress
branch: iter-79/navigate-readiness-and-dom-help
depends_on:
  - iteration-77-spec-drift-and-windows-reparse-points
firefox_refs:
  - path: devtools/shared/specs/watcher.js
    lines: "20-32"
    why: "watchResources / unwatchTargets — confirms the resource-subscription contract used to deliver document-event resources (dom-loading / dom-interactive / dom-complete)."
  - path: devtools/server/actors/resources/document-event.js
    lines: "1-120"
    why: "Source of the document-event resource stream. Confirms when each event fires server-side and whether late subscribers receive a replay or only future events — this is the suspected root cause of the missed events on real sites."
kb_refs:
  - kb/rdp/actors/watcher.md
  - kb/rdp/from-our-codebase/open-gaps.md
first_call_sites: []
dogfood_path: |
  # Theme A — navigate readiness regression on a real site.
  # Pre-iter-79: both calls below time out at the default 10s budget AND at
  # 30s, even though `--no-wait` + an eval shows readyState=complete within
  # seconds.  Post-iter-79: both succeed under the default timeout.
  ff-rdp launch --auto-consent --headless
  ff-rdp navigate https://tennis-sepp.ch                       # --wait complete (default)
  ff-rdp navigate https://tennis-sepp.ch --wait interactive    # should be faster still
  pkill -f 'firefox.*ff-rdp-profile'

  # Theme B — dom help mentions styles/computed.
  ff-rdp dom --help | grep -E 'styles|computed'                # must match at least one line
tags: [iteration, navigate, document-event, cli-help, dogfood]
---

Dogfooding report (2026-05-25) surfaced two paper-cuts that this iteration
addresses:

- **A. `navigate` times out on real sites even when the page is fully loaded.**
  Reproduced locally against `https://tennis-sepp.ch`: `ff-rdp navigate`
  returns `Timeout: page did not fire dom-complete within the timeout` after
  10s (and after 30s with `--timeout 30000`), and the same with
  `--wait interactive`.  A `--no-wait` navigate followed by
  `ff-rdp eval 'document.readyState'` returns `"complete"` within seconds —
  proving the page does fire the events, but `wait_for_doc_complete` in
  `crates/ff-rdp-cli/src/commands/navigate.rs` never observes them.  The
  most-likely root cause is a subscription race: `watchResources` for
  `document-event` is issued after the events have already fired, and the
  Firefox `DocumentEventResource` actor does not replay past events to
  late subscribers (see `firefox_refs` above).

- **B. `dom --help` does not mention `styles` or `computed`.**  Same
  dogfooding session: the reporter reached for `ff-rdp dom --include-style`
  (does not exist) and worked around it with `ff-rdp eval
  getComputedStyle(...)` — even though `ff-rdp styles <SEL>` and
  `ff-rdp computed <SEL>` already exist and do exactly what was wanted.
  The two commands are listed in the top-level `--help` under "CSS & styles",
  but a user who reaches for `dom` first has no signpost from there.

## Tasks

- [x] **A.1** Reproduce the timeout in a unit/live test that drives `navigate`
      against a fixture/page where the document-event resources fire before
      the subscription is established. → `navigate_subscribes_before_navigateto`
      (unit test, captures outbound packet order on a mock TcpListener) +
      `live_navigate_default_wait_reaches_complete` (live test against
      `https://tennis-sepp.ch`).
- [x] **A.2** Fix the subscription race in
      `crates/ff-rdp-cli/src/commands/navigate.rs`: root cause was a missing
      `watchTargets("frame")` call. Per the Firefox watcher contract a
      `WatcherActor` delivers nothing until BOTH `watchTargets("frame")` and
      `watchResources([...])` have been issued. The non-`--with-network`
      branch in `run_core` now calls `WatcherActor::watch_targets` before
      `ResourceCommand::subscribe`, so document-event resources actually
      flow on real pages.
- [x] **A.3** Added live regression test
      `crates/ff-rdp-cli/tests/live_navigate_readiness.rs::live_navigate_default_wait_reaches_complete`
      — navigates to `https://tennis-sepp.ch` under the default timeout
      and asserts the command exits 0 with `ready_state=complete` and a
      non-empty `committed_url`. Gated `FF_RDP_LIVE_NETWORK_TESTS=1`.
- [x] **B.1** Extended `Commands::Dom` `long_about` in
      `crates/ff-rdp-cli/src/cli/args.rs` with a two-line "See also:"
      footer naming `ff-rdp styles` and `ff-rdp computed`.
- [x] **B.2** Added e2e test
      `crates/ff-rdp-cli/tests/dom_help_mentions_styles.rs::dom_help_mentions_styles_and_computed`
      that runs `ff-rdp dom --help` and asserts the output contains both
      `styles` and `computed` (case-insensitive).

## Acceptance Criteria [3/3]

- [x] `live_navigate_default_wait_reaches_complete`: `ff-rdp navigate
      https://tennis-sepp.ch` exits 0 under the default `--timeout` and
      default `--wait complete` and reports a non-empty `committed_url`
      with `ready_state=complete`. Gated `FF_RDP_LIVE_NETWORK_TESTS=1`.
      Test: `crates/ff-rdp-cli/tests/live_navigate_readiness.rs`.
- [x] `dom_help_mentions_styles_and_computed`: running `ff-rdp dom --help`
      contains both `styles` and `computed` (case-insensitive), backed by
      `crates/ff-rdp-cli/tests/dom_help_mentions_styles.rs`.
- [x] `navigate_subscribes_before_navigateto` (unit test in
      `crates/ff-rdp-cli/src/commands/navigate.rs`): the navigate prelude
      issues `watchTargets("frame")` → `watchResources(["document-event"])`
      → `navigateTo` in that exact order, verified by capturing outbound
      packets on a mock TcpListener server.

## Out of scope

- Reworking the `--wait` flag semantics or default level (the default
  remains `dom-complete`).  This iter only fixes the subscription bug so
  the existing default actually works on real pages.
- Adding a `--include-style` flag to `dom`.  Planned separately in
  [[iteration-80-ff-rdp-ergonomics-bundle]] (Theme D); kept out of this
  iter so the discoverability fix can land independently.
- Renaming or restructuring the `styles` / `computed` subcommands.

## References

- [[iteration-77-spec-drift-and-windows-reparse-points]] — related
  spec-fidelity work on watcher resource subscriptions.
- Dogfooding report (2026-05-25, in-session chat).
