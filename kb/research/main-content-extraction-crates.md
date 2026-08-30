---
title: "Main-content extraction for the live page view: Rust crates vs Readability.js injection"
type: research
date: 2026-08-30
status: completed
tags:
  - research
  - readability
  - page-view
  - agent-ergonomics
  - dependencies
---

# Main-content extraction for the live page view

## Why

[[axi-benchmark-comparison]] (2026-08-30 re-measurement) showed `--with-page`'s view is not
worth returning on real pages: `interactive` is the first 50 links in DOM order — on Wikipedia
all site chrome, the article's own links among the 1,609 truncated — and there is no body text.
Fixing both needs a "what is the main content" decision. Prior art: `~/devel/mdget` (readability
→ Markdown for static fetches; shelved because WebFetch covers that case). Its 2026-04 crate
survey chose `dom_smoothie`. This note re-surveys as of 2026-08-30 with two concrete needs:

1. a short excerpt of the main content, degrading gracefully on non-article pages;
2. knowing which `<a href>` are *inside* the main content, to rank refs content-first.

ff-rdp's `Cargo.lock` has no HTML parser today (only `regex`); every Rust candidate adds ~8–10
crates. `deny.toml` permits MIT/Apache/BSD/ISC/MPL-2.0/Zlib/Unicode/CDLA-P-2.0 — GPL
(`article_scraper`), AGPL (`nws`) and UPL-1.0 (`readability-js`) are out.

## Survey

| Crate | Ver / released | ★ | License | Algorithm | Quality evidence | Content root reachable? | Output | Parser deps |
|---|---|---|---|---|---|---|---|---|
| **dom_smoothie** | 0.18.0 / 2026-06 (commit 2026-08) | 217 | MIT | Readability.js port | 91 Mozilla fixtures vendored; ScrapingHub F1 0.865 (Readability.js 0.947) | Yes, undocumented: `Readability.doc` is pub and after `parse()` holds only the retained subtree; `data-*` attrs survive (`class` stripped unless preserved) | HTML/text/Markdown | dom_query → html5ever, selectors |
| **readabilityrs** | 0.1.4 / 2026-08-10 | 89 | Apache-2.0 | Readability.js port | **119/130** Mozilla fixtures, divergences enumerated | Partial: `raw_content` = winning subtree HTML as a string; re-parse to get hrefs | HTML/text/Markdown | scraper, ego-tree |
| **legible** | 0.5.1 / 2026-08 (commit 2026-08-28) | 2 | Apache-2.0 | Readability-descended + shape-specific extractors (docs, listings, discussion) | own fixtures (Wikipedia/GitHub/MDN/Reddit shapes); no pass rate published | No (private); `ContentHint` can force a root | HTML/text/Markdown | html5ever |
| trafilatura (nchapman) | 0.3.0 / 2026-03, dormant | 7 | Apache-2.0 | Trafilatura port + libreadability/justext fallback | self-reported F1 0.913 | `content_html` string with links | text/HTML/MD | heavy (scraper, justext, chrono, whatlang…) |
| rs-trafilatura | 0.2.2 / 2026-04 | 55 | MIT/Apache | Trafilatura + XGBoost page classifier | ScrapingHub F1 0.970 (article-only board) | No | text/HTML/GFM | dom_query + classifier |
| dom-content-extraction | 0.4.4 / 2026-07 | 45 | MPL-2.0 | CETD text density | CleanEval F1 0.78 | **Best API**: per-node density + `NodeId` | text/MD (htmd) | scraper, ego-tree |
| justext | 0.2.0 / 2026-02 | 0 | BSD-2 | jusText paragraphs | 0.804 | paragraph xpath + class only | text | scraper |
| libreadability, readex, llm_readability, readability (kumabook), readable-readability, boilerpipe, readability-rust | 2021–2026 | — | — | — | unverified / abandoned / <300 dl; kumabook's returns "Hacker News" on HN | — | — | — |
| readability-js | 0.1.5 / 2025-10 | 7 | **UPL-1.0** | real Readability.js in QuickJS | 100% by construction | strings only | HTML/text | rquickjs |
| Converters only: htmd 0.5.5 (3.5M dl), html2text 0.17, mdka, fast_html2md | active | | Apache/MIT | — | — | n/a | | html5ever |

No maintained wasm build of Readability.js exists, and nothing wraps Gecko's reader-mode
internals; `isProbablyReaderable` is reimplemented in dom_smoothie and legible.

## Assessment

- **Rust side: dom_smoothie remains the answer**, for a reason the mdget survey did not
  need: its article `Document` is reachable, so stamping elements with `data-ffrdp-id` in the
  page before pulling `outerHTML` makes need (2) a set-membership test after `parse()`. Cost:
  F1 8 points behind real Readability.js, ~10 new crates, and a full `outerHTML` transfer
  (0.5–1 MB on Wikipedia) per call.
- **readabilityrs** is the higher-fidelity port and the only honest pass-rate claim; swap in if
  dom_smoothie's precision hurts excerpts. **legible** is the only crate designed for the
  non-article shapes we also hit (dashboards, GitHub, docs) — too young to depend on;
  re-evaluate in a quarter.
- **Text-density (CETD, jusText) is the wrong tool for this problem**: it scores badly on
  link-dense regions, which is exactly where content/chrome separation matters.

## Recommendation: inject Mozilla's Readability.js into the live page

`@mozilla/readability` 0.6.0 — Apache-2.0 (deny-compatible; vendor with LICENSE), 90 KB
unminified (2.1k lines) + 4.3 KB `Readability-readerable.js`, ~10–40 ms in-content.
`new Readability(document.cloneNode(true), { serializer: el => el }).parse()` returns the
**article root element**; the `a[href]` under it are the content-link set, matched to the
collector's refs by a `data-ffrdp-id` stamped in the same JS pass. `isProbablyReaderable`
labels the excerpt's confidence on non-article pages, where the view falls back to
`innerText` head + landmark containment.

Why this over the Rust route: no DOM serialization leaves the tab; it is Firefox's own
reader-mode algorithm at 100% fidelity instead of 92%; zero new Rust dependencies. Costs:
90 KB of vendored JS in the binary (JS helpers are already embedded), one more `evaluateJS`
round trip (RDP eval bypasses page CSP; a hostile page can still shadow globals — inject into
a closure, cache once per document), and no offline fixture testing — consistent with this
repo's recorded-from-real-Firefox rule rather than a regression from it.

What to take from mdget regardless of extractor: `mdget-core/src/extract.rs`'s
post-processing — sentence-boundary `truncate_output`, Wikipedia `[edit]`-link stripping,
degenerate-table cleanup — for the excerpt.

Decision to be taken in the plan that implements it (see [[iteration-213-act-and-see-benchmark-rerun]]
Theme C follow-up): prototype both routes in the dogfood script on Wikipedia/Ada_Lovelace,
GitHub issues, and a `<main>`-less SPA; record in-content time and whether the Babbage link
lands in the top 50. Sources: Schwartz "Comparing 13 Rust crates for extracting text from HTML"
(2025), ScrapingHub article-extraction-benchmark, the crates' repos as of 2026-08-30.
