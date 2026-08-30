---
title: "Iteration 218: home view — one RDP connection instead of two"
type: iteration
date: 2026-08-30
status: planned
branch: iter-218/home-view-single-connect
depends_on: [212]
first_call_sites:
  - primitive: ff_rdp_cli::commands::connect_tab::connect_and_list_tabs
    site: crates/ff-rdp-cli/src/commands/home.rs (replaces the separate browser_and_tabs + page_block connects)
dogfood_path: |
  ff-rdp launch --headless && ff-rdp navigate https://example.com
  ff-rdp --jq '.results.tabs, .results.page.interactive[0].ref'
  # same output as before this iteration — the AC is round-trip count, not shape
tags: [iteration, cli, agent-ergonomics, performance]
---

# Iteration 218: home view — one RDP connection instead of two

Carry-over from [[iteration-212-ambient-context]]'s local review pass on PR #232 (finding #4, a
code-review subagent report; `kb/decision-log.md` DEC-050 covers 212 itself). `commands/home.rs`
opens two independent RDP connections per invocation: `browser_and_tabs` calls
`RdpConnection::connect` directly to list every tab, then — when a page is loaded — `page_block`
opens a *second*, separate connection (`connect_and_get_target` or `connect_direct`) to resolve
the focused tab's target and collect its accessibility view. Two connects means two TCP round
trips and two RDP handshakes for one invocation of a command whose own module docs call it "a
standing tax on the context window" because the `SessionStart` hook runs it on every agent
session start — which argues for minimizing round trips, not doubling them.

Not fixed in PR #232 itself: `connect_tab.rs` is a shared module used by nearly every command
(`ConnectedTab` resolves to *one* target tab, not the full list `RootActor::list_tabs` returns),
so merging the two call sites is an API change to that shared surface, not a local edit to
`home.rs` — too much regression surface for a same-day review-fix pass on a PR whose live sweep
had already gone green.

## Themes

- **A — One connection, two results.** Add a `connect_tab.rs` primitive that connects once, lists
  every tab (`RootActor::list_tabs`), and resolves the focused tab's target on the *same*
  connection — so `home.rs` gets both the tab list and the accessibility-view handle without a
  second connect. Every other caller of `connect_and_get_target`/`connect_direct` is unaffected;
  this is an addition, not a signature change to the existing functions.
- **B — Wire it into `home.rs`.** Replace the `browser_and_tabs` + `page_block` pair with the new
  primitive; behavior (JSON shape, hints, text rendering, `--hook` trimming) must be
  byte-for-byte identical — this is a performance change, not a behavior change.

## Tasks

### A. Shared single-connect primitive [0/2]
- [ ] `connect_tab.rs`: a function that connects once (direct or via daemon, same routing rule
      `page_block` already uses: daemon-routed only when a daemon is already running, per
      [[decision-log]] DEC-050's "starts nothing" rule), lists every tab, and resolves the
      target for the accessibility-view collection on that same connection
- [ ] Unit tests for the new primitive's tab-list/target-resolution split, independent of `home.rs`

### B. Wire into the home view [0/2]
- [ ] `home.rs` uses the new primitive instead of two separate connects; `browser_and_tabs` and
      `page_block`'s connection logic are retired (or reduced to thin wrappers if other callers
      still need the old shape)
- [ ] Live test: a single-connection assertion (e.g. a connection-count counter in the test
      harness, or a network-level check) proving the round-trip count actually dropped from 2 to 1

## Acceptance Criteria [0/4]

- [ ] `home_view_output_unchanged_by_single_connect` (unit, fixture-driven): the JSON `results`
      payload for a representative (browser up, page loaded) scenario is identical before and
      after this refactor — this is a performance change, not a behavior change
- [ ] A live or unit test proves exactly one RDP connection is opened per `ff-rdp` invocation when
      a page is loaded (the two-connection case this iteration removes)
- [ ] The three `live_212_ambient_context` live tests (`live_home_with_page_lists_tabs_and_refs`,
      `live_home_with_blank_tab_asks_for_a_navigate`, `live_home_hook_form_is_trimmed`) still pass
      unmodified — the refactor must not change what they assert
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Out of scope

- Any change to `home.rs`'s JSON shape, hints, or text rendering — this iteration is purely about
  connection count.
- The other four review findings from PR #232's local pass (idiom-table bug, atomic settings
  write, `shell_quote` escaping, the `apply_install` coercion asymmetry) — those were fixed
  directly in PR #232, not carried over.

## References

- [[iteration-212-ambient-context]] — the command this optimizes, and the review that found it
- [[decision-log]] DEC-050 — "the home view starts nothing" (the daemon-routing rule the new
  primitive must preserve)
