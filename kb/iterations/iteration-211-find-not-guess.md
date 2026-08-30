---
title: "Iteration 211: find, don't guess — --query on the read commands and a cap on page-text"
type: iteration
date: 2026-08-29
status: in-review
branch: iter-211/find-not-guess
depends_on:
  - 210
first_call_sites:
  - primitive: ff_rdp_cli::output_controls::QueryFilter
    site: >-
      crates/ff-rdp-cli/src/commands/page_text.rs (--query; also snapshot.rs,
      a11y_summary.rs, dom.rs)
dogfood_path: |
  ff-rdp navigate https://en.wikipedia.org/wiki/World_population
  ff-rdp page-text --query "billion" --jq '.meta'
  # expected: {"matches": N, "shown": N, "context_lines": 2, "truncated": false, ...} and
  # results is only the matching lines with ±2 lines of context — not the whole article
  ff-rdp page-text --jq '.meta.truncated, .meta.total_chars'
  # expected: true, <a number well above 8000> — page-text is now capped by default
  ff-rdp page-text --full --jq '.results | length'
  # expected: the full innerText length (== meta.total_chars above)
  ff-rdp snapshot --query "1804" --jq '.results'
  # expected: only the table subtree(s) containing "1804", with ancestors kept
  ff-rdp a11y summary --query "Babbage" --jq '.results.interactive'
  # expected: just the matching link(s), each with a ref
  ff-rdp dom "h3 a" --text --jq '.results[0]'
  # on github.com/facebook/react/issues: expected the full issue title, not the "Bug:" prefix
tags:
  - iteration
  - cli
  - agent-ergonomics
  - output
---

# Iteration 211: find, don't guess

In the axi.md benchmark ([[axi-benchmark-comparison]]) the extraction tasks cost ff-rdp
9.3 turns vs 4 (`tabular_data_analysis`) and 10.7 vs 7 (`wikipedia_deep_extraction`), and
produced the only failure (`github_issue_investigation` run 1). Every one of those trajectories
is the same loop: `page-text | head -100` (the answer is further down), `dom <guessed
selector>`, then three to six `eval` scripts until one hits. The agent has no way to say "show me
the part of the page that contains *billion*". `page-text` is also the only read command with no
size cap — the `| head -100` is the agent working around that, and it is why the answer was cut
off.

The failure is the same gap taken to the end: the agent's selectors returned only the `Bug:`
label span of four GitHub issue titles, and it reported those.

## Themes

- **A — `--query <pattern>`** on `page-text`, `snapshot`, `a11y summary`, `dom`: return only what
  matches (plus context), with match counts in `meta`.
- **B — Cap `page-text`** by default with an honest `truncated`/`total_chars`/`--full` triple,
  like `snapshot` already has.
- **C — Full accessible names from `dom --text`.** Element text must be the element's whole
  accessible name, not its first text node.

## Tasks

### A. `--query` [5/5]
- [x] `QueryFilter` in `output_controls.rs`: case-insensitive substring by default, `--query-regex`
      for a regex; one implementation shared by the four commands
- [x] `page-text --query`: emit matching lines with `--context N` lines either side (default 2),
      `meta.matches`, `meta.shown`, and the existing truncation hint when `shown < matches`
- [x] `snapshot --query`: keep every node whose text or attribute values match, plus its ancestors;
      prune everything else; `meta.matches`
- [x] `a11y summary --query`: filter `headings`, `landmarks`, `interactive` by `text`/`name`;
      refs (from [[iteration-210-act-and-see]]) are still registered for the survivors
- [x] `dom <selector> --query`: filter the matched elements by accessible name / text; a
      selector-only call is unchanged

### B. `page-text` cap [2/2]
- [x] Default `--max-chars 8000`; output gains `meta.total_chars`, `meta.truncated`, and when
      truncated the hint `showing 8000 of N chars, use --full or --query <text>`
- [x] `--full` lifts the cap; `--max-chars 0` is rejected (same rule as `--max-frame-mb`)

### C. Accessible names [1/1]
- [x] `dom --text` and the `name` field use the same accessible-name computation as
      `a11y summary` (label, `aria-label`, `aria-labelledby`, then full descendant text), so an
      `<h3><a>Bug: <span>…</span></a></h3>` title comes back whole

## Acceptance Criteria [7/8]

- [x] `live_page_text_query_returns_only_matching_lines_with_context`: on a fixture page with the
      needle on line 40 of 60, `page-text --query needle` returns 5 lines and
      `meta.matches == 1` [2026-08-30: green in both PR-body sweeps and in an isolated
      `FF_RDP_LIVE_TESTS=1 cargo test -p ff-rdp-cli --test live live_211_find_not_guess` re-run
      during merge review]
- [x] `live_page_text_is_capped_by_default`: on a fixture with 20 000 chars of text,
      `results` length ≤ 8000, `meta.truncated == true`, `meta.total_chars == 20000`;
      `--full` returns all of it [2026-08-30: green, same re-run]
- [x] `live_snapshot_query_keeps_ancestors_of_matches`: `snapshot --query <cell text>` on a
      fixture table returns a tree whose leaf is the matching cell and whose root is still `html`
      [2026-08-30: green, same re-run]
- [x] `live_a11y_summary_query_filters_and_keeps_refs`: the filtered `interactive` entries all
      match and each carries a `ref` that `click --ref` accepts [2026-08-30: green, same re-run]
- [x] `live_dom_text_returns_full_accessible_name`: fixture `<h3><a>Bug: <span>title</span></a></h3>`
      → `dom "h3 a" --text` yields `"Bug: title"` [2026-08-30: green, same re-run]
