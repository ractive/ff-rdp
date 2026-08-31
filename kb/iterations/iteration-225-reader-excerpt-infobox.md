---
title: "Iteration 225: the reader excerpt drops the infobox — the fact the task asks for is not in --with-page"
type: iteration
date: 2026-08-31
status: in-progress
branch: iter-225/reader-excerpt-infobox
depends_on:
  - 219
dogfood_path: |
  ff-rdp launch --headless
  ff-rdp navigate 'https://en.wikipedia.org/wiki/Python_(programming_language)' --with-page --page-chars 4000 \
    --jq '.results.page.excerpt | test("Stable release")'
  # TODAY: false — Readability classifies table.infobox as boilerplate and the excerpt is prose only
  ff-rdp page-text --query 'Stable release' --jq '.meta.matches'
  # 3 — the fact is on the page; the agent needs a second command to reach it
  ff-rdp navigate 'https://en.wikipedia.org/wiki/Python_(programming_language)' --with-page --query 'Stable release' \
    --jq '.results.page | {matches, excerpt}'
  # TODAY: matches 0 and an empty excerpt — --query on the view searches only the reader text
  # expected AFTER: the key/value facts from the page's infobox/definition lists appear under
  #   results.page.facts (or in the excerpt), and --query with no reader-text hit falls back to
  #   the page's innerText window so a one-command answer exists
tags:
  - iteration
  - act-and-see
  - page-view
  - readability
  - agent-ergonomics
---

# Iteration 225: the reader excerpt drops the infobox

## Why

The 2026-08-31 two-task re-measurement in [[axi-benchmark-comparison]] (ff-rdp `5a0071d`,
after [[iteration-219-reader-view-page]] and [[iteration-220-with-page-after-navigating-click]])
still reads 7.7 turns on `wikipedia_link_follow` and 10.3 on `wikipedia_infobox_hop` (axi: 4.0 /
4.0). Discoverability is no longer the reason — every one of the six runs used `--with-page` and
`--query`, and `--help | head -50` shows the idioms on lines 14–16. The turns go to `page-text
--query` round trips, 2–6 per run, because the fact the task asks for is **not in the view**:

- Python article, `--with-page --page-chars 4000`: `excerpt` starts with the lede and never
  contains "Stable release". `page-text --query 'Stable release'` finds it three times. Readability
  scores `table.infobox` as boilerplate — correct for reading, wrong for answering.
- Python Software Foundation article: "Formation 2001" is an infobox row; the excerpt has the
  year only in a body sentence, and `--query formed` finds nothing.
- `--with-page --query X` on the view searches the reader text only, so a miss returns
  `matches: 0` and an empty excerpt — the agent's next move is `page-text --query X`, one more
  turn, on the same page it just fetched.

The 5-turn run that did happen (`link_follow` run 1: `navigate --with-page` → `click --ref
--with-page` → `page-text --query` → answer) shows the floor: even the best trajectory pays one
`page-text` because the birth date is in the infobox.

## Themes

- **A — Structured facts from the page, next to the excerpt.** `results.page.facts`: key/value
  pairs harvested from `table.infobox` rows (`th`/`td`), `<dl>` definition lists, and
  `[itemprop]` microdata — capped (say 40 rows), in DOM order, each `{key, value}` with values
  normalized like accessible names. Wikipedia, MDN, GitHub repo sidebars, most product pages have
  one. Readability is not consulted for this; it is a separate, cheap DOM pass in the same
  collector script.
- **B — `--query` falls back to the page.** When `--query` matches nothing in the reader text and
  nothing in `facts`, the excerpt becomes the `page-text --query` window over `innerText` (reuse
  `page_text::build_excerpt`), with `page.query_source: "readability" | "facts" | "innertext"`
  saying which. One command answers or says "not on this page"; two commands never search the
  same page twice.
- **C — Measure.** Same two tasks, `--repeat 3`, same harness. Target ≤ 5 turns average on
  both. Record the number whatever it is.

## Tasks

### A. Facts [3/3]
- [x] Collector: harvest `{key, value}` rows from `table.infobox` / `.infobox` / `table[class*=infobox]`,
      `dl` (`dt`/`dd`), and `[itemprop]`; cap at 40; values via `__ffrdpAccName`-style
      normalization; `facts_total` / `facts_truncated` when the cap bites
- [x] `render_text` shows facts as `key: value` lines after the excerpt
- [x] `--query` matches facts too (`page.matches` counts them)

### B. Fallback [2/2]
- [x] `--query` with zero reader/facts hits → innerText window, `query_source: "innertext"`
- [x] `--query` with zero hits anywhere → `matches: 0`, `excerpt: ""`, and a `hint` naming
      `page-text --full --query` as the exhaustive next step

### C. Measure [0/1]
- [ ] `wikipedia_link_follow` + `wikipedia_infobox_hop`, `--repeat 3`, recorded in Outcome and in
      [[axi-benchmark-comparison]]

## Acceptance Criteria [3/4]

- [x] Python article `--with-page` returns a `facts` row with key matching /Stable release/ (live
      test on the recorded fixture)
- [x] `--with-page --query 'Stable release'` on that page answers in one command (`matches ≥ 1`,
      `query_source` set)
