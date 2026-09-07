---
title: "Iteration 239: home view — one RDP connection instead of two"
type: iteration
date: 2026-08-30
status: in-review
branch: iter-239/home-view-single-connect
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

# Iteration 239: home view — one RDP connection instead of two

> **Renumbered 218 → 239 on 2026-09-06** so the pending queue runs as one contiguous sweep (DEC-051). Older PRs, commits and sweep logs cite it as iteration 218.

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

### A. Shared single-connect primitive [2/2]
- [x] `connect_tab.rs`: a function that connects once (direct or via daemon, same routing rule
      `page_block` already uses: daemon-routed only when a daemon is already running, per
      [[decision-log]] DEC-050's "starts nothing" rule), lists every tab, and resolves the
      target for the accessibility-view collection on that same connection
- [x] Unit tests for the new primitive's tab-list/target-resolution split, independent of `home.rs`

### B. Wire into the home view [2/2]
- [x] `home.rs` uses the new primitive instead of two separate connects; `browser_and_tabs` and
      `page_block`'s connection logic are retired (or reduced to thin wrappers if other callers
      still need the old shape)
- [x] Live test: a single-connection assertion (e.g. a connection-count counter in the test
      harness, or a network-level check) proving the round-trip count actually dropped from 2 to 1

## Acceptance Criteria [2/4]

- [x] `home_view_output_unchanged_by_single_connect` (unit, fixture-driven): the JSON `results`
      payload for a representative (browser up, page loaded) scenario is identical before and
      after this refactor — this is a performance change, not a behavior change
- [x] A live or unit test proves exactly one RDP connection is opened per `ff-rdp` invocation when
      a page is loaded (the two-connection case this iteration removes)
- [ ] The three `live_212_ambient_context` live tests (`live_home_with_page_lists_tabs_and_refs`,
      `live_home_with_blank_tab_asks_for_a_navigate`, `live_home_hook_form_is_trimmed`) still pass
      unmodified — the refactor must not change what they assert
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Implementation notes

- The primitive is `connect_tab::connect_and_list_tabs(cli, TabListRouting) -> Result<TabListing,
  TabListError>` plus `TabListing::attach(cli) -> Result<ConnectedTab, AppError>`.
  `handshake_and_resolve_tab` — the shared path every other command still reaches through
  `connect_and_get_target` / `connect_direct` — is now those two calls in sequence, so the merge
  added no second code path to keep in step.
- **The route is a parameter, not a lookup.** `resolve_connection_target` *starts* a daemon when it
  does not find one, which the home view must never do (DEC-050). `TabListRouting::RunningDaemon`
  takes the registry entry the caller already read, so the "starts nothing" rule is now held by the
  type rather than by `home.rs` remembering to guard the call — and the daemon registry is read
  once per invocation instead of twice.
- **Two things kept the payload identical where a naive merge would have moved it.**
  `TabListing::greeting_version()` is the greeting's version, *not* the device-actor fallback
  `attach` resolves: the old `browser_and_tabs` reported the greeting value, and reporting a
  better one would still have been a change. And `TabListError` carries the raw transport reason
  alongside the `AppError`, because `browser.detail` used to hold `ProtocolError`'s one-line text,
  not `AppError::Connection`'s multi-line `hint:` block.
- **One deliberate behaviour addition.** A registry entry can outlive its daemon. Before this
  iteration the `browser` block came from its own *direct* connect, so a dead proxy cost the
  `page` block and nothing else; now that both share a connection, `connect_once` retries direct
  when the daemon-routed attempt fails, rather than reporting a running Firefox as unreachable and
  sending the agent to `launch`. That is one extra connect on a failure path only.
- **Task B2's assertion is an e2e counting mock, not live Firefox.** The task line says "live
  test"; what landed is `e2e_239_home_with_a_page_opens_one_connection`, which spawns the real
  `ff-rdp` binary against a `TcpListener` that replays the recorded `list_tabs_response.json` /
  `get_target_response.json` and counts accepted connections. A live Firefox cannot be asked how
  many times it was connected to, and on the daemon route the proxy hides the count entirely — so
  the "connection-count counter in the test harness" half of the task's own parenthetical is the
  only form of this assertion that can exist. It was verified to have teeth: adding a second
  `connect_and_list_tabs` call to `connect_once` makes it fail with `left: 2, right: 1`.

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
