---
title: "Iteration 219: reader view on the live page — Readability.js makes --with-page return the content, not the chrome"
type: iteration
date: 2026-08-30
status: planned
branch: iter-219/reader-view-page
depends_on:
  - 210
  - 211
  - 212
first_call_sites:
  - primitive: ff_rdp_cli::commands::page_view::readability_js
    site: crates/ff-rdp-cli/src/commands/page_view.rs (build_page_view_js splices it in)
  - primitive: ff_rdp_cli::commands::page_view::excerpt_at_boundary
    site: crates/ff-rdp-cli/src/commands/page_view.rs::collect (builds `page.excerpt`)
  - primitive: xtask::check_vendored_js
    site: crates/xtask/src/main.rs (check-vendored-js subcommand; CI discipline job)
dogfood_path: |
  ff-rdp launch --headless
  ff-rdp navigate https://en.wikipedia.org/wiki/Ada_Lovelace --with-page --jq '.results.page.interactive[] | select(.name == "Charles Babbage") | {ref, zone}'
  # expected: one entry, zone "content", a ref like e7 — today this link is truncated away
  # behind ~1,200 chrome links ("Jump to content", "Main page", ...) and there is no hit
  ff-rdp navigate https://en.wikipedia.org/wiki/Ada_Lovelace --with-page --jq '.results.page | {readerable, source, excerpt_chars, excerpt: .excerpt[0:160]}'
  # expected: readerable true, source "readability", excerpt starts with the article's lede
  # ("Augusta Ada King, Countess of Lovelace ... was an English mathematician"), not the nav
  ff-rdp click --ref <that ref> --with-page --jq '.results.page.headings[0].text, (.results.page.excerpt | test("1791"))'
  # expected: "Charles Babbage" and true — the birth year is IN the returned page, so the
  # two-command trajectory answers wikipedia_link_follow without a page-text round trip
  ff-rdp navigate https://en.wikipedia.org/wiki/Ada_Lovelace --with-page --query Babbage --jq '.results.page.interactive | length'
  # expected: a handful, all matching — --query narrows the view the way it narrows page-text
  ff-rdp --help | sed -n '/Quick start/,/^$/p'
  # expected: the three idioms from skill_doc's IDIOMS table, verbatim, incl. `--query` and
  # `--with-page` — the surface 42/42 benchmark agents actually read
tags: [iteration, agent-ergonomics, page-view, readability, benchmark]
---

# Iteration 219: reader view on the live page

## Why

The 2026-08-30 re-measurement in [[axi-benchmark-comparison]] found that `--with-page`
([[iteration-210-act-and-see]]) does not shorten the click-through tasks even when agents use
it: `wikipedia_link_follow` used the flag in all three runs and still took 8 / 7 / 10 turns
(axi: 4). Two defects in what the view returns, both visible in one call on
`en.wikipedia.org/wiki/Ada_Lovelace`:

1. **`interactive` is the first 50 links in DOM order.** On a real page that is entirely site
   chrome — "Jump to content", "Main page", "Contents" … — and `interactive_total: 1659,
   interactive_truncated: true` hides the article's own links, including the one the task needs.
   `click --ref` from the view is therefore useless exactly where it matters.
2. **There is no text.** The answer to "report his birth date" lives in body text, so the agent
   fetches `page-text --query` anyway. `--with-page` answers "where can I click next"; the tasks
   also ask "what does the page say now". axi's `open`/`click` return both.

