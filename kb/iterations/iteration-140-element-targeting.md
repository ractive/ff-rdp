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
status: in-progress
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

## Acceptance Criteria [9/9]

- [x] live_140_ref_click_resolves: `click --ref eN` after `dom` clicks the right element
- [x] live_140_ref_reusable: resolving the same ref twice in a row succeeds both times
- [x] live_140_ref_expiry_message: a ref invalidated by navigation reports expiry, not
      "not registered"
- [x] live_140_ambiguous_selector_reports_count: error on a 2-match selector names the match
      count and the chosen index, and distinguishes hidden from not-found
- [x] live_140_visible_flag_targets_visible: `--visible` (or `--index`) reaches the visible
      match where the bare selector fails
- [x] live_140_frame_error_bounded: frame-scan error on a many-frame page is bounded in size
- [x] live_140_frame_filter_count_accurate: `--frame` reports the filtered candidate count
- [x] e2e_click_frame_url_in_results: `--jq '.results.frame_url'` does not throw
- [x] live_140_page_map_selectors_unique: generated page-map selectors resolve to exactly one
      element

## Notes

- Theme D depended on [[iteration-137-daemon-mode-parity]] for frame scanning to return
  non-zero targets through the daemon. **137 has since merged** (iter-138 and iter-139 both
  landed on top of it using the default daemon path throughout), so this dependency is
  resolved — Theme D's live tests can run against the default (daemon) connection mode with
  no blocker. Still verify frame-scan-through-daemon behavior on the wire before relying on
  it (per run-guidance rule 1); don't assume 137's fix generalizes to every frame-count shape
  without checking.
- iter-139 (perf honesty II) reused an existing shared helper (`middle_ellipsis`) rather than
  writing a second truncation helper, and centralized repeated field-shape logic behind one
  small spec struct (`UnavailableMetricSpec`) instead of duplicating it across `perf
  vitals`/`perf audit`/`perf compare`. Theme D here has the identical shape — reuse
  `middle_ellipsis` for the frame-URL list truncation (already flagged above); if Theme A's
  ref-resolution fix and Theme B's disambiguation fix end up sharing field names/thresholds
  across `click`/`type`/`styles`, prefer one shared spec/helper over three copies for the
  same reason.

## Run guidance (batch 138–142, from dogfooding session 63)

Non-negotiable working rules for whoever implements this plan:

1. **Do not trust the root cause stated above.** In iterations 135, 136 and 137 the real
   cause differed from the plan's hypothesis three times running, and twice it was our bug,
   not Firefox's. Reproduce the symptom and verify the mechanism **on the wire** (actual RDP
   packets / actual command output) before writing the fix. If the diagnosis here turns out
   to be wrong, fix the real cause and correct this section.
2. **A live test that passes `--no-daemon` proves nothing about the default path.** That is
   exactly how iter-129 shipped a feature that did not work at all. Every live test added
   here must exercise the default (daemon) path. iter-137 added the guard at
   `crates/ff-rdp-cli/tests/no_daemon_live_test_guard.rs` with a shrink-only grandfather
   list — **do not add entries to that list.**
3. Evidence for every finding in this plan — exact command and exact output — is in
   [[dogfooding-session-63]]. Read it before diagnosing.
