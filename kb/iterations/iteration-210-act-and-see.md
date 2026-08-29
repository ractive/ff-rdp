---
title: "Iteration 210: act-and-see — every state-changing command can return the page it produced"
type: iteration
date: 2026-08-29
status: planned
branch: iter-210/act-and-see
depends_on: []
first_call_sites:
  - primitive: ff_rdp_cli::commands::page_view::collect
    site: crates/ff-rdp-cli/src/commands/navigate.rs (--with-page; also click.rs, type_text.rs, nav_action.rs)
dogfood_path: |
  ff-rdp launch --headless
  ff-rdp launch --headless
  # second call: exit 0, results.already_running == true, same pid as the first — no error
  ff-rdp navigate https://en.wikipedia.org/wiki/Ada_Lovelace --with-page --jq '.results.page.interactive[] | select(.name | test("Babbage"))'
  # expected: one entry {"ref":"e<N>","role":"link","name":"Charles Babbage","href":"/wiki/Charles_Babbage"}
  ff-rdp click --ref e<N> --with-page --jq '.results.page.headings[0]'
  # expected: {"level":1,"text":"Charles Babbage"} — two commands, zero selector guessing
  ff-rdp a11y summary --jq '.results.interactive[0].ref'
  # expected: a ref string, i.e. a11y summary now registers refs like dom does
  ff-rdp type --ref e<M> "Turing Award" --submit --with-page --jq '.results.page.headings[0].text'
  # on the Wikipedia main page search box: expected "Turing Award" (or the search results h1)
tags: [iteration, cli, agent-ergonomics, refs]
---

# Iteration 210: act-and-see

The axi.md browser benchmark ([[axi-benchmark-comparison]], 84 runs, 3 repeats) shows ff-rdp
passing 41/42 tasks but needing **8 turns where chrome-devtools-axi needs 4** on every
click-through task (`wikipedia_infobox_hop`, `wikipedia_link_follow`, `wikipedia_search_click`),
consistently across repeats. The trajectories are all the same shape:

```
ff-rdp navigate <url>                       # returns {status, committed_url, ready_state} — nothing to act on
ff-rdp click "a[href='/wiki/Charles_Babbage']"   # guessed selector, misses
ff-rdp dom "a[href*='Babbage']"             # now the agent has a ref/selector
ff-rdp click "a[href='https://…Charles_Babbage']"
ff-rdp wait --selector .infobox && ff-rdp dom ".infobox td"
```

versus `open <url>` → `click @ref`. Two things cause it: no state-changing command returns
the page it produced, and refs only come out of `dom <selector>`, so the agent must already
know a selector to get a handle. A third, smaller cost: `ff-rdp launch` as the agent's first
browser command failed with exit 1 ("port 6000 already in use") in 3 of 42 runs.

## Themes

- **A — `--with-page`.** `navigate`, `click`, `type`, `reload`, `back`, `forward`, `scroll`
  accept `--with-page` and embed the `a11y summary` view (headings, landmarks, interactive
  elements with refs) under `results.page`. Named after the existing `navigate --with-network`
  precedent; deliberately *not* `--snapshot`, which already names the 50 KB DOM tree.
- **B — Refs everywhere the agent reads.** `a11y summary` and `snapshot` register refs with the
  daemon exactly as `dom` does today, so the first thing an agent sees after navigating already
  carries `click --ref` handles.
- **C — `type --submit`.** Collapse `type → click submit → wait` into one command.
- **D — Idempotent `launch`.** A second `launch` against a port already owned by an
  ff-rdp-launched Firefox returns that instance with exit 0 instead of an error.

## Tasks

### A. `--with-page` [4/4]
- [x] Factor the `a11y summary` collector into `commands/page_view.rs` (`collect(ctx, tab) ->
      PageView`) so `a11y summary` and `--with-page` share one implementation and one JSON shape