The research in [[main-content-extraction-crates]] settled *how* to decide what the content is:
Mozilla's `Readability.js` (Apache-2.0, the algorithm behind Firefox Reader View), **injected into
the live page** via the existing content `eval`. It runs on a clone of the real rendered DOM, so
JS-rendered pages, logged-in sessions and consent-dismissed overlays work; with
`serializer: el => el` it returns the article *element*, so "is this link inside the content" is a
containment test, not a heuristic. The Rust ports (dom_smoothie et al.) would need the whole
`outerHTML` shipped over RDP and score 8 F1 points lower; Firefox's own copy is reachable through
a chrome-scope eval but sits behind `ReaderMode`'s host blocklist (GitHub issue pages return
`null`) and unversioned `moz-src:///` internals. Vendoring is the only option where the output is
a function of the ff-rdp version alone. Downloading at runtime was rejected (remote code executed
in the user's session, offline failure, version drift — see the same note).

From `~/devel/mdget` (the shelved readability→Markdown CLI): its `truncate_output` (cut at a
paragraph/sentence boundary, never mid-word), the Wikipedia `[edit]`-link stripping, and the
junk-description guard on excerpts port directly; its benchmark lesson — agents learn flags from
`--help` alone, the skill file did not help — is why Theme E exists.

## Themes

- **A — Vendor Readability.js, pinned.** `crates/ff-rdp-cli/js/readability/` holds
  `Readability.min.js` (≈33 KB, produced once by hand from the upstream release; no Node in the
  build), `Readability.js` (unminified, for diagnosis), `Readability-readerable.js`, `LICENSE`
  (Apache-2.0) and `VERSION`. An xtask gate `check-vendored-js` recomputes SHA-256 of each file
  against `VERSION` and fails CI on any edit, so an upgrade is a deliberate, reviewable commit.
- **B — Content zones.** The page-view collector stamps every interactive element with a
  transient `data-ffrdp-id`, runs Readability on `document.cloneNode(true)`, and marks each
  entry `zone: "content"` if its id is under the returned article root, else `"chrome"`. The list
  is sorted content → chrome *before* the 50-cap; `chrome_omitted: N` and
  `interactive_truncated` stay honest. `isProbablyReaderable` becomes `page.readerable`.
- **C — Excerpt.** `page.excerpt`: the article's normalized `textContent`, cut at a sentence or
  paragraph boundary (mdget's `truncate_output`, ported) at `--page-chars N` (default 1500;
  `0` disables). `page.source` says `"readability"` or `"innertext"` (the fallback: `<main>` /
  `[role=main]` / `body` `innerText` head when Readability returns `null`, e.g. dashboards,
  forms, SPAs without prose). `--query <text>` narrows the excerpt to the match window (same
  `--context` semantics as `page-text --query`) *and* filters `interactive` by name/href — one
  flag agents already know from [[iteration-211-find-not-guess]].
- **D — Injection mechanics.** The 33 KB payload is evaluated once per document and cached on a
  closure-held handle (not a bare global a hostile page can shadow); subsequent `--with-page`
  calls on the same document pay only the collector. RDP eval bypasses page CSP (verified in
  [[main-content-extraction-crates]]); the collector still guards against pages that override
  `Array.prototype`/`JSON` by capturing built-ins at injection time.
- **E — `--help` Quick start from the IDIOMS table.** 42/42 benchmark runs open with
  `ff-rdp --help` (several `| head -50`); `--query` is invisible there today. Render the same
  three idioms `skill_doc.rs` feeds the home view into the top-level `--help` Quick start block,
  and extend `check-skill-drift` to cover it, so `--help`, `SKILL.md` and the home view cannot
  disagree. Fix the `page-text` one-liner to mention `--query` and the cap.
- **F — Measure.** The point of the iteration is the turn count, so it ends with the same
  harness as [[axi-benchmark-comparison]]: `wikipedia_link_follow` and `wikipedia_infobox_hop`,
  3 repeats each, ff-rdp condition only (~$1, ~10 min), plus the in-content Readability timing
  on Wikipedia, a GitHub issue page and a `<main>`-less SPA.

## Tasks

### A. Vendor and pin [0/4]
- [ ] Add `crates/ff-rdp-cli/js/readability/{Readability.min.js,Readability.js,Readability-readerable.js,LICENSE,VERSION}`
      from `@mozilla/readability` 0.6.0; `VERSION` records the npm version, source URL, and the
      SHA-256 of each file; `include_str!` them from `page_view.rs`
- [ ] `xtask check-vendored-js`: recompute and compare the hashes (Rust `sha2`, xtask-only dep);
      wire into CI's discipline job next to `check-skill-drift`
- [ ] Attribution: one line in README's third-party section and in `--version --verbose` (or
      wherever ff-rdp lists bundled licenses — add the section if none exists)
- [ ] `cargo deny check` clean (Apache-2.0 is already allowed; confirm no new Rust deps beyond
      `sha2` in xtask)

### B. Content zones [0/5]
- [ ] Collector stamps `data-ffrdp-id` on every interactive element before cloning and removes
      the attribute in a `finally` — the live DOM must be byte-identical afterwards (live test:
      `document.documentElement.outerHTML` before == after)
- [ ] Run `new Readability(clone, {serializer: el => el}).parse()`; collect the id set under the
      article root; `zone: "content" | "chrome"` on every `interactive` entry
