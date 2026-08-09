---
branch: iter-140/element-targeting
date: 2026-08-09
depends_on:
  - kb/iterations/iteration-137-daemon-mode-parity.md
dogfood_path: |
  ff-rdp launch --headless --port 6100
  ff-rdp navigate https://www.gov.uk --port 6100
  ff-rdp dom 'button' --port 6100
  ff-rdp click --ref e1 --port 6100        # → must click, not fail on an invalid selector
  ff-rdp styles --ref e2 --port 6100       # → must work twice in a row
  ff-rdp styles --ref e2 --port 6100
  ff-rdp type 'input[name=keywords]' 'passport' --port 6100
  # → must target the visible match, or say how many matched and which was chosen
first_call_sites: []
status: planned
---

# Iteration 140: element targeting — refs, ambiguous selectors, frame diagnostics

From [[dogfooding-session-63]]. `--ref` is advertised across many commands and is broken three
ways; ambiguous selectors silently pick a hidden element and then time out.

## Themes

### Theme A — `--ref` is broken three ways

```
$ ff-rdp dom 'button'                # registers e1..e28, prints a `ref` column
$ ff-rdp click --ref e1
{"error":"selector 'document.querySelectorAll('button')[0]' not ready after 10000ms:
 Document.querySelector: '...' is not a valid selector","error_type":"Timeout"}
$ ff-rdp styles --ref e2
{"error":"actor error: SyntaxError — Element.querySelector: '...' is not a valid selector"}
$ ff-rdp styles --ref e2             # second call, nothing changed
{"error":"ref e2 not found (not registered in this daemon session)","error_type":"User"}
```

1. Refs round-trip into a **JS expression** that is then fed to `querySelector`, which cannot
   parse it. Store and resolve a real element handle (or a genuinely unique selector).
2. The registry is **single-use** — a second resolve of the same ref fails. Refs are meant to
   be stable handles for a session; make repeated resolution work, and if a ref genuinely
   expires (navigation), say *that*, not "not registered".
3. `click --ref` additionally burns the full 10 s before failing, because the invalid selector
   goes down the wait-for-ready path.

This is a headline agent-ergonomics feature that does not work at all. Either fix it properly
or remove it from `--help` — but do not leave it advertised and broken.

### Theme B — ambiguous selectors pick blindly and fail opaquely

```
$ ff-rdp type 'input[name=keywords]' 'passport renewal'
{"error":"selector 'input[name=keywords]' not ready (not found / hidden / unstable) after 10000ms"}
$ ff-rdp geometry 'input[name=keywords]' --format text
input[name=keywords]  input  203.0  376.5  580.0  50.0  yes  yes    ← visible & in viewport
```

Two elements match. `type` takes `[0]` (hidden, `offsetParent === null`) and fails; `geometry`
reports only the visible one. **The two commands disagree about what a selector means.**

Three sub-problems: the blind `[0]` choice, an error that conflates not-found/hidden/unstable
and never says *how many* matched, and 10 s spent to learn nothing.

Fix: report the match count and which index was chosen; distinguish the three failure causes;
and add a way to recover — `--index N` / `--visible` (Theme C).

### Theme C — no way to disambiguate

There is no `--index`/`--nth`/`--visible` on `click`/`type`/`styles`. When the first match is
wrong the user has no recourse short of writing a more specific selector by hand — which
requires a `dom` round-trip that `--ref` was supposed to make unnecessary.

Add the minimum that makes Theme B recoverable. Prefer `--visible` as the ergonomic default
hint in errors, since "the visible one" is what a human means most of the time.

### Theme D — frame diagnostics: 65 KB errors and a miscount

- `click_in_scanned_frame`'s `all_urls()` (`crates/ff-rdp-cli/src/commands/click.rs:415-421`,
  used at :427 and :458) joins every frame URL raw. On theguardian.com with 97 frames the
  error message is **65 KB** of consent-string-laden ad URLs. iter-128's `middle_ellipsis`
  was never applied here; cap the count listed as well as each URL's length.
- `--frame` miscounts: with `--frame guim` and 97 frames it reports `matched in 0 of 97
  frames`, using `targets.len()` instead of the filtered `candidates.len()` — claiming to have
  tried 97 when `--frame` narrowed it to a handful.

### Theme E — `click`'s documented `results.frame_url` doesn't exist

`--help` states the output is `{"results": {..., "frame_url": null}}` and that it is "always
present (never omitted) — so `--jq '.results.frame_url'` never throws". Actual output puts it
in `meta` only, on both the top-frame and in-frame paths. Fix the code or the help so the
documented `--jq` filter works.

### Theme F — the generated page-map hands agents unusable selectors

`.ffrdp/page-map.json` for gov.uk contains `{"label":"Search","selector":"button","role":"button"}`.
`button` matches every button on the page. The page-map exists so an agent can skip discovery;
a non-unique selector guarantees the discovery turn happens anyway — and per Theme B,
`click "button"` will hit a hidden one. Generate selectors that are unique, or record the match
index alongside.

## Acceptance criteria

- [ ] live_140_ref_click_resolves: `click --ref eN` after `dom` clicks the right element
- [ ] live_140_ref_reusable: resolving the same ref twice in a row succeeds both times
- [ ] live_140_ref_expiry_message: a ref invalidated by navigation reports expiry, not
      "not registered"
- [ ] live_140_ambiguous_selector_reports_count: error on a 2-match selector names the match
      count and the chosen index, and distinguishes hidden from not-found
- [ ] live_140_visible_flag_targets_visible: `--visible` (or `--index`) reaches the visible
      match where the bare selector fails
- [ ] live_140_frame_error_bounded: frame-scan error on a many-frame page is bounded in size
- [ ] live_140_frame_filter_count_accurate: `--frame` reports the filtered candidate count
- [ ] e2e_click_frame_url_in_results: `--jq '.results.frame_url'` does not throw
- [ ] live_140_page_map_selectors_unique: generated page-map selectors resolve to exactly one
      element

## Notes

- Theme D depends on [[iteration-137-daemon-mode-parity]]: frame scanning returns zero targets
  through the daemon, so these paths cannot be exercised in the default mode until 137 lands.