- [ ] Two-task benchmark average ≤ 5 turns on both tasks — or the measured number recorded and
      the AC left unticked, never reworded
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q`
      clean; live sweep reconciles [2026-08-31: FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 →
      LIVE_SWEEP_SUMMARY executed=320 skipped=0 preexisting=0 vanished=0 launch_timeout=0
      timed_out=0 total=320, 320 passed / 0 failed, P+F == executed, exit 0]

## Design notes

- **Why not make Readability keep tables.** `keepClasses` / `classesToPreserve` do not change
  what `_clean`/`_removeNodes` decide; forcing tables through would drag in navboxes and
  reference tables that are exactly the chrome 219 removed. A separate fact pass is smaller and
  does not fork the vendored library.
- **Facts are not a second excerpt.** 40 short rows, not a second 1500-char block; the cap is
  the token discipline `--with-page` has kept since 210.

## Out of scope

- The 1-in-5 daemon reset on `click --ref --with-page` — [[iteration-224-with-page-daemon-connection-reset]].
- Default-on for `--with-page` — [[iteration-213-act-and-see-benchmark-rerun]] Theme C, after C here.

## References

- [[axi-benchmark-comparison]] — 2026-08-31 two-task numbers and trajectories
- [[iteration-219-reader-view-page]] — the reader view this extends
- [[main-content-extraction-crates]] — why Readability, and its known blind spot for tabular facts
- [[iteration-228-two-task-benchmark-after-facts]] — the carry-over measurement (Theme C)

## Outcome

**Themes A and B shipped; Theme C was not run, and its acceptance criterion is left unticked.**

### What landed

- `results.page.facts` — up to 40 `{key, value}` rows harvested by a separate DOM pass
  (`FACTS_BLOCK_JS` in `crates/ff-rdp-cli/src/commands/page_view_js.rs`) over `table.infobox` /
  `.infobox` / `table[class*=infobox]` rows, `dl > dt` + its following `dd`s, and `[itemprop]`
  microdata, in one document-order query. Keys are capped at 120 characters and values at 300;
  a raw cell longer than 1 200 is prose in a table and is dropped rather than truncated.
  `facts_total` / `facts_truncated` report what the cap hid. Readability is not consulted, and
  the pass writes nothing to the DOM — `live_225_the_facts_pass_leaves_the_dom_untouched` pins
  that.
- `--query` now matches facts (on `key` *and* `value`), and `page.matches` counts **matching
  excerpt lines as well as matching entries**. iter-219 counted entries only, which reported
  `matches: 0` beside a perfectly good excerpt window whenever the hit was in the prose — the
  exact signal an agent uses to decide whether to spend another turn.
- `page.query_source` — `readability` | `innertext` | `facts` — says which of the three searched
  places answered. On a miss in the article text *and* the facts, `collect` fetches
  `document.body.innerText` (bounded by `QUERY_TEXT_BUDGET`, normalised to one non-blank line per
  block) and builds the window with `page_text::build_excerpt`, i.e. literally the selection the
  follow-up `page-text --query` would have produced. That second round trip is paid only on the
  miss.
- A miss in all three leaves `matches: 0`, `excerpt: ""` and sets `page.hint` naming
  `page-text --full --query` — the one command that genuinely searches more than the view just
  did.
- `--format text` renders a `FACTS (n of total)` block after the excerpt, and labels the excerpt
  with `query_source` when a query is active.

### What was measured, and what was not

- Gates: `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo test --workspace -q` all clean (1 189 unit tests + 39 suites green), plus every `xtask
  check-*` gate.
- **Theme C was not run.** The harness (`axi/bench-browser matrix --condition ff-rdp --task
  wikipedia_link_follow,wikipedia_infobox_hop --repeat 3`) drives its agent through a browser on
  port 6000, and at that point the port was held by a headless Firefox started four hours earlier
  that this run had not launched. `CLAUDE.md` forbids tearing down a browser this run did not
  start, and sharing one makes every turn count meaningless — so the number is unmeasured rather
  than wrong. Filed as [[iteration-228-two-task-benchmark-after-facts]], which is a measurement
  iteration with no product change in it.
- Acceptance criterion 3 ("two-task benchmark average ≤ 5 turns on both tasks") is therefore
  **unticked**, with its premise intact: nothing here says the target was met or that the
  criterion was wrong.

### Carry-over

- [[iteration-228-two-task-benchmark-after-facts]] — the measurement, on a browser the run owns.
- [[iteration-229-resource-bus-subscribe-timeout]] — the load-sensitive subscribe timeout this
  iteration's first sweep surfaced.

### The sweep, and what it caught

`FF_RDP_LIVE_TESTS=1 FF_RDP_LIVE_NETWORK_TESTS=1 cargo run -p xtask -- live-sweep` →
`LIVE_SWEEP_SUMMARY executed=320 skipped=0 preexisting=0 vanished=0 launch_timeout=0 timed_out=0
total=320`, 320 passed / 0 failed (P + F reconciles with `executed`), exit 0. All five
`live_225_reader_facts` tests executed and passed.

The **first** sweep of this branch was red, twice over, and both are recorded rather than re-run
away:

- `live_219_query_narrows_the_embedded_page_view` asserted `matches == interactive.len()`, which
  is iter-219's entries-only counting. Widening `matches` to include excerpt-line hits made it
  see 3 where the entry count was 1. Fixed in this iteration: the assertion now treats the entry
  count as a lower bound and additionally pins `query_source`. The semantics change is the
  deliberate one, not the test.
- `live_61q_resource_bus::live_resource_dedupe` timed out on its first `subscribe` — one failure
  in 311 tests at `--test-threads=6`, green alone in 2.6 s and green in the second full sweep.
  Nothing here touches the resource bus. Filed as
  [[iteration-229-resource-bus-subscribe-timeout]]; "environmental" is a diagnosis, not a
  disposition.