- [x] `query_filter_is_case_insensitive_substring_by_default` (unit) and
      `query_regex_rejects_invalid_pattern_with_exit_2` (unit)
- [ ] Benchmark: re-run [[axi-benchmark-comparison]] `--repeat 3`; `tabular_data_analysis` and
      `wikipedia_deep_extraction` average ≤ 6 turns (were 9.3, 10.7) and
      `github_issue_investigation` passes 3/3 — record the table in this plan's Outcome section
- [x] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q` clean.

## Outcome

Shipped as PR (branch `iter-211/find-not-guess`).

**What landed**

| Theme | Where |
| --- | --- |
| A — `--query` / `--query-regex` | `QueryFilter` + `QueryArgs` in `crates/ff-rdp-cli/src/output_controls.rs` and `cli/args.rs`, flattened into `page-text`, `snapshot`, `a11y summary` and `dom` |
| B — `page-text` cap | `build_excerpt` in `crates/ff-rdp-cli/src/commands/page_text.rs`: `--max-chars 8000` default, `--full`, `meta.total_chars` / `meta.truncated` / `meta.matches` / `meta.shown` / `meta.match_lines` |
| C — accessible names | `__ffrdpAccName` in `crates/ff-rdp-cli/src/commands/js_helpers.rs`, used by `dom`'s ARIA-tree `name`, `dom --text`, and all four `page_view` sections |

**Decisions taken during implementation**

- **`--query-regex` is compiled by clap's value parser, not by the command.** An unparseable
  pattern is therefore a *usage* error (exit 2), rejected before any connection to Firefox is
  opened. Routing it through `AppError::User` would have been exit 1 and would have cost a
  browser round-trip first.
- **`--query` is a per-command flag, not a global one.** `--limit`/`--fields` are global, but a
  global `--query` would be silently inert on `console`, `network`, `perf`, and everything else
  that does not implement it — the exact "flag accepted, nothing happened" failure iter-161
  Theme D removed for `--fields`/`--sort`.
- **`a11y summary --query` collects uncapped and caps after filtering.** The cap defaults to 50
  interactive entries; capping first would hide precisely the control past entry 50 that the
  caller queried for. Refs are minted for the uncapped set in that case, leaving
  registered-but-unreferenced entries in the daemon — the same harmless trade `snapshot` already
  makes.
- **`snapshot --query` keeps a matching node *whole*** (subtree included) and keeps
  non-matching ancestors as a path only. A no-match query prunes to `null`, not to the whole
  page: handing an agent that asked for "billion" the entire document back would read as "here
  are your matches".
- **The accessible-name cap moved from 100 to 300 characters**, not to unbounded —
  `__ffrdpAccName(document.body)` would otherwise return the whole page. A name that does hit
  the cap now ends in `…` rather than stopping silently.
- **`page-text --max-chars` counts characters, not bytes**, so the number matches what
  `--jq '.results | length'` reports and multi-byte text is never cut mid-codepoint.

**Not done**

- The benchmark re-run (`axi-benchmark-comparison --repeat 3`) is left **unticked and not
  reworded**. It is a multi-hour, many-API-call harness run against live sites that this
  implementation pass could not execute, so the turn-count claim in that AC is unverified — the
  code is in, nothing here measures whether it moved 9.3 → ≤6. Folded into
  [[iteration-213-act-and-see-benchmark-rerun]] rather than filed separately: same harness, same
  42 tasks, same money, and 213 already carries iter-210's identical unticked AC.

**Behaviour changes a caller can see**

- `page-text` returns at most 8000 characters unless `--full` or `--max-chars N` is passed.
  Anything scripted against uncapped `page-text` output needs `--full`;
  `live_dom_text_longstring_roundtrip` was updated in this PR for exactly that reason.
- `dom --text` returns the accessible name (whitespace-collapsed, `aria-label`-aware) rather
  than raw `textContent`. For a `<select>` that is now its label instead of the concatenation of
  every `<option>`.
- `a11y summary` / `--with-page` names are capped at 300 characters with a trailing `…` rather
  than 100 with a trailing `...`.
- `dom --count --query` and `dom stats|tree --query` are refused rather than silently ignoring
  the filter.

## Design notes

- **Why not "just use `--jq`".** The agents in the benchmark had `--jq` and never used it for
  this; writing a jaq path against a nested tree to find a substring is harder than the `eval`
  they reached for instead. `--query` is the one-word form of the question they were actually
  asking.
- **Why a cap on `page-text` now.** Every other read command has one (`snapshot`/`a11y`
  `--max-chars`, `console`/`dom`/`network` `--limit`). Uncapped `innerText` on a long article is
  tens of thousands of tokens; the agent's `| head -100` shows it knows that and pays a turn for
  it. With `--query` and `--full` both available, a default cap costs nothing when the agent
  knows what it wants.
- **Text (`--format text`) rendering** of `--query` results shows the match lines with the line
  number prefix, so a follow-up `page-text --full` can be scrolled to.

## Out of scope

- Fuzzy or semantic matching. Substring + regex is what `grep` gives the agent today and what it
  reached for.
- Changing `snapshot`'s default depth/cap; only filtering is added.

## References

- [[axi-benchmark-comparison]] — the `tabular_data_analysis`, `wikipedia_deep_extraction`, and
  `github_issue_investigation` trajectories
- [[iteration-210-act-and-see]] — refs in `a11y summary`, which `--query` must preserve
- `crates/ff-rdp-cli/src/commands/page_text.rs` (no cap today), `snapshot.rs`
  (`bound_snapshot_output`, the cap pattern to copy), `output_controls.rs`