- [ ] Sort content first, then chrome, stable within each; apply the cap after sorting; add
      `chrome_omitted` (count dropped by the cap that were chrome) next to `interactive_total`
- [ ] `page.readerable: bool` from `isProbablyReaderable(document)`; `page.source`
- [ ] Drop `landmarks` from `results.page` (22 entries of `{"role":"navigation","label":""}` on
      Wikipedia that no benchmark trajectory used); `a11y summary` keeps them — it is the
      accessibility surface, `--with-page` is the act-and-see surface

### C. Excerpt [0/4]
- [ ] `page.excerpt` from the article's `textContent` (whitespace-normalized), cut with a ported
      `excerpt_at_boundary(text, max_chars)` — paragraph, then sentence, then word boundary; unit
      tests ported from mdget's `truncate_output` tests
- [ ] `--page-chars N` on every `--with-page` command (default 1500, `0` = no excerpt), threaded
      through `page_view::CollectOptions`; `page.excerpt_chars` and `page.excerpt_truncated`
- [ ] Fallback when Readability returns `null`: `innerText` of `main` / `[role=main]` / `body`,
      same cut; `page.source: "innertext"` — never an empty excerpt on a page that has text
- [ ] `--query` applies to the view: excerpt becomes the `--context`-window around matches (reuse
      `page_text::build_excerpt`), `interactive` filtered by name/href match, `page.matches`
      reported; `--query` with `--with-page` on a command that has no `--query` today gains it
      via the shared `QueryArgs`

### D. Injection mechanics [0/3]
- [ ] Inject once per document: the collector checks for the cached handle first and sends the
      33 KB only when absent; `meta.page_readability_injected: bool` says which happened
- [ ] Capture built-ins at injection time; live test against a page that overrides
      `Array.prototype.forEach` and `JSON.stringify` still returns a correct view
- [ ] Record in-content timing (`performance.now()` around `parse()`) as `meta.page_parse_ms`;
      Design notes get the measured numbers for Wikipedia, a GitHub issue, and an SPA

### E. `--help` from the IDIOMS table [0/3]
- [ ] Top-level `--help` Quick start renders the IDIOMS entries (syntax + one-line why) after the
      launch lines; generated from the same table `skill_doc.rs` and `home.rs` use
- [ ] `check-skill-drift` covers the `--help` block (or a sibling `check-help-idioms` if the
      existing gate's diff shape does not fit)
- [ ] `page-text` command one-liner mentions `--query` and the 8 000-char cap; README command
      reference regenerated

### F. Measure [0/3]
- [ ] Live tests `tests/live/live_219_reader_view.rs`: on the recorded Wikipedia fixture page the
      "Charles Babbage" link is in `interactive` with `zone: "content"` and "Jump to content" is
      not in the top 50; `excerpt` contains the lede; DOM unchanged after collection; fallback
      path on a fixture with no prose
- [ ] Benchmark: `wikipedia_link_follow` + `wikipedia_infobox_hop`, `--repeat 3`, ff-rdp
      condition, same harness/model/prompt as [[axi-benchmark-comparison]] — recorded in the
      Outcome section whatever the number is
- [ ] Update `kb/research/axi-benchmark-comparison.md` with the two-task result and the
      `--with-page` adoption count in those six runs

## Acceptance Criteria [0/7]

- [ ] `ff-rdp navigate https://en.wikipedia.org/wiki/Ada_Lovelace --with-page` returns the
      "Charles Babbage" link with a usable `ref` and `zone: "content"` inside the 50-entry cap,
      and `excerpt` begins with the article lede (live test, recorded fixture)
- [ ] `click --ref <that ref> --with-page` returns a view whose `excerpt` contains "1791" —
      the birth year — so `wikipedia_link_follow` is answerable in two commands (live test)
- [ ] The live DOM is unchanged by collection: no `data-ffrdp-id` remains, `outerHTML` equal
      before and after (live test)
- [ ] `check-vendored-js` fails on a one-byte edit to `Readability.min.js` and passes on the
      committed tree; CI runs it
- [ ] Top-level `--help` shows `page-text --query` and `click --ref … --with-page` in Quick
      start, and `check-skill-drift` (or its sibling) fails when the table and `--help` diverge
      (e2e test)
- [ ] Benchmark: `wikipedia_link_follow` and `wikipedia_infobox_hop` have a measured 3-repeat
      average turn count with this binary recorded in Outcome. Target ≤ 5 (were 8.3 / 8.3 on
      2026-08-30). A number that did not improve is a valid result and must not be re-run until
      it looks better; if agents still did not use `--with-page`, say so — that reopens
      [[iteration-213-act-and-see-benchmark-rerun]] Theme C's default-on question with evidence
- [ ] `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -q`
      clean; live sweep reconciles

## Design notes

- **One flag, not two.** `--with-structure` + `--with-content-summary` was considered and
  rejected: agents find one opt-in flag 13 times in 42 runs; two flags that must be combined for
  the common case is a worse discoverability problem, and axi's advantage is precisely that its
  actions return refs *and* text with no decision to make. `--page-chars 0` is the
  "structure only" knob.
- **Why `cloneNode(true)`.** Readability mutates the tree it parses. Cloning costs ~ms on
  Wikipedia-sized pages and is what Firefox's own `ReaderMode` effectively does (it serializes
  and re-parses). Measure it (Theme D) rather than assume.