- [x] Add `--with-page` to `navigate`, `click`, `type`, and the `nav_action` commands (`reload`,
      `back`, `forward`) and to `scroll`; on success, `results.page = PageView` — same keys as
      `a11y summary` (`headings`, `landmarks`, `interactive`), plus `meta.page_source`
      (`native|js-fallback`, mirroring `a11y`'s `meta.source`)
- [x] `--with-page` waits for `document.readyState == "complete"` (or the command's existing
      `--wait-*`) before collecting, so the page reflects the action's result, not the page it
      left; document the ordering in `--help`
- [x] `--format text` renders `page` with the existing `a11y summary` text renderer beneath the
      command's own line, and prints one hint line: `-> ff-rdp click --ref <ref>  # act on an
      element above`

### B. Refs from the read commands [3/3]
- [x] `a11y summary` registers refs via `daemon::client::register_refs` (daemon route only, as
      `dom` does) and emits `ref` on every `interactive` entry; `meta.refs_registered` as in `dom`
- [x] `snapshot` does the same for nodes marked `interactive: true`
- [x] The `--with-page` payload of Theme A carries the same refs; one registration per command,
      not one per sub-view

### C. `type --submit` [2/2]
- [x] `type … --submit` dispatches Enter on the element and, if the element is inside a `<form>`
      and Enter did not navigate, calls `form.requestSubmit()`; output gains
      `{"submitted": true, "navigated": bool}`
- [x] `--submit` composes with `--with-page` (collect after the resulting navigation settles)

### D. Idempotent `launch` [2/2]
- [x] When the port is busy **and** the owner is a Firefox launched by ff-rdp (the same check
      `--replace` uses to decide what it may stop), `launch` returns exit 0 with
      `results.already_running: true`, the existing `pid`, `port`, `profile`; the error path stays
      for a foreign port owner
- [x] `--replace` behaviour unchanged; `--help` documents the no-op

## Acceptance Criteria [8/9]

- [x] `live_navigate_with_page_returns_headings_and_refs`: `navigate <fixture-page> --with-page`
      → `results.page.headings[0].text` equals the fixture's `<h1>`; every `interactive` entry has
      a `ref` matching `^e\d+$`
- [x] `live_click_ref_from_with_page_lands_on_target`: refs from `navigate --with-page` are
      accepted by `click --ref` and the click's `results.text` matches the ref's `name`
- [x] `live_click_with_page_reflects_post_click_document`: after `click --ref <link> --with-page`
      the returned `page.headings[0]` is the *destination* page's heading, not the origin's
- [x] `live_a11y_summary_registers_refs`: `a11y summary` output has `meta.refs_registered: true`
      and `click --ref` on its first interactive entry succeeds
- [x] `live_snapshot_interactive_nodes_carry_refs`: every `interactive: true` node in `snapshot`
      output has a `ref`
- [x] `live_type_submit_navigates_search_form`: on a fixture form, `type --ref <input> "x"
      --submit` yields `submitted: true` and the resulting URL contains the query
- [x] `live_launch_twice_is_a_noop`: second `launch` on the same port → exit 0,
      `already_running: true`, same `pid`; a listener that is not an ff-rdp Firefox still errors
- [x] `with_page_shape_matches_a11y_summary` (unit): the `page` object and `a11y summary`
      `results` serialise to the same key set
- [ ] Benchmark: re-run [[axi-benchmark-comparison]] `--repeat 3`; average turns on
      `wikipedia_infobox_hop`, `wikipedia_link_follow`, `wikipedia_search_click` ≤ 5 (were 8.0,
      7.3, 8.3) with the same one-paragraph system prompt — record the table in this plan's
      Outcome section
      **NOT DONE — not re-measured.** The mechanisms this AC was supposed to validate all shipped
      and are covered by the seven live tests above, but nobody ran the benchmark, so the turn
      count after this change is unknown and this box stays empty. The harness drives real Claude
      Code agents against live Wikipedia over hours at real per-run cost, and lived in a session
      scratchpad rather than in this repo. Carried over as [[iteration-213-act-and-see-benchmark-rerun]],
      which lands the harness under `tools/` first so the comparison is reproducible at all.
      Note what remains genuinely open: `--with-page` is opt-in, so an unchanged turn count is a
      possible and publishable outcome — it would mean agents do not discover the flag, which is a
      discoverability finding, not a broken feature.
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Outcome

Landed as `iter-210/act-and-see`. Eight of nine acceptance criteria met; the benchmark re-run was
not performed and is carried over (see the AC itself, and [[iteration-213-act-and-see-benchmark-rerun]]).

### What shipped

| Theme | Change |
|---|---|
| A | `crates/ff-rdp-cli/src/commands/page_view.rs` — one collector, one ref registration, one text renderer. `--with-page` on `navigate` (both routes), `click`, `type`, `reload`, `back`, `forward`, and all seven `scroll` subcommands. `results.page`; `meta.page_source` / `page_ready` / `page_refs_registered`. |
| B | `a11y summary` and `snapshot` register refs via `daemon::client::register_refs`, fail-closed exactly as `dom` does (no daemon ⇒ no `ref` field at all). |
| C | `type --submit`: Enter first, `form.requestSubmit()` only when no navigation followed. `results.method` reports which path ran. |
| D | `launch` returns `already_running: true` and exit 0 when the port is held by a Firefox it can prove ff-rdp launched; a foreign owner still errors. `already_running` is present (`false`) on the launching path too. |

### Two decisions the plan did not anticipate

- **`a11y summary` moved off `connect_direct`.** Theme B is impossible otherwise: the ref store
  lives in the daemon, and this command was bypassing it. Nothing in its protocol traffic conflicts
  with the proxy (see the routing table in `dispatch.rs`), so it now takes the normal route.
- **`page_view::attach` refreshes the target actor before collecting.** The console actor cached
  before a click is bound to the old docshell; without the refresh,
  `click --ref <link> --with-page` returns the page it *left*. That is the difference between the
  feature working and it being actively misleading, and it is what
  `live_click_with_page_reflects_post_click_document` pins.

### Live sweep

Gates: `FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1`

```
LIVE_SWEEP_SUMMARY executed=292 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0 total=292
```

CLI tier: 279 passed / 4 failed. Other tiers: 1 + 3 + 3 + 2 passed, 0 failed. 279 + 4 + 9 = 292 =
`executed`, so the record reconciles. All seven `live_210_act_and_see` tests passed.

Four failures, none in changed code paths — one row each in Carry-over below.

## Design notes

- **Why the a11y summary and not `snapshot`.** ff-rdp is 16% cheaper per task than axi in the
  benchmark because its outputs are small enough that agents pipe them through `head`/`grep`;
  axi's `open` returns the full accessibility tree every time and pays for it. The summary view
  is a few hundred tokens on a Wikipedia article; the DOM snapshot is capped at 50 KB. Agents that
  want the tree still run `snapshot`.
- **Refs and staleness.** The daemon already clears the ref store and bumps `nav_generation` on
  navigation (`daemon/server.rs`, `navigation_event_clears_refs_and_increments_generation`), so a
  ref from before a navigation errors rather than clicking the wrong thing. This iteration does
  not add a generation prefix to the id; it only widens *where* refs come from. If a
  same-document SPA route change turns out not to raise the navigation event, that is a
  follow-up plan, not scope creep here.
- **Default-on?** Not in this iteration. `--with-page` is opt-in; whether to default it on for
  JSON is decided after the benchmark re-run shows agents actually reach for it from `--help`.
- **`--submit` is best-effort.** Synthetic Enter is `isTrusted: false` (see `type --help`); the
  `requestSubmit()` fallback is what makes it reliable, and `navigated` tells the agent whether
  anything happened.

## Carry-over

Every non-green line from the sweep, plus the unticked AC. Dispositions per
`.claude/skills/iteration-close`.

| # | Item | Evidence | Disposition |
|---|---|---|---|
| 1 | AC "Benchmark: re-run [[axi-benchmark-comparison]] `--repeat 3`" not done | AC left unticked above | **file** — [[iteration-213-act-and-see-benchmark-rerun]] (validated with `check-iteration-plan`) |
| 2 | `live_166_navigate_document_status::live_166_navigate_reports_document_status` FAILED | `expected HTTP 200, got status: 304` for `https://example.com/`; reproduced on an isolated re-run | **file** — [[iteration-214-live-166-cache-304]] |
| 3 | `live_166_navigate_document_status::live_166_navigate_status_direct_parity` FAILED | same 304, on the `--no-daemon --with-network` route | **file** — same plan, [[iteration-214-live-166-cache-304]]: one defect, one fix |
| 4 | `live_137_daemon_mode_parity::live_137_consent_accept_via_daemon` FAILED in the sweep | `daemon never reported live frame targets`, `live_target_count: 0` after 18 s | **no plan, with a stated reason** — passed on an isolated re-run at `--test-threads=1`; load-sensitive under the sweep's 6 threads. If it fails again on an isolated run, or in two consecutive sweeps, it needs its own plan. |
| 5 | `live_navigate_default_fast::live_navigate_elapsed_matches_wall` FAILED in the sweep | `elapsed_ms (715) must be within ±750ms of measured wall (2187); delta 1472ms` | **no plan, with a stated reason** — passed on the same isolated re-run. The test measures wall-clock including process spawn, which the sweep's parallelism inflates; the ±750 ms band is the load-sensitive part, not the product. Same trigger as row 4 for filing. |

Rows 2 and 3 are one defect: `live_166` hard-asserts HTTP 200 from `https://example.com/`, but
Firefox's HTTP cache makes a repeat visit a conditional request the server answers **304 Not
Modified**. ff-rdp reports what the server sent, which is correct; the test's premise ("a
navigation to a reachable page reports 200") is what is wrong. It reproduced on an isolated
re-run, so it is not load-sensitive. It is **not** caused by this iteration —
`git diff main...HEAD -- crates/ff-rdp-cli/src/commands/navigate.rs` touches only `--with-page`
plumbing and nothing in the document-status path — but "not ours" is a diagnosis, not a
disposition, so it is filed as [[iteration-214-live-166-cache-304]].

## Out of scope

- Hints in JSON output. Agents consume JSON through `--jq`/pipes, where hints are noise; the hint
  surface is `--format text`, the error envelope, and [[iteration-212-ambient-context]].
- `--query` filtering of the page view — [[iteration-211-find-not-guess]].
- Positional ref syntax (`click e12`). Ambiguous with CSS selectors; `--ref` stays.

## References

- [[axi-benchmark-comparison]] — the measurements this plan is answering
- `crates/ff-rdp-cli/src/commands/a11y_summary.rs`, `dom.rs` (`register_refs` call site),
  `daemon/server.rs` (`register-refs`, `nav_generation`), `launch.rs:569` (port-busy error)
- `navigate --with-network` (iter-159) — the naming and embedding precedent
