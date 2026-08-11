---
branch: iter-141/output-hygiene
date: 2026-08-09
depends_on:
  - kb/iterations/iteration-128-network-hint-always-present.md
dogfood_path: |
  ff-rdp launch --headless --port 6100
  ff-rdp navigate https://www.bbc.com/news --port 6100
  ff-rdp console --level error --format text --port 6100 | wc -c
  # → must be kilobytes, not 255KB of padding
  ff-rdp index https://www.gov.uk/browse --max-pages 3 --port 6100 > /tmp/idx.json
  jq . /tmp/idx.json
  # → must be a single valid JSON document
  ff-rdp a11y contrast --fail-only --format text --port 6100
  # → must not print a bare [] that hides capped:true
first_call_sites: []
status: planned
---

# Iteration 141: output hygiene — text padding, invalid JSON, snapshot economics

From [[dogfooding-session-63]]. The `--format text` mode that exists to save tokens can cost
~64 k tokens on a single call, and `index` emits JSON that `jq` cannot parse.

## Themes

### Theme A — `--format text` pads every row to the widest cell

```
$ ff-rdp console --level error --format text
39 lines, 255 KB — every row padded to 8725 columns
```

One long Firefox console message sets the column width for all 39 rows. Same on
`dom 'a,button' --format text --all` (attrs padded to ~400 chars). `dom` already truncates
class names, so the truncation logic exists — it just isn't applied to `message` and `attrs`.

Apply iter-128's `middle_ellipsis` (or the table renderer's existing cap) to every free-text
column, and bound total column width regardless of content.

### Theme B — `index` emits invalid JSON, and its robots.txt parser is wrong

```
$ ff-rdp index https://www.gov.uk/browse --max-pages 3 2>/dev/null > idx.json
$ python3 -c "import json; json.load(open('idx.json'))"
json.decoder.JSONDecodeError: Extra data: line 10 column 1
```

stdout contains the internal `navigate` result **and then** the index result — two concatenated
documents, so `| jq` breaks. A JSON-only CLI must emit exactly one document on stdout.

Separately, the robots.txt parser ignores user-agent grouping: gov.uk's `User-agent: *`
disallows only `/*/print$` and `/search/all*`, while `Disallow: /` belongs to
`User-agent: deepcrawl`. index applied the latter to itself and indexed **1 page instead of 3**.
Each URL is also enqueued twice.

### Theme C — `snapshot` is either near-empty or unaffordable

Default depth 6 on BBC News: 18 461 bytes, 25 `truncated` markers, and `"interactive": true`
exactly **once** on a page with 163 `<a>` and 28 `<button>`. `--depth 30`: 231 KB, still only
45 anchors. `--depth 30 --max-chars 500000`: **644 KB** (~160 k tokens), 40 % styled-component
class hashes. There is no usable middle setting.

Three sub-problems:
1. `truncated: true` is buried at line 3248 and absent from `meta` — the caller can't tell.
2. `--max-chars` help says "characters of text content" but it bounds the serialized tree.
3. Pretty-printing inflates 148 KB of data to 644 KB.

Make truncation visible in `meta`, fix the help to describe what the flag actually bounds, and
give the output a way to be affordable — a compact JSON mode, attribute filtering, or both
(see the feature gaps in [[dogfooding-session-63]]). `--format text` at 4.8 KB is currently the
only viable variant and is not the default.

### Theme D — `--format text` drops JSON metadata, including truncation flags

```
$ ff-rdp a11y contrast --fail-only --format text
[]
  -> ff-rdp screenshot -o contrast.png  # Take a screenshot of contrast issues
$ ff-rdp a11y contrast --fail-only --jq '{total,sampled,meta}'
{"total":0,"sampled":218,"meta":{"summary":{...,"capped":true}}}
```

Text mode loses `sampled: 218` and — critically — `capped: true`, so a truncated sample reads
as a clean bill of health. It then suggests screenshotting issues that don't exist. `dom` does
the same. Empty results should say "no failures found (218 sampled, capped)", never bare `[]`.

### Theme E — CSS syntax errors bypass the JSON envelope

`ff-rdp dom 'div[[['` → `error: Document.querySelectorAll: 'div[[[' is not a valid selector`
as plain text, while every other error is `{"error":…,"error_type":…}`. Route it through the
envelope with an appropriate `error_type` (`User`).

### Theme F — smaller output defects

- `sources` always returns `actor: ""`, breaking the documented chain to `inspect`; in text
  mode the URLs elide to identical-looking strings, so both informative columns are useless.
- `network` JSON returns 20 of N with no truncation flag (`--all` fixes it, but nothing says
  so); `--format text` totals disagree with `--detail` JSON.
- `--jq '.a.b.c'` on a missing path prints nothing and exits 0 — indistinguishable from an
  empty result.
- jq syntax errors report `error_type: "Internal"`; they are user errors.
- `--fields url,bogusfield` silently drops unknown fields.
- `cookies --format text` prints `expires` as raw epoch ms; `sameSite` blank.
- `storage localStorage --format text` prints nothing at all for 0 entries and has no header row.