- **Stamp-and-strip vs. matching by href.** Href matching fails on duplicate links (Wikipedia
  has the same article linked from the infobox, body and navbox) and on `href="#"` buttons.
  Transient ids are exact; the `finally` strip plus the outerHTML-equality live test keeps the
  "no side effects on the page" promise `--with-page` has made since 210.
- **Where the 50-cap lands.** After sorting: content links first, so a page with 30 content
  links and 1,600 chrome links returns all 30 plus 20 chrome. `chrome_omitted` tells the agent
  the nav exists; `--query` reaches it ("Log in").
- **Not using Firefox's own Readability.** Viable (content-process target + `ReaderMode
  .parseDocument`, verified) but rejected: host blocklist (GitHub issues → `null`), unversioned
  `moz-src:///` module path, two extra RDP hops, identical output. Recorded in
  [[main-content-extraction-crates]]; may be a spike if vendoring is ever disallowed.
- **Minified in the tree, produced by hand.** The repo forbids Node in the build. The minified
  file is checked in with the unminified one beside it; `check-vendored-js` pins both. Upgrading
  is: download release, minify locally, update `VERSION` hashes, commit — a reviewable diff.
- **Spec drift.** The injected script runs through the console actor's `evaluateJSAsync`, an
  existing spec'd call; no new RDP surface, no `allow-spec-drift` needed.
- **mdget carry-overs applied**: sentence-boundary truncation (`truncate_output`), `[edit]`
  stripping for MediaWiki pages (`strip_edit_links` — apply to the excerpt only, it is
  Wikipedia-specific but harmless elsewhere), junk-description guard (an excerpt that is only a
  cookie banner or "Skip to content" is replaced by the next paragraph).

## Out of scope

- **Defaulting `--with-page` on.** That decision belongs to [[iteration-213-act-and-see-benchmark-rerun]]
  Theme C and needs Theme F's numbers first; a measurement iteration must not become a
  behaviour-change one and vice versa.
- **`ff-rdp read` — a Markdown reader-view command** (mdget on the live DOM). Natural follow-up
  once the injection exists; not needed for the benchmark gap. File as its own plan if wanted.
- **Renaming `--with-page`.** The payload now matches the name.
- Cross-page "this link is on every page" chrome detection (needs daemon state).
- Landing the benchmark harness in `tools/` — [[iteration-213-act-and-see-benchmark-rerun]]
  Theme A; Theme F here runs it from wherever it lives and says so.

## References

- [[axi-benchmark-comparison]] — the 2026-08-30 re-measurement, adoption map, `link_follow`
  diagnosis
- [[main-content-extraction-crates]] — crate survey, Readability.js injection rationale,
  privileged-route verification
- [[iteration-210-act-and-see]] — introduced `--with-page` and `page_view.rs`
- [[iteration-211-find-not-guess]] — `--query`, `QueryArgs`, `page_text::build_excerpt`
- [[iteration-212-ambient-context]] — `skill_doc.rs` IDIOMS table, `check-skill-drift`
- [[iteration-213-act-and-see-benchmark-rerun]] — owns the default-on decision and the harness
- `~/devel/mdget/crates/mdget-core/src/extract.rs` — `truncate_output`, `strip_edit_links`,
  `looks_like_junk_description`
- https://github.com/mozilla/readability — `@mozilla/readability` 0.6.0, Apache-2.0