Fix the ones that mislead (truncation flags, `error_type` misclassification, silently dropped
fields); the cosmetic ones are optional if the iteration is running long — say which were
deferred rather than ticking them.

**Theme F disposition (iter-141 implementation):**
- Fixed: `network`'s silent 20-cap on `slowest` now carries an explicit
  `slowest_truncated` marker (JSON and `--format text`) — see
  `build_network_summary` / `e2e_network_truncation_flag`.
- Fixed: jq syntax/compile/runtime errors and `--jq-strict` missing-path errors now
  report `error_type: "User"`, not `"Internal"` — see
  `OutputPipeline::finalize_with_hints` / `e2e_jq_error_type_is_user`.
- Not a bug: `--jq '.a.b.c'` on a missing path silently omitting output is the
  documented `JqMissingPolicy::SilentOmit` default ("least surprise for pipelines",
  iter-86 Theme D); `--jq-strict` is the existing, already-tested escape hatch that
  errors instead. No change needed.
- **Deferred** (cosmetic, no misleading data — new iteration plan needed):
  `sources`' `actor: ""` (the native `ThreadActor::list_sources` path already sets a
  real `actor`, but modern Firefox appears to always hit the JS-eval fallback, which
  cannot recover an actor ID — needs a live-Firefox investigation to confirm before
  attempting a fix, out of scope for this pass), `--fields url,bogusfield` silently
  dropping unknown field names, `cookies --format text`'s raw-epoch-ms `expires` /
  blank `sameSite`, `storage localStorage --format text`'s empty-with-no-header-row
  output, and `--format text` totals vs `--detail` JSON disagreement in `network`.

## Acceptance Criteria [8/8]

- [x] live_141_console_text_bounded: `console --level error --format text` on a page with a
      very long message stays bounded; no row padded to another row's width
- [x] live_141_index_single_json_document: `index` stdout parses as exactly one JSON document
- [x] live_141_index_robots_user_agent_groups: a robots.txt with a foreign-UA `Disallow: /` does
      not block our crawl
- [x] live_141_snapshot_truncation_in_meta: `meta` reports truncation and the effective bound
- [x] live_141_text_empty_result_keeps_metadata: `a11y contrast --fail-only --format text` with
      zero failures still reports sampled count and capped state
- [x] e2e_invalid_selector_json_envelope: invalid CSS returns `{"error":…,"error_type":"User"}`
- [x] e2e_network_truncation_flag: truncated `network` JSON carries an explicit marker
- [x] e2e_jq_error_type_is_user: jq syntax errors are not `error_type: "Internal"`

## Notes

- `--format text` exists to reduce token cost; a 255 KB table is a total inversion of its
  purpose, which makes Theme A the priority here.
- iter-140 reused `middle_ellipsis` (iter-128) a second time to bound a different unbounded
  string (its frame-scan error's URL list: `MAX_LISTED_FRAME_URLS`/`FRAME_URL_MAX_LEN` in
  `click.rs`) rather than writing a new truncation helper — Theme A here is the same shape
  (unbounded `message`/`attrs` text columns); reuse `middle_ellipsis` again instead of adding
  a third truncation implementation.
- iter-140 Theme E's bug (`click`'s `run()` used `.remove()` to move `frame_url` from
  `results` into `meta`, so the field `--help` documented as present in **both** silently
  never existed in `results`) was caught by literally running the `--jq` filter the `--help`
  text advertised as safe (`--jq '.results.frame_url'`) and observing it fail. Theme F here
  has the same class of bug (`sources`' `actor: ""`, `network`'s undocumented truncation
  flag) — worth grepping result/meta-building code for other `.remove(...)` calls, and adding
  an e2e test per fixed field that runs the exact `--jq` path the docs promise, the way
  iter-140's `click_jq_results_frame_url_does_not_throw` does.

## Run guidance (batch 138–142, from dogfooding session 63)

Non-negotiable working rules for whoever implements this plan:

1. **Do not trust the root cause stated above.** In iterations 135, 136, 137 and 140 the real
   cause differed from the plan's hypothesis, and more than once it was our bug, not
   Firefox's. iter-140's first-pass fix for its ref-invalidation Theme (narrowing
   `frameUpdate` handling to `isTopLevel: true`) still wasn't enough — live testing against
   Firefox 153 showed Fission spawns a fresh actor pair for the *same* committed URL on
   almost every RDP round-trip, so a same-URL-but-new-actor `frameUpdate` needed an explicit
   URL comparison against the last committed navigation to avoid being misread as a real nav.
   Reproduce the symptom and verify the mechanism **on the wire** (actual RDP packets / actual
   command output) before writing the fix. If the diagnosis here turns out to be wrong, fix
   the real cause and correct this section.
2. **A live test that passes `--no-daemon` proves nothing about the default path.** That is
   exactly how iter-129 shipped a feature that did not work at all. Every live test added
   here must exercise the default (daemon) path. iter-137 added the guard at
   `crates/ff-rdp-cli/tests/no_daemon_live_test_guard.rs` with a shrink-only grandfather
   list — **do not add entries to that list.**
3. Evidence for every finding in this plan — exact command and exact output — is in
   [[dogfooding-session-63]]. Read it before diagnosing.
